//! The features an ADR names and where each stands: the domain rules
//! (decisions 1 to 4, 8, and the order of decision 5).
//!
//! Written from the scenarios of `docs/specs/<this ADR>.json`, before the
//! code, one test per scenario. Every case calls the port
//! (`hexa_core::ports::adr_features`) on ADR text built here; nothing reads a
//! file. The command-line scenarios are in
//! `hexa-cli/tests/adr_2611250845_spec.rs`.
//!
//! Fixture ids are assembled at run time: the citation checker scans source,
//! and a literal id here would claim a decision exists.

use hexa_core::ports::adr_board::{analyze, State};
use hexa_core::ports::adr_features::{features_of, sort_features, AdrFeatures, Feature, FeatureState};

// ── fixtures ────────────────────────────────────────────────────────────

fn id(n: u32) -> String {
    format!("ADR-{n}")
}

const HEADER: &str = "| Feature | What it does | Phase |\n|---|---|---|\n";

/// The table of decision 1 with these rows.
fn table(rows: &[&str]) -> String {
    let mut t = String::from(HEADER);
    for r in rows {
        t.push_str(r);
        t.push('\n');
    }
    t
}

/// A `## Features` section holding `body`.
fn section(body: &str) -> String {
    format!("## Features\n\n{body}\n")
}

/// An ADR's text: its title, status line, an optional `**Built:**` line, its
/// phases, and whatever stands where the Features section would.
fn adr(title: &str, status: &str, built: Option<&str>, phases: &[&str], features: Option<&str>) -> String {
    let mut s = format!("# {title}\n\n**Status:** {status}\n");
    if let Some(b) = built {
        s.push_str(b);
        s.push('\n');
    }
    s.push_str("**Date:** 2026-11-14\n\n## Context\n\nWhy.\n\n## Decision\n\n1. Do it.\n\n");
    if !phases.is_empty() {
        s.push_str("## Phases\n\n");
        for p in phases {
            s.push_str(&format!("### {p}. Phase {p}\n\nWork.\n\n"));
        }
    }
    if let Some(f) = features {
        s.push_str(f);
        s.push('\n');
    }
    s.push_str("## Consequences\n\n- One.\n");
    s
}

fn names(a: &AdrFeatures) -> Vec<&str> {
    a.features.iter().map(|f| f.feature.as_str()).collect()
}

fn named<'a>(a: &'a AdrFeatures, name: &str) -> &'a Feature {
    a.features
        .iter()
        .find(|f| f.feature == name)
        .unwrap_or_else(|| panic!("no feature named {name:?} in {:?}", names(a)))
}

fn built(f: &Feature) -> (usize, usize) {
    (f.built.done, f.built.total)
}

const BUILT_R1: &str = "**Built:** R1 on 2026-11-14.";
const BUILT_F1: &str = "**Built:** F1 on 2026-11-14.";

/// The ADR texts of S05 to S09, each with the status word it is read under.
fn texts_s05_to_s09() -> Vec<(&'static str, String)> {
    let w = section(&table(&["| W | w | |"]));
    let p = section(&table(&["| P | p | |"]));
    vec![
        // S05
        ("accepted", adr("Five", "Accepted", Some(BUILT_R1), &["R1", "R2"], Some(&section(&table(&["| A | a | R1 |", "| B | b | R2 |", "| Whole | all of it | |"]))))),
        // S06
        ("accepted", adr("Six a", "Accepted", Some(BUILT_F1), &[], Some(&w))),
        ("accepted", adr("Six b", "Accepted", Some(BUILT_R1), &["R1", "R2"], Some(&w))),
        ("accepted", adr("Six c", "Accepted", None, &[], Some(&w))),
        // S07
        ("proposed", adr("Seven a", "Proposed", None, &[], Some(&p))),
        ("superseded", adr("Seven b", "Superseded", None, &[], Some(&p))),
        ("draft", adr("Seven c", "Draft", None, &[], Some(&p))),
        // S08
        ("accepted", adr("Eight a", "Accepted", None, &[], Some(&section(&table(&["| X | x | |"]))))),
        ("accepted", adr("Eight b", "Accepted", Some(BUILT_R1), &["R1", "R2"], Some(&section(&table(&["| Y | y | R1, R2 |", "| Whole | all of it | |"]))))),
        // S09
        ("completed", adr("Nine", "Completed", None, &["R1", "R2"], Some(&section(&table(&["| Z | z | R2 |", "| Whole | all of it | |"]))))),
    ]
}

