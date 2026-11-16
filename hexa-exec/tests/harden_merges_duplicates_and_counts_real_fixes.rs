//! A harden pass verifies and fixes each defect once, and counts only a fix
//! that changed code.
//!
//! Seen in a real run: five candidates were two bugs found three times, all
//! "confirmed real", each fixed separately. Candidates from different lenses
//! at one location with one title (once normalized) are one candidate: the
//! first is kept and names the lenses merged into it. And a fix whose changed
//! files are all tests fixed nothing: it is reported "test only".

#[path = "support/harden_fake.rs"]
mod fake;

use fake::{good_fix, repo, subjects, test_only_fix, Fake, GATE, TARGET};
use hexa_exec::adversarial::{run_review_gated, HardenGates};

fn gates() -> HardenGates {
    HardenGates { gate: GATE.to_string(), fast_gate: None, batch: None, baseline: true, lessons: Vec::new() }
}

fn finding(title: &str, location: &str, lens: &str) -> String {
    serde_json::json!({"title": title, "location": location, "description": "d", "lens": lens}).to_string()
}

#[tokio::test]
async fn one_bug_found_by_three_lenses_is_verified_and_fixed_once() {
    let (_tmp, dir) = repo();
    let loc = format!("{TARGET}:1");
    let hunts = vec![
        format!("{},{}", finding("bug-1", &loc, "correctness"), finding("bug-2", &format!("{TARGET}:2"), "correctness")),
        finding("Bug-1", &loc, "concurrency"),
        finding("bug-1.", &format!(" ./{loc} "), "edges"),
        finding("bug-2", &format!("{TARGET}:2"), "durability-safety"),
    ];
    let frontier = Fake::with_hunts(2, hunts, good_fix());
    let report = run_review_gated(frontier.clone(), hexa_exec::default_git_history(), TARGET, &gates(), &dir).await;

    assert_eq!(report.candidate, 5, "the raw count is kept");
    assert_eq!(frontier.verified.lock().unwrap().len(), 2, "each defect is verified once");
    let titles: Vec<&str> = report.confirmed.iter().map(|f| f.title.as_str()).collect();
    assert_eq!(titles, vec!["bug-1", "bug-2"], "the first of each is kept");
    assert_eq!(report.confirmed[0].lens, "correctness+concurrency+edges");
    assert_eq!(report.confirmed[1].lens, "correctness+durability-safety");
    assert_eq!(report.fixed, vec!["bug-1", "bug-2"], "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("duplicate")), "{:?}", report.notes);
    assert!(report.verdict_line().contains("3 duplicate"), "{}", report.verdict_line());
}

#[tokio::test]
async fn the_same_title_at_two_locations_is_two_candidates() {
    let (_tmp, dir) = repo();
    let hunts = vec![
        finding("bug-1", &format!("{TARGET}:1"), "correctness"),
        finding("bug-1", &format!("{TARGET}:9"), "edges"),
    ];
    let frontier = Fake::with_hunts(1, hunts, good_fix());
    let _ = run_review_gated(frontier.clone(), hexa_exec::default_git_history(), TARGET, &gates(), &dir).await;
    assert_eq!(frontier.verified.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_fix_that_changed_only_tests_is_test_only_not_fixed() {
    let (_tmp, dir) = repo();
    let frontier = Fake::new(1, test_only_fix());
    let report = run_review_gated(frontier, hexa_exec::default_git_history(), TARGET, &gates(), &dir).await;

    assert!(report.fixed.is_empty(), "{:?}", report.notes);
    assert_eq!(report.test_only, vec!["bug-1"], "{:?}", report.notes);
    assert!(report.verdict_line().contains("1 test only"), "{}", report.verdict_line());
    assert_eq!(subjects(&dir), vec!["init"], "nothing committed under a fix(harden) subject");
    assert!(!dir.join("tests/bug_1.txt").exists(), "the test-only change is reverted");
}
