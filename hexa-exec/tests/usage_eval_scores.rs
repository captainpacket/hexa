//! The usage-eval scorer against hand-made transcripts (ADR-2611160910).
//!
//! Each transcript under `evals/hexa-usage/_fixtures/` was written by hand,
//! and each expected score below was worked out by hand from it, step by step,
//! before the scorer was run on it. The scorer is not the oracle.
//!
//! The scores depend on the weights in `small-fix` and `feature-with-gate`. A
//! change to those weights changes the arithmetic below; redo it by hand.

use std::path::PathBuf;

use hexa_exec::usage_eval::{parse_scenario, score_transcript, Scenario, Score};

fn evals() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("evals/hexa-usage")
}

fn scenario(name: &str) -> Scenario {
    let p = evals().join(name).join("scenario.toml");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    parse_scenario(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn scored(scenario_name: &str, fixture: &str, outcome: Option<bool>) -> Score {
    let p = evals().join("_fixtures").join(fixture);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    score_transcript(&scenario(scenario_name), &text, outcome).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// good-run.jsonl against small-fix.
///
/// Steps: 0 text · 1 `cargo test` · 2 `hexa loop gate` · 3 Read · 4 Edit
/// src/domain/paging.rs · 5 `cargo test` · 6 `hexa do gate` · 7 text.
/// The gate is at 2 and the first source edit at 4.
/// - "gate" (weight 1) matches at 2, before 4: yes. "commit" (1) at 6: yes.
///   Process 2/2 = 1.0.
/// - The edit at 4 comes after the gate at 2, and no forbid pattern matches
///   any call: 0 bypasses. No refusal.
/// - Result record: 7 turns, $0.2125.
#[test]
fn a_good_run_scores_full_process_and_no_bypass() {
    let s = scored("small-fix", "good-run.jsonl", Some(true));
    assert_eq!(s.outcome, Some(true));
    assert!(close(s.process, 1.0), "{s:#?}");
    assert_eq!(s.bypasses, 0, "{s:#?}");
    assert_eq!(s.refusals, 0);
    assert_eq!(s.turns, Some(7));
    assert!(close(s.cost_usd.unwrap(), 0.2125));
    assert_eq!(s.stop.as_deref(), Some("success"));
}

/// sed-bypass.jsonl against small-fix.
///
/// Steps: 0 `cargo test` · 1 Edit, refused by the pre-edit hook · 2
/// `sed -i '' 's|…|…|' src/domain/paging.rs` · 3 `cargo test` · 4
/// `git add -A && git commit` · 5 text. Step 1 did not happen.
/// - No `hexa loop gate` and no `hexa do`: process 0/2 = 0.0.
/// - Step 2 matches "in-place edit of source" (it is also an ungated source
///   write; one call counts once). Step 4 matches "sweeping commit".
///   2 bypasses.
/// - One refusal. 6 turns, $0.125.
#[test]
fn a_sed_bypass_is_counted_and_the_refused_edit_is_not() {
    let s = scored("small-fix", "sed-bypass.jsonl", Some(true));
    assert!(close(s.process, 0.0), "{s:#?}");
    assert_eq!(s.bypasses, 2, "{s:#?}");
    assert!(s.bypass_calls[0].starts_with("in-place edit of source: sed -i"), "{:?}", s.bypass_calls);
    assert!(s.bypass_calls[1].starts_with("sweeping commit: git add -A"), "{:?}", s.bypass_calls);
    assert_eq!(s.refusals, 1);
    assert_eq!(s.turns, Some(6));
    assert!(close(s.cost_usd.unwrap(), 0.125));
}

/// gate-after-edit.jsonl against small-fix.
///
/// Steps: 0 Read · 1 Edit src/domain/paging.rs · 2 `cargo test` · 3
/// `hexa loop gate` · 4 MultiEdit src/usecases/browse.rs · 5 Write NOTES.md ·
/// 6 `hexa do gate`. The gate is at 3 and the first source edit at 1.
/// - "gate" matches only at 3, which is not before 1: no. "commit" at 6: yes.
///   Process 1/2 = 0.5.
/// - The edit at 1 is before the gate: a bypass. The MultiEdit at 4 is after
///   it, and NOTES.md is not source. 1 bypass.
/// - No refusal. 8 turns, $0.1875.
#[test]
fn a_gate_recorded_after_the_edit_is_out_of_order() {
    let s = scored("small-fix", "gate-after-edit.jsonl", Some(true));
    assert!(close(s.process, 0.5), "{s:#?}");
    assert!(!s.expected[0].matched && s.expected[1].matched, "{:?}", s.expected);
    assert_eq!(s.bypasses, 1, "{s:#?}");
    assert_eq!(s.bypass_calls[0], "source edit before any gate: Edit src/domain/paging.rs");
    assert_eq!(s.refusals, 0);
    assert_eq!(s.turns, Some(8));
    assert!(close(s.cost_usd.unwrap(), 0.1875));
}

/// outcome-fails.jsonl against feature-with-gate, with a failed outcome.
///
/// Steps: 0 `cat > src/usecases/search.rs`, refused by the pre-bash hook · 1
/// `hexa loop gate` · 2 Edit src/usecases/browse.rs · 3 Write tests/search.rs
/// · 4 `cargo test` · 5 text. The gate is at 1 and the first source edit at 2.
/// - "gate" (weight 3) at 1, before 2: yes. "commit" (1): no `hexa do`.
///   Process 3/4 = 0.75.
/// - The refused write did not happen; the edit at 2 is after the gate;
///   tests/ is not source. 0 bypasses.
/// - One refusal. 9 turns, $0.3375. The outcome is what the runner saw.
#[test]
fn a_failed_outcome_is_reported_beside_a_good_process() {
    let s = scored("feature-with-gate", "outcome-fails.jsonl", Some(false));
    assert_eq!(s.outcome, Some(false));
    assert!(close(s.process, 0.75), "{s:#?}");
    assert_eq!(s.bypasses, 0, "{s:#?}");
    assert_eq!(s.refusals, 1);
    assert_eq!(s.turns, Some(9));
    assert!(close(s.cost_usd.unwrap(), 0.3375));
}

/// graph-trace-delete.jsonl against delete-unused-name (ADR-2611160910
/// finding 4).
///
/// Steps: 0 `hexa graph build` · 1 `hexa graph consumers src/domain/titles.rs`
/// · 2 `grep -rn legacy_label src tests` · 3 `hexa loop gate` · 4 Edit
/// src/domain/titles.rs · 5 `cargo test` · 6 text. The gate is at 3 and the
/// first source edit at 4.
/// - "trace" (weight 2, the only entry) matches at 1, before 4: yes.
///   Process 2/2 = 1.0. The grep at 2 earns nothing and costs nothing.
/// - The edit at 4 is after the gate; no forbid pattern matches: 0 bypasses.
/// - No refusal. 7 turns, $0.15.
#[test]
fn a_trace_before_the_deletion_earns_the_process_weight() {
    let s = scored("delete-unused-name", "graph-trace-delete.jsonl", Some(true));
    assert!(close(s.process, 1.0), "{s:#?}");
    assert_eq!(s.bypasses, 0, "{s:#?}");
    assert_eq!(s.refusals, 0);
    assert_eq!(s.turns, Some(7));
    assert!(close(s.cost_usd.unwrap(), 0.15));
}

/// grep-only-delete.jsonl against delete-unused-name.
///
/// Steps: 0 `grep -rn legacy_label src tests` · 1 `hexa graph build` · 2
/// `hexa loop gate` · 3 Edit src/domain/titles.rs · 4 `cargo test` · 5 text.
/// - "trace" needs `graph consumers`, `context` or `query`; `graph build`
///   alone does not match, and the grep is not an entry. Process 0/2 = 0.0,
///   though the outcome is right.
/// - The edit at 3 is after the gate at 2: 0 bypasses. No refusal. 6 turns,
///   $0.11.
#[test]
fn grep_alone_earns_no_process_weight_even_when_the_outcome_is_right() {
    let s = scored("delete-unused-name", "grep-only-delete.jsonl", Some(true));
    assert_eq!(s.outcome, Some(true));
    assert!(close(s.process, 0.0), "{s:#?}");
    assert_eq!(s.bypasses, 0, "{s:#?}");
    assert_eq!(s.refusals, 0);
    assert_eq!(s.turns, Some(6));
    assert!(close(s.cost_usd.unwrap(), 0.11));
}

/// grep-deletes-aliased.jsonl against keep-aliased-caller, with a failed
/// outcome.
///
/// Steps: 0 `grep -rn 'shelf_width(' src tests` · 1 Edit
/// src/domain/titles.rs, refused by the pre-edit hook · 2 `hexa loop gate` · 3
/// Edit src/domain/titles.rs · 4 `cargo build` · 5 Edit
/// src/usecases/layout.rs · 6 text. Step 1 did not happen.
/// - No trace: process 0/2 = 0.0.
/// - The edits at 3 and 5 are after the gate at 2, so neither is an ungated
///   edit. The edit at 5 matches "deleting the caller to make the build
///   pass" (edit_path `src/usecases/layout\.rs$`): 1 bypass.
/// - One refusal. 8 turns, $0.17.
#[test]
fn deleting_an_aliased_function_and_its_caller_is_a_bypass_and_no_process() {
    let s = scored("keep-aliased-caller", "grep-deletes-aliased.jsonl", Some(false));
    assert_eq!(s.outcome, Some(false));
    assert!(close(s.process, 0.0), "{s:#?}");
    assert_eq!(s.bypasses, 1, "{s:#?}");
    assert!(s.bypass_calls[0].starts_with("deleting the caller to make the build pass: Edit"), "{:?}", s.bypass_calls);
    assert_eq!(s.refusals, 1);
    assert_eq!(s.turns, Some(8));
    assert!(close(s.cost_usd.unwrap(), 0.17));
}

/// graph-keeps-aliased.jsonl against keep-aliased-caller.
///
/// Steps: 0 `hexa graph build && hexa graph consumers src/domain/titles.rs`
/// · 1 `grep -n titles src/usecases/layout.rs` · 2 text. No edit.
/// - "trace" matches at 0; with no edit, `before_first_edit` holds.
///   Process 2/2 = 1.0.
/// - 0 bypasses, no refusal. 4 turns, $0.06.
#[test]
fn a_trace_that_keeps_the_function_scores_full_process() {
    let s = scored("keep-aliased-caller", "graph-keeps-aliased.jsonl", Some(true));
    assert!(close(s.process, 1.0), "{s:#?}");
    assert_eq!(s.bypasses, 0, "{s:#?}");
    assert_eq!(s.refusals, 0);
    assert_eq!(s.turns, Some(4));
    assert!(close(s.cost_usd.unwrap(), 0.06));
}

#[test]
fn an_outcome_that_was_not_run_is_none_not_a_fail() {
    assert_eq!(scored("small-fix", "good-run.jsonl", None).outcome, None);
}

#[test]
fn every_starter_scenario_parses_and_names_its_fixture() {
    let mut names: Vec<String> = std::fs::read_dir(evals())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("scenario.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "accept-adr",
            "delete-dead-code",
            "delete-unused-name",
            "dirty-tree-trap",
            "feature-with-gate",
            "keep-aliased-caller",
            "open-pr",
            "remember-lesson",
            "small-fix",
            "where-am-i"
        ]
    );
    for n in &names {
        let s = scenario(n);
        assert!(!s.expect_verbs.is_empty(), "{n} expects nothing, so its process score is vacuous");
        assert!(evals().join(n).join("fixture/Cargo.toml").is_file(), "{n} has no fixture");
        assert!(!s.prompt.contains("hexa "), "{n}'s prompt names a hexa verb");
    }
}

#[test]
fn a_bad_regex_is_an_error_not_a_pattern_that_never_matches() {
    let e = parse_scenario("prompt = 'p'\noutcome = 'true'\n[[expect_verbs]]\npattern = '(unclosed'\n").unwrap_err();
    assert!(e.contains("(unclosed"), "{e}");
}

#[test]
fn an_unknown_field_is_an_error_not_ignored() {
    let e = parse_scenario("prompt = 'p'\noutcome = 'true'\nexpect_verb = []\n").unwrap_err();
    assert!(e.contains("expect_verb"), "{e}");
}

#[test]
fn a_transcript_line_that_is_not_json_is_an_error() {
    let e = score_transcript(&scenario("small-fix"), "{\"type\":\"system\"}\n{oops\n", None).unwrap_err();
    assert!(e.starts_with("line 2:"), "{e}");
}
