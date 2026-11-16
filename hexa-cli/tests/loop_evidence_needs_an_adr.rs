//! `hexa loop evidence` appends to the recorded ADR at `stage done`. With no
//! ADR there is nowhere to put it, and `stage done` used to exit 0 having run
//! nothing. The evidence is now refused when it is recorded, and `stage done`
//! refuses evidence it cannot record. A gate whose command runs no test runner
//! hexa reads is warned about when it is recorded.

use std::path::Path;
use std::process::Command;

fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".hexa")).unwrap();
    std::fs::create_dir_all(d.path().join("docs/adrs")).unwrap();
    std::fs::write(d.path().join("docs/adrs/ADR-0001-first.md"), "# ADR-0001: first\n").unwrap();
    d
}

fn run(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(dir)
        .env("HEXA_SESSION_ID", "evidence-test")
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
}

#[test]
fn evidence_without_an_adr_is_refused_when_recorded() {
    let d = project();
    let (ok, text) = run(d.path(), &["loop", "evidence", "echo measured"]);
    assert!(!ok, "{text}");
    assert!(text.contains("hexa loop adr"), "names the fix: {text}");
    let (_, shown) = run(d.path(), &["loop"]);
    assert!(!shown.contains("evidence echo measured"), "nothing was recorded: {shown}");
}

#[test]
fn evidence_with_an_adr_is_appended_at_done() {
    let d = project();
    assert!(run(d.path(), &["loop", "adr", "ADR-0001"]).0);
    assert!(run(d.path(), &["loop", "gate", "true", "--uncounted"]).0);
    assert!(run(d.path(), &["loop", "evidence", "echo MEASURED-42"]).0);
    let (ok, text) = run(d.path(), &["loop", "stage", "done"]);
    assert!(ok, "{text}");
    let adr = std::fs::read_to_string(d.path().join("docs/adrs/ADR-0001-first.md")).unwrap();
    assert!(adr.contains("MEASURED-42"), "{adr}");
}

#[test]
fn done_refuses_evidence_it_has_no_adr_to_record_in() {
    // State written before the refusal existed: evidence, no ADR.
    let d = project();
    assert!(run(d.path(), &["loop", "gate", "true", "--uncounted"]).0);
    let file = d.path().join(".hexa/loop.json");
    let mut state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    for (_, entry) in state["sessions"].as_object_mut().unwrap() {
        entry["evidence"] = "echo LOST".into();
    }
    std::fs::write(&file, state.to_string()).unwrap();

    let (ok, text) = run(d.path(), &["loop", "stage", "done"]);
    assert!(!ok, "{text}");
    assert!(text.contains("evidence") && text.contains("hexa loop adr"), "{text}");
}

#[test]
fn a_gate_that_runs_no_known_test_runner_is_warned_about() {
    let d = project();
    let (ok, text) = run(d.path(), &["loop", "gate", "true"]);
    assert!(ok, "the gate is still recorded: {text}");
    assert!(text.contains("--uncounted"), "{text}");

    let (ok, text) = run(d.path(), &["loop", "gate", "cargo test --test add"]);
    assert!(ok);
    assert!(!text.contains("--uncounted"), "a test runner draws no warning: {text}");

    let (_, text) = run(d.path(), &["loop", "gate", "true", "--uncounted"]);
    assert!(!text.contains("will need"), "a declared uncounted gate is not warned about: {text}");

    let (_, text) = run(d.path(), &["loop", "gate", "make check"]);
    assert!(!text.contains("will need"), "an indirection cannot be classified, so no warning: {text}");
}
