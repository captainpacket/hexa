//! The run record (ADR-2611051850, phase 1): one file per hexa run at
//! `~/.hexa/runs/<id>.json`, written when the run starts and closed when it
//! ends, so "what is running, and how did the last ones end" is read from
//! files and never from `ps` or a log.
//!
//! The record says what the run is (`verb`, `target`, `gate`, `worktree`,
//! `log`), which process owns it (`pid` with `pid_start`, the pair that names
//! one process across pid reuse), and how it ended (`outcome`, `commit`). A
//! record still `running` whose process is gone is reported `abandoned`: a
//! crashed or killed run never wrote its own ending, and counting it as
//! running is the lie this file exists to stop.
//!
//! It is a separate file from `~/.hexa/agent-runs.jsonl`, which already holds
//! subagent start/stop rows and one row per finished `hexa do` run. That feed
//! has no pid, no start time and no running state, and its rows are appended
//! after the fact; `hexa status` reads it and this directory together and
//! does not copy one into the other.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Environment a detaching parent sets on its child: the id of the record the
/// parent wrote, so the child completes that record instead of starting a
/// second one for the same run.
pub const ENV_RUN_ID: &str = "HEXA_RUN_ID";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunRecord {
    pub id: String,
    pub verb: String,
    pub target: String,
    pub pid: u64,
    /// `None` when the platform gave no start time for the pid; then
    /// `pid_start_unknown` is true and pid-reuse protection does not apply.
    pub pid_start: Option<u64>,
    #[serde(default)]
    pub pid_start_unknown: bool,
    pub started: String,
    /// `None` while the run is in progress.
    pub ended: Option<String>,
    /// `running|passed|failed|killed|abandoned|unknown`
    pub outcome: String,
    pub commit: Option<String>,
    pub worktree: Option<String>,
    pub gate: Option<String>,
    pub log: Option<String>,
    /// The task-ledger id (phase 2). Always null in phase 1.
    #[serde(default)]
    pub task: Option<String>,
    /// What the last gate run said (ADR-2611151645 §6): `hexa status` and the
    /// next session can say why a pass failed without running the gate again.
    #[serde(default)]
    pub gate_result: Option<GateResult>,
}

/// The last gate run of a pass: whether it passed, how long it took, and the
/// end of its output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub passed: bool,
    pub secs: u64,
    pub tail: String,
}

/// `<state dir>/runs`: the directory of both the records and the detached
/// runs' logs.
pub fn runs_dir() -> PathBuf {
    hexa_exec::state_dir().join("runs")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn new_id(verb: &str) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let rnd = uuid::Uuid::new_v4().simple().to_string();
    format!("{verb}-{stamp}-{}", &rnd[..6])
}

fn path_of(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Write the record atomically: a reader sees the old file or the new one,
/// never half of either.
pub fn write(dir: &Path, rec: &RunRecord) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{}.json.tmp", rec.id));
    std::fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path_of(dir, &rec.id)).map_err(|e| format!("rename to {}: {e}", rec.id))
}

fn read_one(dir: &Path, id: &str) -> Option<RunRecord> {
    serde_json::from_str(&std::fs::read_to_string(path_of(dir, id)).ok()?).ok()
}

fn this_process(
    verb: &str,
    target: &str,
    gate: Option<&str>,
    id: String,
    log: Option<String>,
) -> RunRecord {
    let pid = u64::from(std::process::id());
    let pid_start = crate::commands::loop_cmd::pid_start(pid);
    RunRecord {
        id,
        verb: verb.to_string(),
        target: target.to_string(),
        pid,
        pid_start,
        pid_start_unknown: pid_start.is_none(),
        started: now(),
        ended: None,
        outcome: "running".to_string(),
        commit: None,
        worktree: std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string()),
        gate: gate.map(str::to_string),
        log,
        task: None,
        gate_result: None,
    }
}

/// A run in this process. `begin` writes the record; `finish` closes it. A
/// guard dropped without `finish` (an early `?`, a panic) closes it `failed`,
/// so an error path cannot leave a record claiming to run. A process killed
/// outright leaves `running`, which readers report as `abandoned`.
///
/// Writing is best effort and loud: a record that cannot be written is said
/// on stderr and the run goes on, because a ledger problem must not fail the
/// work, and a silent one would make a later "0 runs" a lie.
pub struct RunGuard {
    dir: PathBuf,
    rec: Option<RunRecord>,
}

impl RunGuard {
    pub fn begin(verb: &str, target: &str, gate: Option<&str>) -> RunGuard {
        Self::begin_in(
            &runs_dir(),
            verb,
            target,
            gate,
            std::env::var(ENV_RUN_ID).ok().filter(|v| !v.is_empty()),
        )
    }

