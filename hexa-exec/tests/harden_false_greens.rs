//! No false greens from harden or build (ADR-2611160730 §5).
//!
//! A gate that exits 0 having run no tests is not a pass, wherever harden
//! or build runs it: the baseline, after a fix, the final gate, build's gate.
//! Before this ADR only the fast gate asked. And a fix is "fixed" only once
//! its new test was shown red without the fix; otherwise it is reported
//! `fixed (untested)` and the commit kept.

#[path = "support/harden_fake.rs"]
mod fake;

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use fake::{git, good_fix, repo, subjects, Fake, FixAction, GATE, TARGET};
use hexa_exec::adversarial::{run_review_gated, run_review_with, HardenGates};
use hexa_exec::ports::{Frontier, FrontierError};

/// `true`, with a test runner's line saying it ran nothing.
const VACUOUS: &str = "echo 'running 0 tests'; echo 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured'; true";

fn gates(gate: &str, fast: Option<&str>, baseline: bool) -> HardenGates {
    HardenGates { gate: gate.to_string(), fast_gate: fast.map(str::to_string), batch: None, baseline }
}

#[tokio::test]
async fn a_zero_test_gate_is_vacuous_and_fails_harden() {
    let (_tmp, dir) = repo();
    let frontier = Fake::new(1, good_fix());
    let report = run_review_with(frontier.clone(), TARGET, VACUOUS, &dir, Arc::new(|_| {})).await;

    let base = report.baseline.clone().expect("the baseline is red: the gate ran no tests");
    assert!(!base.passed);
    assert!(base.tail.contains("ran no tests"), "the gate's run says why: {}", base.tail);
    assert!(report.fixed.is_empty());
    assert_eq!(frontier.calls(), 0, "nothing was hunted on a gate that proves nothing");
}

#[tokio::test]
async fn a_zero_test_gate_fails_each_fix_and_the_final_gate() {
    let (_tmp, dir) = repo();
    let report = run_review_gated(Fake::new(1, good_fix()), hexa_exec::default_git_history(), TARGET, &gates(VACUOUS, None, false), &dir).await;

    assert!(report.fixed.is_empty(), "a fix gated by nothing is not fixed: {:?}", report.notes);
    assert!(report.gate_run && !report.gate_passed, "{}", report.gate_line());
    assert!(report.gate_runs.iter().all(|r| !r.passed && r.tail.contains("ran no tests")), "{:?}", report.gate_runs);
    assert_eq!(subjects(&dir), vec!["init"], "nothing committed");
}

/// The escape hatch is the loop's: a gate recorded `--uncounted` is judged
/// by its exit status alone (ADR-2611081151 §6).
#[tokio::test]
async fn a_gate_recorded_uncounted_is_judged_by_its_exit_status() {
    let (_tmp, dir) = repo();
    std::fs::create_dir_all(dir.join(".hexa")).unwrap();
    let state = serde_json::json!({ "sessions": { "s": { "gate": VACUOUS, "gate_uncounted": true } } });
    std::fs::write(dir.join(".hexa/loop.json"), state.to_string()).unwrap();
    let report = run_review_gated(Fake::new(1, good_fix()), hexa_exec::default_git_history(), TARGET, &gates(VACUOUS, None, true), &dir).await;

    assert!(report.baseline.is_none(), "{:?}", report.baseline);
    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    assert!(report.gate_passed, "{}", report.gate_line());
    assert!(report.notes.iter().any(|n| n.contains("uncounted")), "the report says so: {:?}", report.notes);
}

// ── red first ────────────────────────────────────────────────────────────────

/// The fix's test passes without the fix: the gate never needed it. The
/// commit stays; the finding is reported untested.
#[tokio::test]
async fn a_fix_whose_test_passes_without_the_fix_is_reported_untested() {
    let (_tmp, dir) = repo();
    let report = run_review_gated(Fake::new(1, good_fix()), hexa_exec::default_git_history(), TARGET, &gates(GATE, None, true), &dir).await;

    assert_eq!(report.fixed, vec!["bug-1"], "the commit is kept: {:?}", report.notes);
    assert_eq!(report.untested, vec!["bug-1"], "notes: {:?}", report.notes);
    assert!(report.verdict_line().contains("untested"), "{}", report.verdict_line());
    assert!(
        report.notes.iter().any(|n| n.contains("fixed (untested)") && n.contains("bug-1")),
        "{:?}",
        report.notes
    );
    assert_eq!(subjects(&dir), vec!["fix(harden): bug-1", "init"]);
    assert_eq!(std::fs::read_to_string(dir.join(TARGET)).unwrap(), "original\nfix 1\n", "the fix was restored");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "", "the red-first check leaves nothing behind");
}

