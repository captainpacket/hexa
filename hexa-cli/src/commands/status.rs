//! `hexa status` — where the work stands (ADR-2611051850, phase 1).
//!
//! One report, two renderings. JSON when `--json` is given or stdout is not a
//! terminal (an agent reading a pipe gets the machine form without asking);
//! text for a person at a terminal. The report holds:
//!
//! - the project and git state (branch, head, dirty, ahead/behind; `--remote`
//!   also asks the remote, opt-in because it touches the network),
//! - the loop (ADR, stage, gate, checklist),
//! - the runs in progress, from `~/.hexa/runs/<id>.json`,
//! - the subagents in progress, from the start/stop rows the hooks already
//!   append to `~/.hexa/agent-runs.jsonl`,
//! - the last five outcomes, from both, without counting a run twice,
//! - a `count` of what is running.
//!
//! A count is never a guess. When a source cannot be read its count is `null`
//! and `unreadable` says why; "zero because I could not look" is a different
//! fact from "zero".

use chrono::{DateTime, Utc};
use clap::Args;
use colored::Colorize;
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::Path;

use crate::commands::run_record::{self, RunRecord};
use crate::commands::task_ledger;
use hexa_core::ports::ledger::{Ledger, LedgerError};

/// A subagent that started and never stopped counts as in progress only for
/// this long; past it the start row is a stale one (a harness that crashed
/// does not write a stop), reported as `stale_agents` and not as running.
const AGENT_FRESH_HOURS: i64 = 12;

/// How many finished runs the report lists.
const RECENT: usize = 5;

#[derive(Args, Debug, Default)]
pub struct StatusArgs {
    /// Print JSON. The default when stdout is not a terminal.
    #[arg(long)]
    pub json: bool,
    /// Also ask the remote for the branch head (opt-in: it uses the network).
    #[arg(long)]
    pub remote: bool,
}

pub async fn run(args: StatusArgs) -> anyhow::Result<()> {
    let as_json = args.json || !std::io::stdout().is_terminal();
    emit(&args, as_json)
}

/// The human form, whatever stdout is. For bare `hexa`, which prints this and
/// then the getting-started guide.
pub async fn run_text() -> anyhow::Result<()> {
    emit(&StatusArgs::default(), false)
}

fn emit(args: &StatusArgs, as_json: bool) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let report = gather(&cwd, args.remote);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render_text(&report));
    }
    Ok(())
}

fn gather(cwd: &Path, remote: bool) -> Value {
    let mut report = build_report(&Inputs {
        runs_dir: run_record::runs_dir(),
        agent_log: hexa_exec::ports::RunLog::read_checked(&hexa_exec::run_log()),
        ledger: task_ledger::read(cwd),
        now: Utc::now(),
        alive: &run_record::process_alive,
    });
    let obj = report.as_object_mut().expect("report is an object");
    obj.insert("project".into(), json!(cwd.display().to_string()));
    obj.insert("hexa_dir".into(), json!(cwd.join(".hexa").is_dir()));
    obj.insert("git".into(), git_state(cwd, remote));
    obj.insert("loop".into(), crate::commands::loop_cmd::loop_summary(cwd));
    report
}

/// What the report is built from, passed in so a test can supply a temporary
/// state directory, a fixed clock and a liveness answer.
pub struct Inputs<'a> {
    pub runs_dir: std::path::PathBuf,
    pub agent_log: Result<hexa_exec::ports::AgentRunsRead, String>,
    /// The task ledger (`.hexa/tasks.json` of the main checkout).
    pub ledger: Result<Ledger, LedgerError>,
    pub now: DateTime<Utc>,
    pub alive: &'a dyn Fn(u64, Option<u64>) -> bool,
}

