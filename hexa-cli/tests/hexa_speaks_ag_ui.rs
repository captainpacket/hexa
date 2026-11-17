//! hexa speaks AG-UI (ADR-2611161830): a working verb with `--events` reports
//! itself as an AG-UI 1.1 event stream, JSON lines on stdout and nothing else,
//! every line valid against the protocol's own schema.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn validator() -> jsonschema::Validator {
    let raw = include_str!("../../hexa-core/tests/fixtures/ag-ui/schema-draft-1.1.json");
    jsonschema::draft202012::new(&serde_json::from_str::<Value>(raw).unwrap()).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn hexa(dir: &Path, args: &[&str]) -> Out {
    let home = dir.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(dir)
        .env("HOME", &home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HEXA_SESSION_ID", "ag-ui-session")
        .env_remove("HEXA_EVENTS")
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    Out { code: o.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&o.stdout).into(), stderr: String::from_utf8_lossy(&o.stderr).into() }
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(d.path().join(".hexa")).unwrap();
    std::fs::write(d.path().join(".gitignore"), ".hexa/\n.test-home/\n").unwrap();
    std::fs::write(d.path().join("a.txt"), "one\n").unwrap();
    std::fs::write(d.path().join("fail.sh"), "#!/bin/sh\necho 'test result: FAILED. 0 passed; 1 failed; 0 ignored'\nexit 1\n").unwrap();
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-q", "-m", "init"]);
    d
}

/// Every stdout line is a schema-valid event; returns them.
fn events(out: &Out) -> Vec<Value> {
    let v = validator();
    assert!(!out.stdout.trim().is_empty(), "no events: stderr {}", out.stderr);
    out.stdout
        .lines()
        .map(|l| {
            let doc: Value = serde_json::from_str(l).unwrap_or_else(|e| panic!("stdout carries only events, not {l:?} ({e})"));
            let errs: Vec<String> = v.iter_errors(&doc).map(|e| e.to_string()).collect();
            assert!(errs.is_empty(), "{l}: {errs:?}");
            doc
        })
        .collect()
}

fn types(evs: &[Value]) -> Vec<&str> {
    evs.iter().map(|e| e["type"].as_str().unwrap()).collect()
}

#[test]
fn finish_with_events_is_one_run_whose_activity_is_the_open_work() {
    let d = repo();
    std::fs::write(d.path().join("a.txt"), "changed\n").unwrap();
    let out = hexa(d.path(), &["finish", "--events"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let evs = events(&out);
    let t = types(&evs);
    assert_eq!(t.first(), Some(&"RUN_STARTED"));
    assert_eq!(t.last(), Some(&"RUN_FINISHED"));
    assert_eq!(evs[0]["protocolVersion"], "1.1");
    assert_eq!(evs[0]["threadId"], "ag-ui-session", "the thread is the loop session");
    let act: Vec<&Value> = evs.iter().filter(|e| e["type"] == "ACTIVITY_SNAPSHOT").collect();
    assert_eq!(act.len(), 1);
    assert_eq!(act[0]["activityType"], "hexa.open_work");
    assert!(act[0]["content"]["open"].as_array().unwrap().iter().any(|i| i["kind"] == "uncommitted"), "{}", act[0]);
    assert!(out.stderr.contains("open work"), "the person's text goes to stderr: {}", out.stderr);
}

#[test]
fn events_to_a_file_leave_stdout_to_the_person() {
    let d = repo();
    let path = d.path().join(".hexa").join("events.jsonl");
    let out = hexa(d.path(), &["finish", &format!("--events={}", path.display())]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("nothing open"), "stdout {} stderr {}", out.stdout, out.stderr);
    let file = Out { code: 0, stdout: std::fs::read_to_string(&path).unwrap(), stderr: String::new() };
    assert_eq!(types(&events(&file)), vec!["RUN_STARTED", "ACTIVITY_SNAPSHOT", "RUN_FINISHED"]);
}

#[test]
fn a_failing_gate_ends_the_run_in_run_error_with_its_steps_closed() {
    let d = repo();
    std::fs::write(d.path().join("a.txt"), "changed\n").unwrap();
    let out = hexa(d.path(), &["do", "gate", "--uncounted", "--files", "a.txt", "-e", "sh fail.sh", "-m", "x", "--events"]);
    assert_ne!(out.code, 0);
    let evs = events(&out);
    let t = types(&evs);
    assert_eq!(t.first(), Some(&"RUN_STARTED"));
    assert_eq!(t.last(), Some(&"RUN_ERROR"), "{t:?}");
    assert_eq!(evs.last().unwrap()["code"], "gate_failed");
    let opened = evs.iter().position(|e| e["type"] == "STEP_STARTED" && e["stepName"] == "gate").expect("a gate step");
    let closed = evs.iter().position(|e| e["type"] == "STEP_FINISHED" && e["stepName"] == "gate").expect("closed");
    assert!(opened < closed && closed < evs.len() - 1);
}
