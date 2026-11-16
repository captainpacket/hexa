//! The usage eval's third finding (ADR-2611160910, baseline 20261116T144539Z):
//! after the hook refused an edit, the model recorded `cargo test --test
//! paging` as the gate, and it was accepted as the whole proof. When
//! `.hexa/project.json` declares the project's full gate (`gate.full`), `hexa
//! loop gate` with a different command warns, records that command as the
//! fast gate, and keeps the full gate as the one `stage done` and `do gate`
//! are decided by. It never refuses: a narrow gate is a legitimate fast gate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn project(project_json: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(repo.join(".hexa")).unwrap();
    std::fs::write(repo.join(".hexa/project.json"), project_json).unwrap();
    std::fs::create_dir_all(tmp.path().join("home")).unwrap();
    (tmp, repo)
}

fn hexa(home: &Path, repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_CODE_SESSION_ID", "narrow")
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env_remove("HEXA_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .output()
        .unwrap()
}

fn entry(repo: &Path) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo.join(".hexa/loop.json")).unwrap()).unwrap();
    v["sessions"]["narrow"].clone()
}

fn out(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

const FULL: &str = r#"{"name":"p","id":"00000000-test","gate":{"full":"cargo test"}}"#;

#[test]
fn a_narrower_gate_warns_and_is_recorded_as_the_fast_gate() {
    let (t, repo) = project(FULL);
    let home = t.path().join("home");
    let o = hexa(&home, &repo, &["loop", "gate", "cargo test --test paging"]);
    assert!(o.status.success(), "never refused: {}", out(&o));
    let text = out(&o);
    assert!(text.contains("fast gate"), "says it is recorded as the fast gate: {text}");
    assert!(text.contains("cargo test --test paging"), "{text}");
    assert!(text.contains("stage done") && text.contains("do gate"), "says the full gate still decides: {text}");
    let e = entry(&repo);
    assert_eq!(e["fast_gate"], "cargo test --test paging", "{e}");
    assert_eq!(e["gate"], "cargo test", "the full gate stays the gate: {e}");
    assert!(e["gate_at"].is_string(), "the hooks see a current gate: {e}");
}

#[test]
fn the_full_gate_itself_is_recorded_without_a_warning() {
    let (t, repo) = project(FULL);
    let home = t.path().join("home");
    let o = hexa(&home, &repo, &["loop", "gate", "cargo test"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(!out(&o).contains("fast gate"), "{}", out(&o));
    let e = entry(&repo);
    assert_eq!(e["gate"], "cargo test");
    assert!(e.get("fast_gate").is_none_or(|v| v.is_null()), "{e}");
}

#[test]
fn with_no_full_gate_declared_the_command_is_the_gate() {
    let (t, repo) = project(r#"{"name":"p","id":"00000000-test"}"#);
    let home = t.path().join("home");
    let o = hexa(&home, &repo, &["loop", "gate", "cargo test --test paging"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(!out(&o).contains("fast gate"), "{}", out(&o));
    assert_eq!(entry(&repo)["gate"], "cargo test --test paging");
}