fn parse_ts(v: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v?.as_str()?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn age_secs(then: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Value {
    then.map(|t| json!((now - t).num_seconds().max(0)))
        .unwrap_or(Value::Null)
}

/// The runs, agents, recent outcomes, counts and problems. Pure in its
/// inputs: no clock, no home directory, no process table of its own.
pub fn build_report(i: &Inputs) -> Value {
    let mut unreadable: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();

    // ── run records ──────────────────────────────────────────────────────────
    let mut running: Vec<Value> = Vec::new();
    let mut finished: Vec<(String, Value)> = Vec::new(); // (sort key, entry)
    let mut abandoned = 0usize;
    let mut earliest_record: Option<String> = None;
    let runs_count: Value = match run_record::read_all(&i.runs_dir) {
        Err(e) => {
            unreadable.push(json!({ "source": i.runs_dir.display().to_string(), "reason": e }));
            Value::Null
        }
        Ok(read) => {
            if !read.unreadable.is_empty() {
                let files: Vec<String> = read
                    .unreadable
                    .iter()
                    .map(|(f, w)| format!("{f}: {w}"))
                    .collect();
                warnings.push(json!({
                    "source": i.runs_dir.display().to_string(),
                    "reason": format!("{} run record(s) could not be read, so the counts may be short", files.len()),
                    "files": files,
                }));
            }
            for rec in &read.records {
                earliest_record = match earliest_record {
                    Some(e) if e <= rec.started => Some(e),
                    _ => Some(rec.started.clone()),
                };
                // A record that belongs to a registered task is read through its
                // task (the `tasks` list): the process that registered it was a
                // short-lived CLI call, so its pid says nothing about the task.
                if rec.task.is_some() && rec.ended.is_none() && rec.outcome == "running" {
                    continue;
                }
                let state = run_record::effective_state(rec, i.alive);
                let started = parse_ts(Some(&json!(rec.started)));
                let mut entry = run_entry(rec, &state);
                entry["age_secs"] = age_secs(started, i.now);
                match state.as_str() {
                    "running" => running.push(entry),
                    other => {
                        if other == "abandoned" {
                            abandoned += 1;
                        }
                        let key = rec.ended.clone().unwrap_or_else(|| rec.started.clone());
                        finished.push((key, entry));
                    }
                }
            }
            json!(running.len())
        }
    };

    // ── the existing agent log ───────────────────────────────────────────────
    let mut agents: Vec<Value> = Vec::new();
    let mut stale = 0usize;
    let agents_count: Value;
    let stale_count: Value;
    match &i.agent_log {
        Err(e) => {
            unreadable.push(json!({ "source": "agent-runs.jsonl", "reason": e }));
            agents_count = Value::Null;
            stale_count = Value::Null;
        }
        Ok(log) => {
            if log.skipped_lines > 0 {
                warnings.push(json!({
                    "source": log.path.display().to_string(),
                    "reason": format!("{} line(s) were not JSON and were skipped", log.skipped_lines),
                }));
            }
            // The latest lifecycle event per agent id decides: start with no
            // later stop is open.
            let mut last: std::collections::BTreeMap<String, &Value> =
                std::collections::BTreeMap::new();
            for row in &log.rows {
                if row["kind"] == "subagent" {
                    if let Some(id) = row["agent_id"].as_str().filter(|s| !s.is_empty()) {
                        last.insert(id.to_string(), row);
                    }
                }
            }
            for (id, row) in last {
                if row["event"] != "start" {
                    continue;
                }
                let ts = parse_ts(row.get("ts"));
                let fresh = ts.is_some_and(|t| (i.now - t).num_hours() < AGENT_FRESH_HOURS);
                if !fresh {
                    stale += 1;
                    continue;
                }
                agents.push(json!({
                    "agent_id": id,
                    "agent_type": row["agent_type"],
                    "started": row["ts"],
                    "age_secs": age_secs(ts, i.now),
                    "cwd": row["cwd"],
                    "in_worktree": row["in_worktree"],
                }));
            }
            agents_count = json!(agents.len());
            stale_count = json!(stale);

            // `hexa do` rows from before there were run records. Once records
            // exist, a `do` run is in the record; its row in this log is the
            // same run and is not listed again.
            for row in &log.rows {
                if row.get("agent").and_then(|v| v.as_str()).is_none() {
                    continue;
                }
                let started = row["started_at"].as_str().unwrap_or("");
                if earliest_record.as_deref().is_some_and(|e| started >= e) {
                    continue;
                }
                finished.push((
                    started.to_string(),
                    json!({
                        "source": "agent-runs.jsonl",
                        "verb": "do",
                        "target": row["file"],
                        "started": row["started_at"],
                        "ended": Value::Null,
                        "outcome": if row["ok"] == true { "passed" } else { "failed" },
                        "commit": row["committed"],
                    }),
                ));
            }
        }
    }

    // ── the task ledger ──────────────────────────────────────────────────────
    // Tasks that hold their leases, then the last few that finished. A task
    // that was released is history and is not listed.
    let mut tasks: Vec<Value> = Vec::new();
    let tasks_count: Value = match &i.ledger {
        Err(e) => {
            unreadable.push(json!({ "source": "tasks.json", "reason": e.message, "code": e.code, "hint": e.hint }));
            Value::Null
        }
        Ok(l) => {
            let mut done: Vec<&hexa_core::ports::ledger::Task> =
                l.tasks.iter().filter(|t| !t.status.is_active() && t.status != hexa_core::ports::ledger::TaskStatus::Released).collect();
            done.sort_by(|a, b| b.updated.cmp(&a.updated));
            for t in l.active().chain(done.into_iter().take(RECENT)) {
                tasks.push(json!({
                    "id": t.id, "name": t.name, "status": t.status, "owner": t.owner,
                    "leases": t.leases, "worktree": t.worktree, "gate": t.gate, "run": t.run,
                    "outcome": t.outcome, "commit": t.commit,
                    "started": t.created, "updated": t.updated,
                    "age_secs": age_secs(parse_ts(Some(&json!(t.created))), i.now),
                }));
            }
            json!(l.active().count())
        }
    };

    finished.sort_by(|a, b| b.0.cmp(&a.0));
    let recent: Vec<Value> = finished.into_iter().take(RECENT).map(|(_, e)| e).collect();
    running.sort_by(|a, b| a["started"].as_str().cmp(&b["started"].as_str()));

    json!({
        "runs": running,
        "agents": agents,
        "tasks": tasks,
        "recent": recent,
        "count": {
            "runs": runs_count,
            "agents": agents_count,
            "tasks": tasks_count,
            "abandoned": if runs_count.is_null() { Value::Null } else { json!(abandoned) },
            "stale_agents": stale_count,
        },
        "unreadable": unreadable,
        "warnings": warnings,
        "ok": unreadable.is_empty(),
    })
}

fn run_entry(rec: &RunRecord, state: &str) -> Value {
    json!({
        "source": "run",
        "id": rec.id,
        "verb": rec.verb,
        "target": rec.target,
        "state": state,
        "pid": rec.pid,
        "pid_start_unknown": rec.pid_start_unknown,
        "started": rec.started,
        "ended": rec.ended,
        "outcome": state,
        "commit": rec.commit,
        "gate": rec.gate,
        "worktree": rec.worktree,
        "log": rec.log,
    })
}

// ── git ──────────────────────────────────────────────────────────────────────

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git did not run: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Branch, head, dirty count, ahead/behind of the upstream, and with
/// `remote` the remote's head. Each fact that cannot be had is `null` with a
/// `could_not_tell` entry saying why, never a default that reads as an answer.
fn git_state(cwd: &Path, remote: bool) -> Value {
    let head = match git(cwd, &["rev-parse", "--short", "HEAD"]) {
        Ok(h) => h,
        Err(e) => return json!({ "repo": false, "could_not_tell": e }),
    };
    let mut why: Vec<String> = Vec::new();
    let branch = git(cwd, &["branch", "--show-current"])
        .ok()
        .filter(|b| !b.is_empty());
    let dirty = match git(cwd, &["status", "--porcelain"]) {
        Ok(s) => json!(s.lines().count()),
        Err(e) => {
            why.push(format!("dirty: {e}"));
            Value::Null
        }
    };
    let (mut ahead, mut behind, mut upstream) = (Value::Null, Value::Null, Value::Null);
    match git(cwd, &["rev-parse", "--abbrev-ref", "@{u}"]) {
        Ok(u) => {
            upstream = json!(u);
            match git(cwd, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]) {
                Ok(c) => {
                    let mut n = c.split_whitespace().map(|x| x.parse::<u64>().ok());
                    behind = n.next().flatten().map_or(Value::Null, |v| json!(v));
                    ahead = n.next().flatten().map_or(Value::Null, |v| json!(v));
                }
                Err(e) => why.push(format!("ahead/behind: {e}")),
            }
        }
        Err(_) => why.push("ahead/behind: the branch has no upstream".into()),
    }
    let mut out = json!({
        "repo": true, "branch": branch, "head": head, "dirty": dirty,
        "upstream": upstream, "ahead": ahead, "behind": behind,
    });
    if remote {
        out["remote_head"] = match remote_head(cwd, branch.as_deref()) {
            Ok(h) => json!(h),
            Err(e) => {
                why.push(format!("remote: {e}"));
                Value::Null
            }
        };
    }
    if !why.is_empty() {
        out["could_not_tell"] = json!(why);
    }
    out
}

/// The remote branch head, with a bound on the wait: a network that blocks
/// must not hang a status.
fn remote_head(cwd: &Path, branch: Option<&str>) -> Result<String, String> {
    let branch = branch.ok_or("detached HEAD: no branch to ask about")?;
    let mut child = std::process::Command::new("git")
        .args(["ls-remote", "origin", &format!("refs/heads/{branch}")])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git did not run: {e}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("origin did not answer within 10s".into());
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&o.stdout);
    match text.split_whitespace().next() {
        Some(sha) => Ok(sha.chars().take(7).collect()),
        None => Err(format!("origin has no branch {branch}")),
    }
}

