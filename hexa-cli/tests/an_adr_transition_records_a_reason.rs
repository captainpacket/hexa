//! An ADR transition records a reason someone gave.
//!
//! `adr accept`, `complete` and `supersede` used to write a placeholder line
//! ("operator: accepted via `hexa adr accept`") when no reason was given. The
//! usage eval watched a model hand-edit that line afterwards: the placeholder
//! recorded a decision nobody explained. The reason is now required, and a
//! blank one is refused before anything is written.
//!
//! Ids are assembled at run time: the citation checker scans source, and a
//! literal id here would claim a decision exists.

use std::path::Path;
use std::process::Command;

struct Run {
    code: i32,
    out: String,
}

fn run(dir: &Path, args: &[&str]) -> Run {
    let home = dir.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(dir)
        .env("HOME", &home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", dir)
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env("NO_COLOR", "1")
        .output()
        .expect("run hexa");
    Run { code: o.status.code().unwrap_or(-1), out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)) }
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn id(digits: &str) -> String {
    format!("ADR-{digits}")
}

/// Two Proposed ADRs in a committed temp repo.
fn ledger() -> (tempfile::TempDir, String, String) {
    let (a, b) = (id("2701040000"), id("2701050000"));
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("docs/adrs")).unwrap();
    for (x, name) in [(&a, "first"), (&b, "second")] {
        let body = format!("# {x}: A decision\n\n**Status:** Proposed\n**Date:** 2026-11-01\n\n## Context\n\nx\n\n## Decision\n\n1. **It holds.** Always.\n\n## Consequences\n\nx\n");
        std::fs::write(d.path().join(format!("docs/adrs/{x}-{name}.md")), body).unwrap();
    }
    std::fs::write(d.path().join(".gitignore"), ".test-home/\n").unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-qm", "init"]);
    (d, a, b)
}

fn text(d: &Path, x: &str) -> String {
    let f = std::fs::read_dir(d.join("docs/adrs")).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with(x)).unwrap();
    std::fs::read_to_string(f.path()).unwrap()
}

#[test]
fn accept_without_a_reason_is_refused_and_writes_nothing() {
    let (d, a, _) = ledger();
    let before = text(d.path(), &a);
    let r = run(d.path(), &["adr", "accept", &a]);
    assert_ne!(r.code, 0, "no reason, no transition:\n{}", r.out);
    assert!(r.out.contains("--rationale"), "the refusal names what to give:\n{}", r.out);
    let r = run(d.path(), &["adr", "accept", &a, "--rationale", "   "]);
    assert_ne!(r.code, 0, "a blank reason is no reason:\n{}", r.out);
    assert!(r.out.contains("--rationale"), "{}", r.out);
    assert_eq!(text(d.path(), &a), before, "nothing changed");
    assert!(!text(d.path(), &a).contains("operator:"), "no placeholder line");
}

#[test]
fn complete_and_supersede_need_a_reason_too() {
    let (d, a, b) = ledger();
    assert_eq!(run(d.path(), &["adr", "accept", &a, "--rationale", "signed off"]).code, 0);
    let before = text(d.path(), &a);
    assert_ne!(run(d.path(), &["adr", "complete", &a, "--force"]).code, 0);
    assert_ne!(run(d.path(), &["adr", "supersede", &a, "--by", &b]).code, 0);
    assert_eq!(text(d.path(), &a), before, "nothing changed");
}

#[test]
fn supersede_still_refuses_a_bad_replacement_when_a_reason_is_given() {
    // The refusal is about --by, not about the missing reason.
    let (d, a, _) = ledger();
    let before = text(d.path(), &a);
    for by in [a.clone(), id("2799999999"), id("2701")] {
        let r = run(d.path(), &["adr", "supersede", &a, "--by", &by, "--rationale", "replaced"]);
        assert_ne!(r.code, 0, "--by {by}:\n{}", r.out);
        assert!(!r.out.contains("give the reason"), "refused for the replacement, not the reason:\n{}", r.out);
    }
    assert_eq!(text(d.path(), &a), before, "nothing changed");
}