/// The fix's test fails without the fix: it is fixed, and not untested.
#[tokio::test]
async fn a_fix_whose_test_is_red_without_the_fix_is_fixed() {
    let (_tmp, dir) = repo();
    // Red only when the test exists and the fix does not.
    let gate = "if [ -f tests/bug_1.txt ] && ! grep -q 'fix 1' src/target.txt; then echo 'test result: FAILED. 0 passed; 1 failed;'; exit 1; fi; echo 'test result: ok. 1 passed; 0 failed;'";
    let report = run_review_gated(Fake::new(1, good_fix()), hexa_exec::default_git_history(), TARGET, &gates(gate, None, true), &dir).await;

    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    assert!(report.untested.is_empty(), "the test was red without the fix: {:?}", report.notes);
    assert!(!report.verdict_line().contains("untested"), "{}", report.verdict_line());
    assert_eq!(std::fs::read_to_string(dir.join(TARGET)).unwrap(), "original\nfix 1\n");
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}

/// A fix that changed no test file has nothing to show red: untested.
#[tokio::test]
async fn a_fix_with_no_test_file_is_untested() {
    let (_tmp, dir) = repo();
    let fix: FixAction = Arc::new(|cwd: &Path, n: usize| {
        let body = std::fs::read_to_string(cwd.join(TARGET)).unwrap() + &format!("fix {n}\n");
        std::fs::write(cwd.join(TARGET), body).unwrap();
    });
    let report = run_review_gated(Fake::new(1, fix), hexa_exec::default_git_history(), TARGET, &gates(GATE, None, true), &dir).await;

    assert_eq!(report.fixed, vec!["bug-1"]);
    assert_eq!(report.untested, vec!["bug-1"], "notes: {:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("no test file")), "{:?}", report.notes);
}

/// With a fast gate, the red-first check runs the fast gate only, on the
/// test files.
#[tokio::test]
async fn with_a_fast_gate_the_red_first_check_uses_only_the_fast_gate() {
    let (tmp, dir) = repo();
    let log = tmp.path().join("runs.log");
    let full = format!("echo full >> '{}'; {GATE}", log.display());
    let fast = format!("echo fast {{touched}} >> '{}'; {GATE}", log.display());
    let report = run_review_gated(Fake::new(1, good_fix()), hexa_exec::default_git_history(), TARGET, &gates(&full, Some(&fast), false), &dir).await;

    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    assert_eq!(report.untested, vec!["bug-1"]);
    let runs = std::fs::read_to_string(&log).unwrap();
    let fast_runs: Vec<&str> = runs.lines().filter(|l| l.starts_with("fast")).collect();
    assert_eq!(fast_runs.len(), 2, "the fix's fast gate, then the red-first fast gate: {runs}");
    assert_eq!(fast_runs[1], "fast tests/bug_1.txt", "the red-first run is given the test files: {runs}");
    assert_eq!(runs.lines().filter(|l| *l == "full").count(), 1, "one full gate, the batch's: {runs}");
}

// ── build ────────────────────────────────────────────────────────────────────

struct Builder;

#[async_trait]
impl Frontier for Builder {
    async fn run(&self, prompt: &str, cwd: &Path, _t: std::time::Duration, _s: &str) -> Result<String, FrontierError> {
        if prompt.starts_with("Implement the following spec") {
            std::fs::create_dir_all(cwd.join("src/lib")).unwrap();
            std::fs::write(cwd.join("src/lib/built.txt"), "built\n").unwrap();
            return Ok("built".to_string());
        }
        Ok("a design".to_string())
    }
}

#[tokio::test]
async fn a_zero_test_gate_fails_build_and_commits_nothing() {
    let (_tmp, dir) = repo();
    let report = hexa_exec::adversarial::run_build(Arc::new(Builder), "a challenge", "src/lib", VACUOUS, 2, &dir, 30, 0).await;

    assert!(!report.build_ok, "notes: {:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("ran no tests")), "{:?}", report.notes);
    assert_eq!(subjects(&dir), vec!["init"]);
}