    /// `begin` with the directory and the parent's record id passed in, so
    /// tests do not touch the process environment.
    pub fn begin_in(
        dir: &Path,
        verb: &str,
        target: &str,
        gate: Option<&str>,
        parent_id: Option<String>,
    ) -> RunGuard {
        // A detached child completes its parent's record: same id and log,
        // but the pid and start time of the process that does the work.
        let inherited = parent_id.as_deref().and_then(|id| read_one(dir, id));
        let rec = match inherited {
            Some(parent) => {
                let mut mine = this_process(
                    &parent.verb,
                    &parent.target,
                    parent.gate.as_deref(),
                    parent.id,
                    parent.log,
                );
                mine.started = parent.started;
                mine
            }
            None => this_process(verb, target, gate, new_id(verb), None),
        };
        if let Err(e) = write(dir, &rec) {
            eprintln!("hexa: could not write the run record ({e}); this run will not appear in `hexa status`");
        }
        RunGuard {
            dir: dir.to_path_buf(),
            rec: Some(rec),
        }
    }

    pub fn id(&self) -> &str {
        self.rec.as_ref().map(|r| r.id.as_str()).unwrap_or("")
    }

    /// Keep what the pass's last gate run said (ADR-2611151645 §6).
    pub fn set_gate_result(&mut self, passed: bool, secs: u64, tail: &str) {
        if let Some(r) = self.rec.as_mut() {
            r.gate_result = Some(GateResult { passed, secs, tail: tail.to_string() });
        }
    }

    pub fn set_commit(&mut self, sha: &str) {
        if let Some(r) = self.rec.as_mut() {
            r.commit = Some(sha.to_string());
        }
    }

    /// Close the record with `outcome` (`passed`, `failed` or `killed`).
    pub fn finish(mut self, outcome: &str) {
        self.close(outcome);
    }

    /// Close from a `Result` that has no finer verdict: `Ok` passed, `Err` failed.
    pub fn finish_result<T, E>(self, r: &Result<T, E>) {
        self.finish(if r.is_ok() { "passed" } else { "failed" });
    }

    fn close(&mut self, outcome: &str) {
        if let Some(mut r) = self.rec.take() {
            r.ended = Some(now());
            r.outcome = outcome.to_string();
            if let Err(e) = write(&self.dir, &r) {
                eprintln!(
                    "hexa: could not close the run record {} ({e}); it will read as abandoned",
                    r.id
                );
            }
        }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.close("failed");
    }
}

/// The parent of a detached run writes the record before it spawns, with
/// `pid` 0 (no process yet), so a child that dies before it can write is still
/// listed, as `abandoned`. Returns the id to hand the child in [`ENV_RUN_ID`].
pub fn begin_detached(
    verb: &str,
    target: &str,
    gate: Option<&str>,
    log: &Path,
) -> Result<String, String> {
    begin_detached_in(&runs_dir(), verb, target, gate, log)
}

pub fn begin_detached_in(
    dir: &Path,
    verb: &str,
    target: &str,
    gate: Option<&str>,
    log: &Path,
) -> Result<String, String> {
    let mut rec = this_process(
        verb,
        target,
        gate,
        new_id(verb),
        Some(log.display().to_string()),
    );
    rec.pid = 0;
    rec.pid_start = None;
    rec.pid_start_unknown = true;
    write(dir, &rec)?;
    Ok(rec.id)
}

/// The run record of a registered task (ADR-2611051850 phase 2/3). It names the
/// process that registered (`pid` with its start time), but whether the task
/// is live is the ledger's to say: the registering process is a short-lived
/// CLI call, so `hexa status` reads a task-bound record through its task and
/// never calls it `abandoned` by pid.
pub fn begin_for_task(task_id: &str, name: &str, gate: Option<&str>, worktree: Option<&str>) -> Result<String, String> {
    begin_for_task_in(&runs_dir(), task_id, name, gate, worktree)
}

pub fn begin_for_task_in(
    dir: &Path,
    task_id: &str,
    name: &str,
    gate: Option<&str>,
    worktree: Option<&str>,
) -> Result<String, String> {
    let mut rec = this_process("agent", name, gate, new_id("agent"), None);
    rec.task = Some(task_id.to_string());
    if let Some(w) = worktree {
        rec.worktree = Some(w.to_string());
    }
    write(dir, &rec)?;
    Ok(rec.id)
}

/// Close the record of a task with its outcome and commit. Errs when the record
/// is missing or cannot be read: the caller says so, it does not pretend.
pub fn close_task_run(id: &str, outcome: &str, commit: Option<&str>) -> Result<(), String> {
    close_task_run_in(&runs_dir(), id, outcome, commit)
}

