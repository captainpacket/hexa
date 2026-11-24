//! The lifecycle runs its gates where the project says tests run: a hexa verb
//! on the remote runner runs its tests there (§3), and `loop stage done` asks
//! the remote whether HEAD is pushed and sends its evidence with the gate
//! (§4). harden (§1) is `hexa-exec/tests/harden_remote_gate.rs`; the evidence
//! reader (§2) is `hexa-core/src/domain/evidence.rs`'s tests.
//!
//! Written from the scenarios of `docs/specs/<this ADR>.json`, before the
//! code. Every case runs the built binary in a temp repository whose origin is
//! a bare repository beside it; the remote runner is a local script that
//! clones the commit it is sent from that origin, as a build server would. No
//! network.
//!
//! Fixture ids are assembled at run time: the citation checker scans source,
//! and a literal id here would claim a decision exists.

use std::path::{Path, PathBuf};
use std::process::Command;

const SESSION: &str = "remote-lifecycle-test";

fn hexa_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hexa"))
}

struct Run {
    code: i32,
    out: String,
}

fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
    let home = dir.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(hexa_bin());
    cmd.args(args)
        .current_dir(dir)
        .env("HOME", &home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("CLAUDE_CODE_SESSION_ID", SESSION)
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env_remove("HEXA_SESSION_ID")
        .env_remove("HEXA_ON_RUNNER")
        .env("NO_COLOR", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o = cmd.output().expect("run hexa");
    Run {
        code: o.status.code().unwrap_or(-1),
        out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
    }
}

fn run(dir: &Path, args: &[&str]) -> Run {
    run_env(dir, args, &[])
}

fn ok(dir: &Path, args: &[&str]) {
    let r = run(dir, args);
    assert_eq!(r.code, 0, "hexa {args:?}:\n{}", r.out);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

fn executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn the_id() -> String {
    format!("ADR-{}", "2701240000")
}

fn adr_rel() -> String {
    format!("docs/adrs/{}-a-decision.md", the_id())
}

const PASSES: &str = "#!/bin/bash\ntouch ran-here-$(basename \"$0\" .sh)\nexit 0\n";
/// Passes only when the runner marker names the commit checked out.
const MARKER: &str = "#!/bin/bash\ntouch ran-here-marker\necho \"marker=$HEXA_ON_RUNNER\"\n[ \"$HEXA_ON_RUNNER\" = \"$(git rev-parse HEAD)\" ]\n";
const EVIDENCE: &str = "#!/bin/bash\ntouch ran-here-evidence\necho EVIDENCE-OK\n";
const TOUCH: &str = "#!/bin/bash\ntouch ran-here-touch\nexit 0\n";

/// A repository on `main` with an ADR of one decision and its accepted spec,
/// S1 and S2 bound to two passing bash tests, the `ci/` scripts, committed.
fn project() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let id = the_id();
    write(&root, "package.json", "{}\n");
    write(&root, "tests/a.sh", PASSES);
    write(&root, "tests/b.sh", PASSES);
    write(&root, "ci/marker.sh", MARKER);
    write(&root, "ci/evidence.sh", EVIDENCE);
    write(&root, "ci/touch.sh", TOUCH);
    write(&root, ".gitignore", ".test-home/\ntarget/\nran-here-*\n.hexa/loop.json\n.hexa/loop.lock\n.hexa/graph/\n.hexa/sessions/\n");
    write(
        &root,
        &adr_rel(),
        &format!(
            "# {id}: A decision\n\n**Status:** Accepted\n**Date:** 2026-11-01\n\n## Context\n\nx\n\n\
             ## Decision\n\n1. **It holds.** Always.\n\n## Consequences\n\nx\n"
        ),
    );
    let sc = |sid: &str, kind: &str, t: &str| {
        serde_json::json!({ "id": sid, "decision": ["§1"], "kind": kind, "description": format!("{sid} holds"),
                            "given": "a thing", "when": "it runs", "then": "it holds", "test": t })
    };
    let spec = serde_json::json!({ "adr": id, "feature": "a-feature",
                                   "specs": [sc("S1", "example", "tests/a.sh"), sc("S2", "negative", "tests/b.sh")] });
    write(&root, &format!("docs/specs/{id}.json"), &(serde_json::to_string_pretty(&spec).unwrap() + "\n"));
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    ok(&root, &["spec", "accept", &id, "--by", "a reviewer"]);
    ok(&root, &["spec", "seal", &id]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "accept"]);
    (t, root)
}