// ── decision 1: the section and its table ───────────────────────────────

#[test]
fn s01_a_features_table_gives_one_row_per_feature_with_its_sentence_and_phase() {
    let content = adr(
        "Impact",
        "Accepted",
        None,
        &["R1", "R2"],
        Some(&section(&table(&[
            "| Blast radius | Says what goes down with a device | R2 |",
            "| Impact list | Lists the affected services | |",
        ]))),
    );
    let a = features_of(&id(1), "Impact", "accepted", &content);

    assert!(a.declared, "an ADR with a Features section is declared");
    assert_eq!(names(&a), ["Blast radius", "Impact list"]);

    let blast = named(&a, "Blast radius");
    assert_eq!(blast.what, "Says what goes down with a device");
    assert_eq!(blast.phases, ["R2"]);

    let list = named(&a, "Impact list");
    assert_eq!(list.what, "Lists the affected services");
    assert!(list.phases.is_empty(), "an empty Phase cell names no phase: {:?}", list.phases);

    for f in &a.features {
        assert_eq!(f.adr, id(1));
        assert_eq!(f.adr_title, "Impact");
        assert!(f.declared, "{} comes from the table, so it is declared", f.feature);
    }
}

#[test]
fn s02_columns_match_without_case_what_stands_for_what_it_does_and_a_row_names_several_phases() {
    let read = |row: &str| {
        let body = format!("| feature | WHAT | phase |\n|---|---|---|\n{row}\n");
        let content = adr("Tracing", "Accepted", None, &["R1", "R2", "R3"], Some(&section(&body)));
        features_of(&id(2), "Tracing", "accepted", &content)
    };

    let commas = read("| Trace | Follows a cable | R1, R3 |");
    assert_eq!(names(&commas), ["Trace"]);
    let trace = named(&commas, "Trace");
    assert_eq!(trace.what, "Follows a cable");
    assert_eq!(trace.phases, ["R1", "R3"]);

    let spaces = read("| Trace | Follows a cable | R1 R3 |");
    assert_eq!(named(&spaces, "Trace").phases, ["R1", "R3"], "phases separated by a space read the same");
    assert_eq!(named(&spaces, "Trace").what, "Follows a cable");
}

#[test]
fn s03_a_table_in_a_code_fence_or_after_the_first_table_is_not_read() {
    let body = format!(
        "An example:\n\n```\n{fenced}```\n\n{real}\nAnd later:\n\n{second}",
        fenced = table(&["| Fenced | no | |"]),
        real = table(&["| Real | yes | |"]),
        second = table(&["| Second | no | |"]),
    );
    let content = adr("Fences", "Accepted", None, &[], Some(&section(&body)));
    let a = features_of(&id(3), "Fences", "accepted", &content);

    assert!(a.declared);
    assert_eq!(names(&a), ["Real"], "only the first table outside a fence is read");
}

#[test]
fn s04_a_features_section_with_no_rows_declares_no_feature() {
    let header_only = adr("A CI rule", "Accepted", None, &[], Some(&section(HEADER)));
    let a = features_of(&id(4), "A CI rule", "accepted", &header_only);
    assert!(a.declared, "a header and a separator are still a Features section");
    assert!(a.features.is_empty(), "no row, so no feature, and no title standing in: {:?}", names(&a));

    let sentence = adr("A naming rule", "Accepted", None, &[], Some(&section("This decision gives no feature.\n")));
    let b = features_of(&id(5), "A naming rule", "accepted", &sentence);
    assert!(b.declared, "a heading and a sentence are still a Features section");
    assert!(b.features.is_empty(), "no table, so no feature, and no title standing in: {:?}", names(&b));
}

