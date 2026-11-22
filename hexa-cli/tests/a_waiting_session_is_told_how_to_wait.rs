//! Findings from the pre-landing review of ADR-2611220204, each a test:
//! the landing hint works with no checkout of the base, a step's title is
//! its key so a duplicate is refused, a blocked session is told the verb that
//! records waiting, and the next session start shows the last session's hold.

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

struct Repo {
    _t: tempfile::TempDir,
    dir: std::path::PathBuf,
    home: std::path::PathBuf,
    session: String,
}

fn repo() -> Repo {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("proj");
    let home = t.path().join("home");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "r\n").unwrap();
    std::fs::write(dir.join(".gitignore"), ".hexa/\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    Repo { _t: t, dir, home, session: uuid::Uuid::new_v4().to_string() }
}

fn hexa(r: &Repo, session: &str, pid: Option<&str>, args: &[&str], stdin: &str) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hexa"));
    c.current_dir(&r.dir)
        .args(args)
        .env("HOME", &r.home)
        .env("HEXA_SESSION_ID", session)
        .env("CLAUDE_SESSION_ID", session)
        .env("CLAUDE_PROJECT_DIR", &r.dir)
        .env("NO_COLOR", "1")
        .env_remove("HEXA_TASK")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match pid {
        Some(p) => {
            c.env("HEXA_SESSION_PID", p);
        }
        None => {
            c.env_remove("HEXA_SESSION_PID");
        }
    }
    let mut child = c.spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// With the base checked out nowhere, a `merge --ff-only` run in some other
/// checkout does nothing and the branch stays unlanded: the hint must name a
/// fast-forward that needs no checkout of the base.
#[test]
fn the_landing_hint_works_with_no_checkout_of_the_base() {
    let r = repo();
    git(&r.dir, &["checkout", "-q", "-b", "feat/x"]);
    std::fs::write(r.dir.join("x.txt"), "x\n").unwrap();
    git(&r.dir, &["add", "x.txt"]);
    git(&r.dir, &["commit", "-q", "-m", "x"]);
    let hint = hexa_cli::commands::finish::fast_forward(&r.dir, "feat/x", "main");
    assert!(hint.contains("fetch . feat/x:main"), "no checkout of main: {hint}");

    // Following the hint lands it, and keeps the branch's own commit.
    let tip = git(&r.dir, &["rev-parse", "feat/x"]);
    git(&r.dir, &["fetch", "-q", ".", "feat/x:main"]);
    assert_eq!(git(&r.dir, &["rev-parse", "main"]), tip, "main is fast-forwarded to the branch's tip");

    // With main checked out somewhere, the fast-forward is run there.
    let wt = r._t.path().join("main-wt");
    git(&r.dir, &["worktree", "add", "-q", wt.to_str().unwrap(), "main"]);
    let hint = hexa_cli::commands::finish::fast_forward(&r.dir, "feat/x", "main");
    assert!(hint.contains("merge --ff-only feat/x") && hint.contains("main-wt"), "from main's checkout: {hint}");
}

/// A step's title is its key: two steps titled the same would be one piece
/// of work to a hold, so a second is refused and nothing is added.
#[test]
fn a_second_step_with_the_same_title_is_refused() {
    let r = repo();
    assert!(hexa(&r, &r.session, None, &["loop", "task", "add", "Write the parser"], "").status.success());
    let before = std::fs::read_to_string(r.dir.join(".hexa/loop.json")).unwrap();
    let o = hexa(&r, &r.session, None, &["loop", "task", "add", "Write the parser"], "");
    assert!(!o.status.success(), "a duplicate title is refused: {}", text(&o));
    assert!(text(&o).contains("Write the parser"), "the refusal names the title: {}", text(&o));
    assert_eq!(std::fs::read_to_string(r.dir.join(".hexa/loop.json")).unwrap(), before, "nothing written");
    assert!(hexa(&r, &r.session, None, &["loop", "task", "add", "Write the lexer"], "").status.success(), "another title is fine");
}

/// A session sent back for open work is told the verb that records why it
/// waits; without it, the hold the ADR made cannot be found from the block.
#[test]
fn a_blocked_stop_names_the_hold_verb() {
    let r = repo();
    assert!(hexa(&r, &r.session, None, &["loop", "task", "add", "Write the parser"], "").status.success());
    let payload = serde_json::json!({ "session_id": r.session, "hook_event_name": "Stop", "stop_hook_active": false }).to_string();
    let o = hexa(&r, &r.session, None, &["hook", "stop"], &payload);
    let out = text(&o);
    assert!(out.contains("\"decision\":\"block\""), "an open task blocks: {out}");
    assert!(out.contains("hexa loop hold"), "the block names the verb: {out}");
}

/// The next session has a new id, so its own hold lines are empty: the
/// previous session's hold is what the person reads when it starts.
#[test]
fn the_next_session_start_shows_the_previous_hold() {
    let r = repo();
    let a = r.session.clone();
    // Session A records work, holds it, and ends (pid 0).
    for args in [&["loop", "task", "add", "Wait for review"][..], &["loop", "hold", "waiting on the owner's review"][..]] {
        let o = hexa(&r, &a, Some("0"), args, "");
        assert!(o.status.success(), "{args:?}: {}", text(&o));
    }
    let lines = hexa_cli::commands::loop_cmd::previous_lines(&r.dir);
    assert!(
        lines.iter().any(|l| l.contains("waiting on the owner's review")),
        "the previous session's hold is shown: {lines:?}"
    );
}