pub fn close_task_run_in(dir: &Path, id: &str, outcome: &str, commit: Option<&str>) -> Result<(), String> {
    let mut rec = read_one(dir, id).ok_or_else(|| format!("run record {id} is missing or unreadable in {}", dir.display()))?;
    rec.ended = Some(now());
    rec.outcome = outcome.to_string();
    if let Some(c) = commit {
        rec.commit = Some(c.to_string());
    }
    write(dir, &rec)
}

/// Is the process a record names still the one running? The pid must exist
/// (a permission error means it exists), and when both start times are known
/// they must match: a recycled pid is somebody else's process.
pub fn process_alive(pid: u64, recorded_start: Option<u64>) -> bool {
    if pid == 0 {
        return false;
    }
    let Ok(p) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the pid can be signalled.
    let exists = unsafe { libc::kill(p, 0) } == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    if !exists {
        return false;
    }
    match (recorded_start, crate::commands::loop_cmd::pid_start(pid)) {
        (Some(then), Some(now)) => then == now,
        _ => true,
    }
}

/// What a record means now: its own outcome once it has ended, else `running`
/// while its process lives and `abandoned` when it does not.
pub fn effective_state(rec: &RunRecord, alive: &dyn Fn(u64, Option<u64>) -> bool) -> String {
    if rec.ended.is_some() || rec.outcome != "running" {
        return rec.outcome.clone();
    }
    if alive(rec.pid, rec.pid_start) {
        "running"
    } else {
        "abandoned"
    }
    .to_string()
}

/// Every record in `dir`, plus the files that could not be read. A directory
/// that is absent holds no runs; a directory or file that cannot be read is
/// reported, never counted as zero.
#[derive(Debug, Default)]
pub struct RunsRead {
    pub records: Vec<RunRecord>,
    /// `(file name, why)` for each `*.json` that did not read or parse.
    pub unreadable: Vec<(String, String)>,
}