// ── decision 2: the state ───────────────────────────────────────────────

#[test]
fn s05_a_feature_is_completed_when_its_phase_is_built_and_accepted_when_it_is_not() {
    let content = adr(
        "Five",
        "Accepted",
        Some(BUILT_R1),
        &["R1", "R2"],
        Some(&section(&table(&["| A | a | R1 |", "| B | b | R2 |"]))),
    );
    let got = features_of(&id(5), "Five", "accepted", &content);

    let a = named(&got, "A");
    assert_eq!(a.state, FeatureState::Completed);
    assert_eq!(built(a), (1, 1));

    let b = named(&got, "B");
    assert_eq!(b.state, FeatureState::Accepted);
    assert_eq!(built(b), (0, 1));
}

#[test]
fn s06_a_row_that_names_no_phase_takes_the_whole_adr() {
    let w = section(&table(&["| W | w | |"]));

    let phaseless_built = adr("Six a", "Accepted", Some(BUILT_F1), &[], Some(&w));
    let a = features_of(&id(61), "Six a", "accepted", &phaseless_built);
    assert_eq!(named(&a, "W").state, FeatureState::Completed);
    assert_eq!(built(named(&a, "W")), (1, 1));

    let half_built = adr("Six b", "Accepted", Some(BUILT_R1), &["R1", "R2"], Some(&w));
    let b = features_of(&id(62), "Six b", "accepted", &half_built);
    assert_eq!(named(&b, "W").state, FeatureState::Accepted, "one phase of two is not the whole ADR");
    assert_eq!(built(named(&b, "W")), (1, 2));

    let nothing_built = adr("Six c", "Accepted", None, &[], Some(&w));
    let c = features_of(&id(63), "Six c", "accepted", &nothing_built);
    assert_eq!(named(&c, "W").state, FeatureState::Accepted);
    assert_eq!(built(named(&c, "W")), (0, 1));
}

#[test]
fn s07_proposed_retired_and_unreadable_adrs_give_their_features_those_states() {
    let p = section(&table(&["| P | p | |"]));

    let proposed = features_of(&id(71), "Seven a", "proposed", &adr("Seven a", "Proposed", None, &[], Some(&p)));
    assert_eq!(named(&proposed, "P").state, FeatureState::Proposed);

    let retired = features_of(&id(72), "Seven b", "superseded", &adr("Seven b", "Superseded", None, &[], Some(&p)));
    assert_eq!(named(&retired, "P").state, FeatureState::Retired);

    let unreadable = features_of(&id(73), "Seven c", "draft", &adr("Seven c", "Draft", None, &[], Some(&p)));
    assert_eq!(named(&unreadable, "P").state, FeatureState::Unknown);
}

#[test]
fn s08_the_status_word_alone_never_completes_a_feature_and_one_phase_of_two_is_not_completed() {
    let no_evidence = adr("Eight a", "Accepted", None, &[], Some(&section(&table(&["| X | x | |"]))));
    let a = features_of(&id(81), "Eight a", "accepted", &no_evidence);
    let x = named(&a, "X");
    assert_eq!(x.state, FeatureState::Accepted, "accepted is a decision, not a build");
    assert_ne!(x.state, FeatureState::Completed);

    let one_of_two = adr(
        "Eight b",
        "Accepted",
        Some(BUILT_R1),
        &["R1", "R2"],
        Some(&section(&table(&["| Y | y | R1, R2 |"]))),
    );
    let b = features_of(&id(82), "Eight b", "accepted", &one_of_two);
    let y = named(&b, "Y");
    assert_eq!(y.phases, ["R1", "R2"]);
    assert_eq!(y.state, FeatureState::Accepted);
    assert_eq!(built(y), (1, 2));
}