/// A bare origin and a runner script beside it. An honest runner logs the
/// commit it is sent, clones it from origin, prints the id it checked out and
/// runs the command there. A silent one logs and exits 0, running nothing.
struct Remote {
    _dir: tempfile::TempDir,
    origin: PathBuf,
    log: PathBuf,
    template: String,
}

impl Remote {
    fn invocations(&self) -> usize {
        std::fs::read_to_string(&self.log).map(|s| s.lines().count()).unwrap_or(0)
    }
}

fn remote_for(root: &Path, silent: bool) -> Remote {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    git(dir.path(), &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    let log = dir.path().join("runner.log");
    let runner = dir.path().join("runner.sh");
    let body = if silent {
        format!("#!/bin/bash\necho \"$1\" >> '{}'\nexit 0\n", log.display())
    } else {
        format!(
            "#!/bin/bash\necho \"$1\" >> '{log}'\nd=$(mktemp -d)\n\
             git clone -q '{origin}' \"$d/c\" && cd \"$d/c\" && git checkout -q \"$1\" || exit 2\n\
             echo \"runner: testing $(git rev-parse HEAD)\"\nbash -c \"$2\"; rc=$?\ncd /; rm -rf \"$d\"; exit $rc\n",
            log = log.display(),
            origin = origin.display()
        )
    };
    std::fs::write(&runner, body).unwrap();
    executable(&runner);
    Remote { template: format!("bash {} {{commit}} {{cmd}}", runner.display()), origin, log, _dir: dir }
}

/// Set `test.remote`, commit it, push `main` with its upstream.
fn use_remote(root: &Path, r: &Remote) {
    let pj = serde_json::json!({ "name": "p", "id": "00000000-test", "lifecycle_enforcement": "advisory",
                                 "test": { "remote": r.template } });
    write(root, ".hexa/project.json", &serde_json::to_string_pretty(&pj).unwrap());
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "remote"]);
    git(root, &["push", "-q", "-u", "origin", "main"]);
}

fn remote_project(silent: bool) -> (tempfile::TempDir, PathBuf, Remote) {
    let (t, root) = project();
    let remote = remote_for(&root, silent);
    use_remote(&root, &remote);
    (t, root, remote)
}

fn head(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"])
}

// ── §3 a hexa verb on the runner runs its tests there ────────────────────────

