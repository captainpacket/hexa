//! The usage eval's second finding (ADR-2611160910, baseline
//! 20261116T144539Z, where-am-i): setup recorded a gate and two tasks, one
//! done, and a new session saw only `ended: session local · stage gate`,
//! without the tasks, so the model reported "no recorded work". Here a session
//! with no loop entry of its own sees the most recent ended session's ADR,
//! gate, stage and checklist, labelled as the previous session's, in `hexa
//! status`, `hexa loop` and the session-start banner; the next step comes from
//! it; and `hexa loop resume` carries it into this session so the hooks accept
//! edits under it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NEW: &str = "the-new-session";

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(repo.join(".hexa")).unwrap();
    std::fs::write(repo.join("src/x.rs"), "fn a() {}\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "target/\n.hexa/loop.json\n.hexa/loop.lock\n").unwrap();
    std::fs::write(repo.join(".hexa/project.json"), r#"{"name":"p","id":"00000000-test"}"#).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "init"]);
    (tmp, home, repo)
}

/// Run hexa as `session`. The previous session records under hexa's own
/// variables with pid 0, which is never live, so it reads as ended on every
/// platform.
fn hexa_as(session: &str, home: &Path, repo: &Path, args: &[&str], stdin: &str) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hexa"));
    cmd.args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", repo)
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("HEXA_INTERNAL")
        .env_remove("HEXA_TASK")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if session == NEW {
        cmd.env("CLAUDE_CODE_SESSION_ID", NEW).env_remove("HEXA_SESSION_ID").env_remove("HEXA_SESSION_PID");
    } else {
        cmd.env("HEXA_SESSION_ID", session).env("HEXA_SESSION_PID", "0").env_remove("CLAUDE_CODE_SESSION_ID");
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// What where-am-i's setup leaves: a gate and two tasks, the first done.
fn previous_work(home: &Path, repo: &Path) {
    for args in [
        &["loop", "gate", "cargo test"][..],
        &["loop", "task", "add", "Fix the page count for an even split"],
        &["loop", "task", "add", "Show the page number in the CLI"],
        &["loop", "task", "done", "1"],
    ] {
        ok(hexa_as("prev", home, repo, args, ""));
    }
}

fn pre_bash(home: &Path, repo: &Path, command: &str) -> Output {
    let payload = serde_json::json!({
        "session_id": NEW, "hook_event_name": "PreToolUse", "tool_name": "Bash",
        "cwd": repo.to_str().unwrap(), "tool_input": { "command": command },
    });
    hexa_as(NEW, home, repo, &["hook", "pre-bash"], &payload.to_string())
}

#[test]
fn hexa_loop_in_a_fresh_session_shows_the_previous_sessions_checklist() {
    let (_t, home, repo) = project();
    previous_work(&home, &repo);
    let out = ok(hexa_as(NEW, &home, &repo, &["loop"], ""));
    assert!(out.contains("previous session"), "labelled as the previous session's: {out}");
    assert!(out.contains("gate cargo test"), "{out}");
    assert!(out.contains("[x] 1 Fix the page count for an even split"), "{out}");
    assert!(out.contains("2 Show the page number in the CLI"), "{out}");
    assert!(out.contains("hexa loop resume"), "says how to carry it over: {out}");
}

#[test]
fn hexa_status_in_a_fresh_session_shows_the_previous_checklist_and_takes_its_next_step_from_it() {
    let (_t, home, repo) = project();
    previous_work(&home, &repo);
    let v: serde_json::Value = serde_json::from_str(&ok(hexa_as(NEW, &home, &repo, &["status", "--json"], ""))).unwrap();
    let lp = &v["loop"];
    assert_eq!(lp["recorded"], false, "this session recorded nothing: {lp}");
    let prev = &lp["previous"];
    assert_eq!(prev["gate"], "cargo test", "{lp}");
    assert_eq!(prev["stage"], "gate", "{lp}");
    assert_eq!(prev["checklist"].as_array().map(Vec::len), Some(2), "{lp}");
    assert_eq!(prev["checklist"][1]["title"], "Show the page number in the CLI", "{lp}");
    let next = &v["next"];
    assert_eq!(next["command"], "hexa loop resume", "{next}");
    assert!(next["text"].as_str().unwrap().contains("Show the page number in the CLI"), "{next}");
}

#[test]
fn the_session_start_banner_shows_the_previous_checklist_and_suggests_resume() {
    let (_t, home, repo) = project();
    previous_work(&home, &repo);
    let payload = serde_json::json!({ "session_id": NEW, "hook_event_name": "SessionStart", "cwd": repo.to_str().unwrap() });
    let out = ok(hexa_as(NEW, &home, &repo, &["hook", "session-start"], &payload.to_string()));
    assert!(out.contains("previous session"), "{out}");
    assert!(out.contains("Show the page number in the CLI"), "{out}");
    assert!(out.contains("hexa loop resume"), "{out}");
}

#[test]
fn loop_resume_copies_the_previous_entry_and_the_hooks_accept_edits_under_it() {
    let (_t, home, repo) = project();
    previous_work(&home, &repo);
    // Before: this session has no gate, so a write to source is refused.
    let before = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(before.status.code(), Some(2), "{}", String::from_utf8_lossy(&before.stderr));

    let out = ok(hexa_as(NEW, &home, &repo, &["loop", "resume"], ""));
    assert!(out.contains("cargo test"), "{out}");
    let file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo.join(".hexa/loop.json")).unwrap()).unwrap();
    let mine = &file["sessions"][NEW];
    assert_eq!(mine["gate"], "cargo test", "{file}");
    assert_eq!(mine["stage"], "gate", "{file}");
    assert_eq!(mine["tasks"].as_array().map(Vec::len), Some(2), "{file}");
    let gate_at = chrono::DateTime::parse_from_rfc3339(mine["gate_at"].as_str().unwrap()).unwrap();
    let done_at = chrono::DateTime::parse_from_rfc3339(mine["tasks"][0]["done_at"].as_str().unwrap()).unwrap();
    assert!(gate_at > done_at, "gate_at is reset to now, after the last step was checked off: {file}");
    // The previous session's entry is left as it was.
    assert_eq!(file["sessions"]["prev"]["gate"], "cargo test", "{file}");

    let after = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(after.status.code(), Some(0), "{}", String::from_utf8_lossy(&after.stderr));
    // And this session now reads as its own work, not the previous one's.
    let shown = ok(hexa_as(NEW, &home, &repo, &["loop"], ""));
    assert!(!shown.contains("previous session"), "{shown}");
    assert!(shown.contains("[x] 1 Fix the page count"), "{shown}");
}

#[test]
fn loop_resume_with_nothing_to_resume_says_so() {
    let (_t, home, repo) = project();
    let o = hexa_as(NEW, &home, &repo, &["loop", "resume"], "");
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no ended session"), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn loop_resume_does_not_overwrite_this_sessions_own_work() {
    let (_t, home, repo) = project();
    previous_work(&home, &repo);
    ok(hexa_as(NEW, &home, &repo, &["loop", "gate", "cargo test --test mine"], ""));
    let o = hexa_as(NEW, &home, &repo, &["loop", "resume"], "");
    assert!(!o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(String::from_utf8_lossy(&o.stderr).contains("hexa loop clear"), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn an_ended_session_that_recorded_nothing_is_not_the_previous_work() {
    let (_t, home, repo) = project();
    previous_work(&home, &repo);
    // A later session that only touched a file: it recorded no work.
    ok(hexa_as("empty", &home, &repo, &["loop", "stage", "decide"], ""));
    let v: serde_json::Value = serde_json::from_str(&ok(hexa_as(NEW, &home, &repo, &["status", "--json"], ""))).unwrap();
    assert_eq!(v["loop"]["previous"]["gate"], "cargo test", "{}", v["loop"]);
}