#[test]
fn s09_a_completed_status_builds_every_phase_as_the_board_reads_it() {
    let content = adr("Nine", "Completed", None, &["R1", "R2"], Some(&section(&table(&["| Z | z | R2 |"]))));
    let a = features_of(&id(9), "Nine", "completed", &content);
    let z = named(&a, "Z");
    assert_eq!(z.state, FeatureState::Completed);
    assert_eq!(built(z), (1, 1));
}

// ── decision 3: undeclared ──────────────────────────────────────────────

#[test]
fn s10_an_adr_with_no_features_section_gives_one_row_its_title_with_declared_false() {
    let title = format!("{}: The power read", id(7));
    let content = adr(&title, "Accepted", Some(BUILT_F1), &[], None);
    let a = features_of(&id(7), &title, "accepted", &content);

    assert!(!a.declared, "no Features section, so undeclared");
    assert_eq!(names(&a), ["The power read"], "the title stands in, without its id prefix");
    let row = &a.features[0];
    assert_eq!(row.state, FeatureState::Completed);
    assert!(!row.declared);
    assert!(row.phases.is_empty());
    assert_eq!(row.adr, id(7));
}

#[test]
fn s11_a_retired_adr_with_no_features_section_gives_no_row() {
    let title = format!("{}: The old way", id(11));
    let content = adr(&title, "Superseded", None, &[], None);
    let a = features_of(&id(11), &title, "superseded", &content);
    assert!(!a.declared);
    assert!(a.features.is_empty(), "a retired, undeclared ADR gives no row: {:?}", names(&a));
}

// ── decision 4: a pure function that agrees with the board ──────────────

#[test]
fn s12_the_state_of_a_feature_agrees_with_the_boards_analysis_of_the_same_text() {
    let mut checked = 0;
    let mut seen: Vec<FeatureState> = Vec::new();
    for (n, (status, content)) in texts_s05_to_s09().iter().enumerate() {
        let board = analyze(status, content).state;
        let expected = match board {
            State::Built => FeatureState::Completed,
            State::Retired => FeatureState::Retired,
            State::Unknown => FeatureState::Unknown,
            State::Proposed => FeatureState::Proposed,
            State::Accepted | State::Building => FeatureState::Accepted,
        };
        let got = features_of(&id(120 + n as u32), "Agreement", status, content);
        for f in got.features.iter().filter(|f| f.phases.is_empty()) {
            assert_eq!(
                f.state, expected,
                "text {n} ({status}): the board reads {board:?}, the feature {:?} reads {:?}",
                f.feature, f.state
            );
            checked += 1;
            if !seen.contains(&f.state) {
                seen.push(f.state);
            }
        }
    }
    assert!(checked >= 10, "every text has a row that names no phase; only {checked} were compared");
    for state in [
        FeatureState::Completed,
        FeatureState::Accepted,
        FeatureState::Proposed,
        FeatureState::Retired,
        FeatureState::Unknown,
    ] {
        assert!(seen.contains(&state), "no text exercised {state:?}");
    }
}

#[test]
fn s13_features_of_reads_nothing_but_its_arguments() {
    let content = adr(
        "Thirteen",
        "Accepted",
        Some(BUILT_R1),
        &["R1", "R2"],
        Some(&section(&table(&["| Here | h | R1 |", "| There | t | |"]))),
    );
    let before = features_of(&id(13), "Thirteen", "accepted", &content);
    assert_eq!(names(&before), ["Here", "There"]);

    // A directory with no docs/adrs in it or above it, and no repository.
    let bare = std::env::temp_dir().join(format!("hexa-adr-features-s13-{}", std::process::id()));
    std::fs::create_dir_all(&bare).unwrap();
    assert!(!bare.join("docs").exists() && !bare.join(".git").exists());
    let home = std::env::current_dir().unwrap();
    std::env::set_current_dir(&bare).unwrap();
    let elsewhere = features_of(&id(13), "Thirteen", "accepted", &content);
    let again = features_of(&id(13), "Thirteen", "accepted", &content);
    std::env::set_current_dir(&home).unwrap();
    let _ = std::fs::remove_dir_all(&bare);

    assert_eq!(elsewhere, before, "the directory it runs in changes nothing");
    assert_eq!(again, elsewhere, "two calls return equal values");
}