/// S09: A loop gate of hexa spec verify under test.remote costs one remote run and passes there
#[test]
fn s09_a_nested_spec_verify_costs_one_remote_run() {
    let (_t, root, remote) = remote_project(false);
    let gate = format!("'{}' spec verify {}", hexa_bin().display(), the_id());
    ok(&root, &["loop", "gate", &gate]);
    let r = run(&root, &["loop", "stage", "done"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(remote.invocations(), 1, "the outer run only: the spec verify on the runner ran its tests there\n{}", r.out);
    assert!(!root.join("ran-here-a").exists(), "nothing ran here");
}

/// S10: HEXA_ON_RUNNER equal to HEAD runs the tests in this checkout
#[test]
fn s10_the_marker_for_head_runs_the_tests_here() {
    let (_t, root, remote) = remote_project(true);
    let r = run_env(&root, &["spec", "verify", &the_id()], &[("HEXA_ON_RUNNER", &head(&root))]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("2 passed"), "{}", r.out);
    assert_eq!(remote.invocations(), 0, "the template was not called");
    assert!(root.join("ran-here-a").exists() && root.join("ran-here-b").exists(), "the tests ran in this checkout");
}

/// S11: HEXA_ON_RUNNER naming another commit is refused
#[test]
fn s11_a_marker_for_another_commit_is_refused() {
    let (_t, root, remote) = remote_project(true);
    let other = git(&root, &["rev-parse", "HEAD~1"]);
    let r = run_env(&root, &["spec", "verify", &the_id()], &[("HEXA_ON_RUNNER", &other)]);
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("HEXA_ON_RUNNER") && r.out.contains(&other) && r.out.contains(&head(&root)), "{}", r.out);
    assert_eq!(remote.invocations(), 0);
    assert!(!root.join("ran-here-a").exists(), "nothing ran");
}

/// S12: The batch script exports HEXA_ON_RUNNER for each command
#[test]
fn s12_the_batch_exports_the_marker_for_the_commit() {
    let (_t, root, remote) = remote_project(false);
    ok(&root, &["loop", "gate", "bash ci/marker.sh"]);
    let r = run(&root, &["loop", "stage", "done"]);
    assert_eq!(r.code, 0, "the gate saw HEXA_ON_RUNNER equal to the commit checked out:\n{}", r.out);
    assert_eq!(remote.invocations(), 1);
    assert!(!root.join("ran-here-marker").exists());
}

// ── §4 stage done asks the remote, and sends its evidence with the gate ──────

/// S13: stage done refuses with head-not-pushed when the remote does not hold HEAD, though the local tracking ref does
#[test]
fn s13_a_head_the_remote_lost_is_not_pushed() {
    let (_t, root, remote) = remote_project(false);
    ok(&root, &["loop", "gate", "bash ci/touch.sh"]);
    git(&remote.origin, &["update-ref", "-d", "refs/heads/main"]);
    assert!(!git(&root, &["branch", "-r", "--contains", "HEAD"]).is_empty(), "the local tracking ref still names HEAD");
    let r = run(&root, &["loop", "stage", "done"]);
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("head-not-pushed") && r.out.contains("git push"), "{}", r.out);
    assert_eq!(remote.invocations(), 0, "nothing was sent");
}

/// S14: stage done refuses an unpushed HEAD with the named code
#[test]
fn s14_an_unpushed_head_is_refused_by_code() {
    let (_t, root, remote) = remote_project(false);
    ok(&root, &["loop", "gate", "bash ci/touch.sh"]);
    write(&root, "notes.txt", "later\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "not pushed"]);
    let r = run(&root, &["loop", "stage", "done"]);
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("head-not-pushed"), "{}", r.out);
    assert_eq!(remote.invocations(), 0);
    assert!(!root.join("ran-here-touch").exists());
}

/// S15: stage done sends the evidence command with the gate, in one run
#[test]
fn s15_the_evidence_goes_with_the_gate() {
    let (_t, root, remote) = remote_project(false);
    ok(&root, &["loop", "adr", &the_id()]);
    ok(&root, &["loop", "gate", "bash ci/touch.sh"]);
    ok(&root, &["loop", "evidence", "bash ci/evidence.sh"]);
    let r = run(&root, &["loop", "stage", "done"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(remote.invocations(), 1, "the gate and the evidence in one run:\n{}", r.out);
    assert!(read(&root, &adr_rel()).contains("EVIDENCE-OK"), "the evidence reached the ADR:\n{}", read(&root, &adr_rel()));
    assert!(!root.join("ran-here-evidence").exists(), "the evidence did not run here");
    assert!(!root.join("ran-here-touch").exists(), "the gate did not run here");
}

/// S16: loop task done runs no gate, here or remote
#[test]
fn s16_task_done_runs_no_gate() {
    let (_t, root, remote) = remote_project(false);
    ok(&root, &["loop", "gate", "bash ci/touch.sh"]);
    ok(&root, &["loop", "task", "add", "a step"]);
    let r = run(&root, &["loop", "task", "done", "1"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("1 of"), "the step is checked off:\n{}", r.out);
    assert!(!root.join("ran-here-touch").exists(), "no gate ran here");
    assert_eq!(remote.invocations(), 0, "and none on the runner");
}
