//! `hexa do run` names the stage a failed run stopped at, and `hexa do runs`
//! lists this repository's runs, `do gate` runs included.
//!
//! The audit: with no model configured, `do run` printed "failed evidence
//! fail" though the evidence never ran; `do runs` listed every project's runs;
//! and `do gate` runs never appeared in it.

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.name", "t"]);
    git(d.path(), &["config", "user.email", "t@x"]);
    std::fs::write(d.path().join("a.rs"), "// a\n").unwrap();
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-qm", "init"]);
    d
}

fn hexa(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(dir)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("HEXA_PROJECT_ROOT", dir)
        .env_remove("HEXA_DIRECT_MODEL")
        .env_remove("HEXA_REACT_MODEL")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

#[test]
fn a_run_with_no_model_fails_at_inference_not_at_evidence() {
    let r = repo();
    let home = tempfile::tempdir().unwrap();
    let out = hexa(r.path(), home.path(), &["do", "run", "edit a", "--file", "a.rs", "--evidence", "touch evidence-ran"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(!r.path().join("evidence-ran").exists(), "the evidence never ran: {t}");
    assert!(!t.contains("evidence fail"), "it did not fail at evidence: {t}");
    assert!(t.contains("failed at inference") && t.contains("evidence not run"), "{t}");
}

#[test]
fn do_gate_runs_are_listed_and_only_this_repositorys() {
    let a = repo();
    let b = repo();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(a.path().join("a.rs"), "// a2\n").unwrap();
    let gated = hexa(
        a.path(),
        home.path(),
        &["do", "gate", "--files", "a.rs", "--evidence", "true", "--uncounted", "--message", "gate-run-marker"],
    );
    assert!(gated.status.success(), "{}", text(&gated));

    let in_a = text(&hexa(a.path(), home.path(), &["do", "runs"]));
    assert!(in_a.contains("gate-run-marker"), "{in_a}");
    assert!(in_a.contains("1 runs"), "{in_a}");

    let in_b = text(&hexa(b.path(), home.path(), &["do", "runs"]));
    assert!(!in_b.contains("gate-run-marker"), "another repository's run is not listed: {in_b}");
    assert!(in_b.contains("--all"), "says what is hidden and how to see it: {in_b}");

    let all = text(&hexa(b.path(), home.path(), &["do", "runs", "--all"]));
    assert!(all.contains("gate-run-marker"), "{all}");
}