// ── text ─────────────────────────────────────────────────────────────────────

fn ago(secs: Option<u64>) -> String {
    match secs {
        None => "?".into(),
        Some(s) if s < 90 => format!("{s}s"),
        Some(s) if s < 5400 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn render_text(r: &Value) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let _ = writeln!(o, "{} hexa project status\n", "\u{2b21}".cyan());
    let _ = writeln!(o, "  Project: {}", show(&r["project"]));
    let _ = writeln!(
        o,
        "  Config:  {}",
        if r["hexa_dir"] == true {
            ".hexa/ found".green()
        } else {
            "no .hexa/ directory".yellow()
        }
    );
    let g = &r["git"];
    if g["repo"] == true {
        let mut line = format!("{} on {}", show(&g["head"]), show(&g["branch"]));
        if !g["dirty"].is_null() {
            let _ = write!(line, " · {} dirty", g["dirty"]);
        }
        if !g["ahead"].is_null() {
            let _ = write!(
                line,
                " · {} ahead, {} behind {}",
                g["ahead"],
                g["behind"],
                show(&g["upstream"])
            );
        }
        if g.get("remote_head").is_some() {
            let _ = write!(line, " · remote head {}", show(&g["remote_head"]));
        }
        let _ = writeln!(o, "  Git:     {line}");
        for w in g["could_not_tell"].as_array().into_iter().flatten() {
            let _ = writeln!(
                o,
                "           {}",
                format!("could not tell: {}", show(w)).dimmed()
            );
        }
    } else {
        let _ = writeln!(o, "  Git:     {}", "not a git repository".dimmed());
    }
    let l = &r["loop"];
    if l["readable"] == false {
        let _ = writeln!(
            o,
            "  Loop:    {} {}",
            "unreadable:".red(),
            show(&l["error"])
        );
    } else if l["recorded"] == true {
        let _ = writeln!(
            o,
            "  Loop:    stage {} · ADR {} · gate {} · checklist {}/{}",
            show(&l["stage"]),
            show(&l["adr"]),
            show(&l["gate"]),
            l["checklist_done"],
            l["checklist"].as_array().map_or(0, Vec::len)
        );
        if let Some(run) = l["running"].as_str() {
            let _ = writeln!(o, "           {run}");
        }
    } else {
        let _ = writeln!(
            o,
            "  Loop:    {}",
            "nothing recorded (hexa loop adr <ID>)".dimmed()
        );
    }

    let c = &r["count"];
    let n = |v: &Value| {
        if v.is_null() {
            "unreadable".red().to_string()
        } else {
            v.to_string()
        }
    };
    let _ = writeln!(
        o,
        "\n  Running: {} run(s) · {} agent(s) · {} task(s) · {} abandoned",
        n(&c["runs"]),
        n(&c["agents"]),
        n(&c["tasks"]),
        n(&c["abandoned"])
    );
    for t in r["tasks"].as_array().into_iter().flatten() {
        let _ = writeln!(
            o,
            "    task   {:<8} {}  {}  leases {}  {}",
            show(&t["status"]),
            show(&t["id"]),
            show(&t["name"]),
            t["leases"].as_array().map(|a| a.iter().map(show).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            ago(t["age_secs"].as_u64()).dimmed()
        );
    }
    for run in r["runs"].as_array().into_iter().flatten() {
        let _ = writeln!(
            o,
            "    run    {:<8} {}  {}  pid {}  {}",
            show(&run["verb"]),
            show(&run["target"]),
            ago(run["age_secs"].as_u64()).dimmed(),
            show(&run["pid"]),
            show(&run["log"]).dimmed()
        );
    }
    for a in r["agents"].as_array().into_iter().flatten() {
        let _ = writeln!(
            o,
            "    agent  {:<8} {}  {}",
            show(&a["agent_type"]),
            show(&a["agent_id"]),
            ago(a["age_secs"].as_u64()).dimmed()
        );
    }
    if c["stale_agents"].as_u64().unwrap_or(0) > 0 {
        let _ = writeln!(
            o,
            "    {}",
            format!(
                "{} older subagent start(s) have no stop and are not counted as running",
                c["stale_agents"]
            )
            .dimmed()
        );
    }
    let recent = r["recent"].as_array().cloned().unwrap_or_default();
    if recent.is_empty() {
        let _ = writeln!(o, "\n  Last outcomes: {}", "none recorded".dimmed());
    } else {
        let _ = writeln!(o, "\n  Last outcomes:");
        for e in &recent {
            let outcome = show(&e["outcome"]);
            let colored = match outcome.as_str() {
                "passed" => outcome.green(),
                "failed" | "abandoned" | "killed" => outcome.red(),
                _ => outcome.normal(),
            };
            let _ = writeln!(
                o,
                "    {:<9} {:<8} {}  {}",
                colored,
                show(&e["verb"]),
                show(&e["target"]),
                show(&e["commit"]).yellow()
            );
        }
    }
    for p in r["unreadable"].as_array().into_iter().flatten() {
        let _ = writeln!(
            o,
            "\n  {} {}: {}",
            "UNREADABLE".red().bold(),
            show(&p["source"]),
            show(&p["reason"])
        );
    }
    for w in r["warnings"].as_array().into_iter().flatten() {
        let _ = writeln!(
            o,
            "  {} {}: {}",
            "warning".yellow(),
            show(&w["source"]),
            show(&w["reason"])
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexa_exec::ports::AgentRunsRead;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-06T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn log(rows: Vec<Value>) -> Result<AgentRunsRead, String> {
        Ok(AgentRunsRead {
            path: "agent-runs.jsonl".into(),
            present: true,
            rows,
            skipped_lines: 0,
        })
    }

    fn sub(id: &str, event: &str, ts: &str) -> Value {
        json!({"kind":"subagent","event":event,"agent_id":id,"agent_type":"general-purpose","ts":ts})
    }

    fn record(dir: &Path, id: &str, pid: u64, started: &str, ended: Option<&str>, outcome: &str) {
        let rec = RunRecord {
            id: id.into(),
            verb: "harden".into(),
            target: "src/x.rs".into(),
            pid,
            pid_start: Some(7),
            pid_start_unknown: false,
            started: started.into(),
            ended: ended.map(str::to_string),
            outcome: outcome.into(),
            commit: None,
            worktree: None,
            gate: Some("g".into()),
            log: None,
            task: None,
            gate_result: None,
        };
        run_record::write(dir, &rec).unwrap();
    }

    fn inputs<'a>(
        dir: &Path,
        agent_log: Result<AgentRunsRead, String>,
        alive: &'a dyn Fn(u64, Option<u64>) -> bool,
    ) -> Inputs<'a> {
        Inputs {
            runs_dir: dir.to_path_buf(),
            agent_log,
            ledger: Ok(Ledger::default()),
            now: now(),
            alive,
        }
    }

    #[test]
    fn status_lists_runs_and_counts_the_ones_in_progress() {
        let d = tempfile::tempdir().unwrap();
        record(
            d.path(),
            "live",
            100,
            "2026-11-06T11:00:00Z",
            None,
            "running",
        );
        record(
            d.path(),
            "gone",
            200,
            "2026-11-06T10:00:00Z",
            None,
            "running",
        );
        record(
            d.path(),
            "done",
            300,
            "2026-11-06T09:00:00Z",
            Some("2026-11-06T09:30:00Z"),
            "passed",
        );
        let alive = |pid: u64, _s: Option<u64>| pid == 100;
        let r = build_report(&inputs(d.path(), log(vec![]), &alive));
        assert_eq!(r["count"]["runs"], 1, "{r}");
        assert_eq!(r["runs"][0]["id"], "live");
        assert_eq!(r["runs"][0]["age_secs"], 3600);
        assert_eq!(
            r["count"]["abandoned"], 1,
            "the dead pid with no ended is abandoned, not running"
        );
        let outcomes: Vec<(&str, &str)> = r["recent"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["id"].as_str().unwrap(), e["outcome"].as_str().unwrap()))
            .collect();
        assert_eq!(
            outcomes,
            vec![("gone", "abandoned"), ("done", "passed")],
            "newest first by ended, else started"
        );
        assert_eq!(r["ok"], true);
    }

    #[test]
    fn status_lists_runs_from_the_existing_agent_log_without_a_second_log() {
        let d = tempfile::tempdir().unwrap();
        let rows = vec![
            sub("a1", "start", "2026-11-06T11:30:00+00:00"),
            sub("a2", "start", "2026-11-06T11:40:00+00:00"),
            sub("a2", "stop", "2026-11-06T11:50:00+00:00"),
            sub("old", "start", "2026-11-01T00:00:00+00:00"),
            json!({"agent":"direct-react","ok":true,"committed":"abc1234","file":"x.rs","started_at":"2026-11-05T00:00:00+00:00"}),
        ];
        let alive = |_: u64, _: Option<u64>| false;
        let r = build_report(&inputs(d.path(), log(rows), &alive));
        assert_eq!(
            r["count"]["agents"], 1,
            "a1 started and never stopped; a2 stopped; old is stale: {r}"
        );
        assert_eq!(r["agents"][0]["agent_id"], "a1");
        assert_eq!(r["count"]["stale_agents"], 1);
        assert_eq!(
            r["recent"][0]["commit"], "abc1234",
            "a legacy `do` row is listed when no record covers it"
        );
        assert_eq!(r["recent"][0]["outcome"], "passed");
    }

    #[test]
    fn status_lists_runs_without_listing_a_do_run_twice() {
        let d = tempfile::tempdir().unwrap();
        record(
            d.path(),
            "do-1",
            300,
            "2026-11-06T09:00:00Z",
            Some("2026-11-06T09:30:00Z"),
            "passed",
        );
        // The row the same run appended to agent-runs.jsonl: after the record started.
        let row = json!({"agent":"direct-react","ok":true,"committed":"abc1234","file":"x.rs","started_at":"2026-11-06T09:00:05+00:00"});
        let alive = |_: u64, _: Option<u64>| false;
        let r = build_report(&inputs(d.path(), log(vec![row]), &alive));
        assert_eq!(r["recent"].as_array().unwrap().len(), 1, "{r}");
    }

    /// ADR-2611051850: a count of zero because the record could not be read
    /// is not a count of zero.
    #[test]
    fn status_lists_runs_reports_an_unreadable_source_as_unreadable_not_zero() {
        let d = tempfile::tempdir().unwrap();
        let not_a_dir = d.path().join("runs");
        std::fs::write(&not_a_dir, "x").unwrap();
        let alive = |_: u64, _: Option<u64>| false;
        let r = build_report(&inputs(
            &not_a_dir,
            Err("cannot read agent-runs.jsonl: permission denied".into()),
            &alive,
        ));
        assert!(r["count"]["runs"].is_null(), "{r}");
        assert!(r["count"]["agents"].is_null(), "{r}");
        assert_eq!(r["unreadable"].as_array().unwrap().len(), 2);
        assert_eq!(r["ok"], false);
        assert!(render_text(&r).contains("UNREADABLE"));
    }

    #[test]
    fn status_lists_runs_warns_of_a_corrupt_record_but_still_counts_the_rest() {
        let d = tempfile::tempdir().unwrap();
        record(
            d.path(),
            "live",
            100,
            "2026-11-06T11:00:00Z",
            None,
            "running",
        );
        std::fs::write(d.path().join("bad.json"), "{").unwrap();
        let alive = |pid: u64, _: Option<u64>| pid == 100;
        let r = build_report(&inputs(d.path(), log(vec![]), &alive));
        assert_eq!(r["count"]["runs"], 1);
        assert_eq!(
            r["warnings"].as_array().unwrap().len(),
            1,
            "the corrupt file is named, not skipped silently: {r}"
        );
    }

    #[test]
    fn status_lists_runs_empty_state_is_a_true_zero() {
        let d = tempfile::tempdir().unwrap();
        let absent = AgentRunsRead {
            path: "x".into(),
            present: false,
            rows: vec![],
            skipped_lines: 0,
        };
        let alive = |_: u64, _: Option<u64>| false;
        let r = build_report(&inputs(&d.path().join("never-made"), Ok(absent), &alive));
        assert_eq!(
            (
                r["count"]["runs"].clone(),
                r["count"]["agents"].clone(),
                r["count"]["tasks"].clone()
            ),
            (json!(0), json!(0), json!(0))
        );
        assert_eq!(r["ok"], true);
    }

    fn task(id: &str, status: hexa_core::ports::ledger::TaskStatus, updated: &str) -> hexa_core::ports::ledger::Task {
        hexa_core::ports::ledger::Task {
            id: id.into(),
            name: format!("name-{id}"),
            owner: None,
            worktree: None,
            gate: None,
            leases: vec![format!("src/{id}")],
            status,
            run: None,
            outcome: None,
            commit: None,
            note: None,
            created: "2026-11-06T10:00:00Z".into(),
            updated: updated.into(),
        }
    }

    /// ADR-2611051850 phase 2: the ledger's active tasks are counted and listed,
    /// finished ones are listed but not counted, released ones are not listed.
    #[test]
    fn status_lists_tasks_counts_the_active_ones_and_hides_the_released() {
        use hexa_core::ports::ledger::TaskStatus::*;
        let d = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        ledger.tasks.push(task("a", Claimed, "2026-11-06T10:00:00Z"));
        ledger.tasks.push(task("b", Running, "2026-11-06T10:05:00Z"));
        ledger.tasks.push(task("c", Done, "2026-11-06T10:10:00Z"));
        ledger.tasks.push(task("d", Released, "2026-11-06T10:11:00Z"));
        let alive = |_: u64, _: Option<u64>| false;
        let mut i = inputs(d.path(), log(vec![]), &alive);
        i.ledger = Ok(ledger);
        let r = build_report(&i);
        assert_eq!(r["count"]["tasks"], 2, "{r}");
        let ids: Vec<&str> = r["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "{r}");
        assert!(render_text(&r).contains("task"));
    }

    #[test]
    fn status_lists_tasks_reports_an_unreadable_ledger_as_unreadable_not_zero() {
        let d = tempfile::tempdir().unwrap();
        let alive = |_: u64, _: Option<u64>| false;
        let mut i = inputs(d.path(), log(vec![]), &alive);
        i.ledger = Err(LedgerError::new("ledger_unreadable", "bad json".into(), "fix it".into(), None, true));
        let r = build_report(&i);
        assert!(r["count"]["tasks"].is_null(), "{r}");
        assert_eq!(r["ok"], false);
        assert_eq!(r["unreadable"][0]["code"], "ledger_unreadable");
    }

    /// The record of a registered task is read through its task: a dead
    /// registering pid must not make it abandoned or running.
    #[test]
    fn status_lists_tasks_without_calling_a_task_bound_record_abandoned() {
        let d = tempfile::tempdir().unwrap();
        let mut rec = RunRecord {
            id: "agent-1".into(),
            verb: "agent".into(),
            target: "one".into(),
            pid: 999,
            pid_start: Some(1),
            pid_start_unknown: false,
            started: "2026-11-06T10:00:00Z".into(),
            ended: None,
            outcome: "running".into(),
            commit: None,
            worktree: None,
            gate: None,
            log: None,
            task: Some("a".into()),
            gate_result: None,
        };
        run_record::write(d.path(), &rec).unwrap();
        let alive = |_: u64, _: Option<u64>| false;
        let r = build_report(&inputs(d.path(), log(vec![]), &alive));
        assert_eq!((r["count"]["runs"].clone(), r["count"]["abandoned"].clone()), (json!(0), json!(0)), "{r}");
        rec.ended = Some("2026-11-06T11:00:00Z".into());
        rec.outcome = "passed".into();
        run_record::write(d.path(), &rec).unwrap();
        let r = build_report(&inputs(d.path(), log(vec![]), &alive));
        assert_eq!(r["recent"][0]["outcome"], "passed", "a closed task record is a last outcome: {r}");
    }
}