pub fn read_all(dir: &Path) -> Result<RunsRead, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(RunsRead::default()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut out = RunsRead::default();
    for ent in entries {
        let ent = ent.map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
        let name = ent.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        let parsed = std::fs::read_to_string(ent.path())
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<RunRecord>(&t).map_err(|e| e.to_string()));
        match parsed {
            Ok(r) => out.records.push(r),
            Err(why) => out.unreadable.push((name, why)),
        }
    }
    out.records
        .sort_by(|a, b| a.started.cmp(&b.started).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Run `f` with `HEXA_HOME` set to `home` (or removed, for `None`), and put it back after, even
/// if `f` panics. The process environment is shared and the test harness runs tests on parallel
/// threads, so a test that sets it, or reads a path derived from it (`runs_dir`, `state_dir`),
/// must take this scope: one test at a time holds the variable, and every other test that reads
/// it waits. A test that reads it without the scope can see another test's temporary directory.
#[cfg(test)]
pub(crate) fn with_env_home<T>(home: Option<&Path>, f: impl FnOnce() -> T) -> T {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("HEXA_HOME", v),
                None => std::env::remove_var("HEXA_HOME"),
            }
        }
    }
    // A test that panicked inside the scope poisons the lock; the environment is already restored.
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _restore = Restore(std::env::var_os("HEXA_HOME"));
    match home {
        Some(h) => std::env::set_var("HEXA_HOME", h),
        None => std::env::remove_var("HEXA_HOME"),
    }
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn begin_writes_a_running_record_for_this_process_and_finish_closes_it() {
        let d = tmp();
        let mut g = RunGuard::begin_in(d.path(), "do", "src/x.rs", Some("cargo test"), None);
        let id = g.id().to_string();
        let r: RunRecord = read_one(d.path(), &id).expect("record exists while running");
        assert_eq!(r.outcome, "running");
        assert!(r.ended.is_none());
        assert_eq!(r.pid, u64::from(std::process::id()));
        assert_eq!(r.gate.as_deref(), Some("cargo test"));
        assert_eq!(effective_state(&r, &process_alive), "running");
        g.set_commit("abc1234");
        g.finish("passed");
        let r = read_one(d.path(), &id).unwrap();
        assert_eq!(
            (r.outcome.as_str(), r.commit.as_deref()),
            ("passed", Some("abc1234"))
        );
        assert!(r.ended.is_some());
    }

    #[test]
    fn a_guard_dropped_without_finish_is_failed_not_left_running() {
        let d = tmp();
        let id = {
            let g = RunGuard::begin_in(d.path(), "build", "t", None, None);
            g.id().to_string()
        };
        let r = read_one(d.path(), &id).unwrap();
        assert_eq!(r.outcome, "failed");
        assert!(r.ended.is_some());
    }

    #[test]
    fn a_running_record_with_a_dead_pid_is_abandoned_not_running() {
        let mut r = this_process("harden", "t", None, "x".into(), None);
        r.pid = 4_000_000_000; // above any pid_max
        assert_eq!(effective_state(&r, &process_alive), "abandoned");
        r.pid = 0;
        assert_eq!(effective_state(&r, &process_alive), "abandoned");
    }

    #[test]
    fn a_recycled_pid_with_a_different_start_is_not_the_run() {
        let me = u64::from(std::process::id());
        let start = crate::commands::loop_cmd::pid_start(me);
        assert!(process_alive(me, start));
        if let Some(s) = start {
            assert!(
                !process_alive(me, Some(s + 1)),
                "same pid, other start time: a different process"
            );
        }
    }

    #[test]
    fn a_detached_child_completes_its_parents_record_and_makes_no_second_one() {
        let d = tmp();
        let log = d.path().join("harden-x.log");
        let id = begin_detached_in(d.path(), "harden", "src/x.rs", Some("g"), &log).unwrap();
        let before = read_one(d.path(), &id).unwrap();
        assert_eq!(before.pid, 0, "no process yet");
        assert_eq!(effective_state(&before, &process_alive), "abandoned");
        let g = RunGuard::begin_in(d.path(), "harden", "src/x.rs", Some("g"), Some(id.clone()));
        assert_eq!(g.id(), id);
        let mid = read_one(d.path(), &id).unwrap();
        assert_eq!(mid.pid, u64::from(std::process::id()));
        assert_eq!(mid.log.as_deref(), Some(log.to_str().unwrap()));
        g.finish("passed");
        let all = read_all(d.path()).unwrap();
        assert_eq!(all.records.len(), 1, "one run, one record");
        assert_eq!(all.records[0].outcome, "passed");
    }

    #[test]
    fn an_unreadable_record_is_reported_and_an_unreadable_dir_is_an_error_not_zero() {
        let d = tmp();
        std::fs::write(d.path().join("bad.json"), "{not json").unwrap();
        std::fs::write(d.path().join("run.log"), "ignored").unwrap();
        let r = read_all(d.path()).unwrap();
        assert!(r.records.is_empty());
        assert_eq!(r.unreadable.len(), 1, "{:?}", r.unreadable);
        assert_eq!(r.unreadable[0].0, "bad.json");
        // A path that is a file, not a directory, cannot be listed.
        let f = d.path().join("afile");
        std::fs::write(&f, "x").unwrap();
        assert!(read_all(&f).is_err());
        // No directory at all is a true zero.
        assert!(read_all(&d.path().join("nope")).unwrap().records.is_empty());
    }

    #[test]
    fn an_env_scope_excludes_every_other_scope_so_no_test_reads_anothers_home() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let other = tmp();
        let holder_done = Arc::new(AtomicBool::new(false));
        let (entered, wait) = std::sync::mpsc::channel();
        let holder = {
            let (home, done) = (other.path().to_path_buf(), holder_done.clone());
            std::thread::spawn(move || {
                with_env_home(Some(&home), || {
                    entered.send(()).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let mine = std::env::var_os("HEXA_HOME");
                    done.store(true, Ordering::SeqCst);
                    mine
                })
            })
        };
        wait.recv().unwrap(); // the holder is inside its scope, with HEXA_HOME at `other`
        // A second scope must not start its body until the holder's body has finished.
        let (waited, seen) = with_env_home(None, || (holder_done.load(Ordering::SeqCst), std::env::var_os("HEXA_HOME")));
        assert!(waited, "the second scope ran its body while the first still held HEXA_HOME");
        assert_eq!(seen, None);
        assert_eq!(holder.join().unwrap(), Some(other.path().into()), "the holder saw only its own value");
    }

    #[test]
    fn an_env_scope_restores_the_variable_when_its_body_panics() {
        let marker = tmp();
        let before = with_env_home(Some(marker.path()), || std::env::var_os("HEXA_HOME"));
        assert_eq!(before, Some(marker.path().into()));
        let r = std::panic::catch_unwind(|| {
            let d = tempfile::tempdir().unwrap();
            with_env_home(Some(d.path()), || panic!("a failing test inside the scope"))
        });
        assert!(r.is_err());
        // The panic poisoned the lock and must not have left the variable pointing at a deleted directory.
        let after = with_env_home(None, || std::env::var_os("HEXA_HOME"));
        assert_eq!(after, None);
    }
}