// ── decision 5: the order ───────────────────────────────────────────────

#[test]
fn s15_sort_features_orders_by_state_then_name_without_case_then_adr_id() {
    let built = adr("Built", "Accepted", Some(BUILT_F1), &[], Some(&section(&table(&["| zeta | z | |", "| Alpha | a | |"]))));
    let beta = section(&table(&["| beta | b | |"]));
    let accepted = adr("Accepted", "Accepted", None, &[], Some(&beta));
    let proposed = adr("Proposed", "Proposed", None, &[], Some(&section(&table(&["| Gamma | g | |"]))));

    let mut rows: Vec<Feature> = Vec::new();
    rows.extend(features_of(&id(4), "Proposed", "proposed", &proposed).features);
    rows.extend(features_of(&id(2), "Accepted", "accepted", &accepted).features);
    rows.extend(features_of(&id(3), "Built", "accepted", &built).features);
    rows.extend(features_of(&id(1), "Accepted", "accepted", &accepted).features);
    assert_eq!(rows.len(), 5);

    let expected = [
        ("Alpha".to_string(), id(3)),
        ("zeta".to_string(), id(3)),
        ("beta".to_string(), id(1)),
        ("beta".to_string(), id(2)),
        ("Gamma".to_string(), id(4)),
    ];
    let order = |rows: &[Feature]| -> Vec<(String, String)> { rows.iter().map(|f| (f.feature.clone(), f.adr.clone())).collect() };

    sort_features(&mut rows);
    assert_eq!(order(&rows), expected);

    // Any shuffle sorts to the same order.
    rows.reverse();
    sort_features(&mut rows);
    assert_eq!(order(&rows), expected);
    rows.rotate_left(2);
    rows.swap(0, 3);
    sort_features(&mut rows);
    assert_eq!(order(&rows), expected);
}

// ── decision 8: what the section says and the ADR cannot support ────────

#[test]
fn s23_a_row_naming_a_phase_the_adr_does_not_have_is_unknown_with_a_warning() {
    let content = adr("Ghosts", "Accepted", None, &["R1", "R2"], Some(&section(&table(&["| Ghost | g | R9 |"]))));
    let a = features_of(&id(23), "Ghosts", "accepted", &content);
    let ghost = named(&a, "Ghost");
    assert_eq!(ghost.state, FeatureState::Unknown);
    assert_eq!(ghost.warnings, ["unknown-phase"]);
}

#[test]
fn s24_no_feature_column_is_undeclared_an_empty_name_is_skipped_and_duplicates_are_kept_and_flagged() {
    let title = format!("{}: The rack view", id(241));
    let no_column = adr(&title, "Accepted", None, &[], Some(&section("| Name | What |\n|---|---|\n| Rack | Shows a rack |\n")));
    let a = features_of(&id(241), &title, "accepted", &no_column);
    assert_eq!(a.warnings, ["no-feature-column"]);
    assert!(!a.declared, "a table with no Feature column declares nothing");
    assert_eq!(names(&a), ["The rack view"], "the title stands in, as for an ADR with no section");
    assert!(!a.features[0].declared);

    let nameless = adr("Nameless", "Accepted", None, &[], Some(&section(&table(&["|  | nameless | |", "| Kept | k | |"]))));
    let b = features_of(&id(242), "Nameless", "accepted", &nameless);
    assert_eq!(b.warnings, ["empty-feature"]);
    assert_eq!(names(&b), ["Kept"]);

    let twice = adr("Twice", "Accepted", None, &[], Some(&section(&table(&["| Dup | one | |", "| dup | two | |"]))));
    let c = features_of(&id(243), "Twice", "accepted", &twice);
    assert_eq!(names(&c), ["Dup", "dup"], "both rows are kept");
    for f in &c.features {
        assert_eq!(f.warnings, ["duplicate-feature"], "{} is one of two with the same name", f.feature);
    }
}
