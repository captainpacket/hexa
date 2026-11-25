//! The features an ADR names, and where each stands (ADR-2611250845).
//!
//! A feature's state is the board's reading of its ADR (`adr_board::analyze`),
//! never a second opinion: this module reads only the `## Features` section
//! itself. Everything here is a pure function of the ADR's text.

use std::collections::BTreeMap;

use serde::Serialize;

use super::adr_board::{analyze, phase_heading, unfenced_lines, Analysis, State, Units};

/// A row names a phase the ADR does not have. The row's state is `unknown`.
pub const WARN_UNKNOWN_PHASE: &str = "unknown-phase";
/// The section has a table with no `Feature` column. The ADR is undeclared.
pub const WARN_NO_FEATURE_COLUMN: &str = "no-feature-column";
/// A row whose `Feature` cell is empty. The row is skipped.
pub const WARN_EMPTY_FEATURE: &str = "empty-feature";
/// Two rows of one ADR with the same name, ignoring case. Both are kept.
pub const WARN_DUPLICATE_FEATURE: &str = "duplicate-feature";

/// Where a feature stands (decision 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureState {
    /// What builds the feature is built.
    Completed,
    /// The ADR is accepted; what builds the feature is not built, or only part of it.
    Accepted,
    /// The ADR is proposed; what builds the feature is not built.
    Proposed,
    /// The ADR is superseded, rejected, deprecated or abandoned.
    Retired,
    /// The ADR's status could not be read, or the row names a phase the ADR does not have.
    Unknown,
}

impl FeatureState {
    /// Every state, in the order of the table.
    pub const ALL: [FeatureState; 5] = [
        FeatureState::Completed,
        FeatureState::Accepted,
        FeatureState::Proposed,
        FeatureState::Retired,
        FeatureState::Unknown,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FeatureState::Completed => "completed",
            FeatureState::Accepted => "accepted",
            FeatureState::Proposed => "proposed",
            FeatureState::Retired => "retired",
            FeatureState::Unknown => "unknown",
        }
    }

    /// One of the four states a caller may ask for by name (decision 5).
    /// `unknown` is a finding, not a state to ask for.
    pub fn named(name: &str) -> Option<FeatureState> {
        [FeatureState::Proposed, FeatureState::Accepted, FeatureState::Completed, FeatureState::Retired]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// One row of the table: a feature, as one ADR gives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Feature {
    /// The feature's name, as a person would say it.
    pub feature: String,
    /// One sentence, or empty when the row gives none.
    pub what: String,
    pub state: FeatureState,
    /// The ADR's id, `ADR-2611250845`.
    pub adr: String,
    pub adr_title: String,
    /// The phases of the ADR that build the feature; empty means the whole ADR.
    pub phases: Vec<String>,
    /// How much of what builds the feature is built. `phases` inside is empty: the ids are in `Feature::phases`.
    pub built: Units,
    /// False when the ADR has no `## Features` section and its title stands in (decision 3).
    pub declared: bool,
    /// `unknown-phase`, `duplicate-feature` (decision 8).
    pub warnings: Vec<String>,
}

/// What one ADR gives.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdrFeatures {
    /// True when the ADR has a `## Features` section, with or without rows.
    pub declared: bool,
    pub features: Vec<Feature>,
    /// Findings about the section as a whole: `no-feature-column`, `empty-feature` (decision 8).
    pub warnings: Vec<String>,
}

// ── the section and its table ───────────────────────────────────────────

/// The lines of the `## Features` section, without its heading. `lines` are
/// the lines outside any code fence.
fn section_of<'a>(lines: &[&'a str]) -> Option<Vec<&'a str>> {
    let start = lines.iter().position(|l| l.trim_end().eq_ignore_ascii_case("## features"))?;
    let rest = &lines[start + 1..];
    let end = rest.iter().position(|l| l.starts_with("## ")).unwrap_or(rest.len());
    Some(rest[..end].to_vec())
}

fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// The cells of one table line. `\|` is a pipe inside a cell.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if c == '|' {
            out.push(cell.trim().to_string());
            cell.clear();
        } else {
            cell.push(c);
        }
    }
    if !cell.trim().is_empty() {
        out.push(cell.trim().to_string());
    }
    out
}

/// `|---|:--|`: the line between a table's header and its rows.
fn is_separator(row: &[String]) -> bool {
    !row.is_empty() && row.iter().all(|c| c.contains('-') && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

struct Table {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// Every table in `lines`, in order: a run of lines that start with `|`.
fn tables(lines: &[&str]) -> Vec<Table> {
    let mut found = Vec::new();
    let mut run: Vec<Vec<String>> = Vec::new();
    let mut close = |run: &mut Vec<Vec<String>>| {
        if let Some((header, rows)) = run.split_first() {
            found.push(Table { header: header.clone(), rows: rows.iter().filter(|r| !is_separator(r)).cloned().collect() });
        }
        run.clear();
    };
    for line in lines {
        if is_table_line(line) {
            run.push(cells(line));
        } else {
            close(&mut run);
        }
    }
    close(&mut run);
    found
}

/// Where the columns of decision 1 are in a header row.
struct Columns {
    feature: usize,
    what: Option<usize>,
    phase: Option<usize>,
}

fn columns(header: &[String]) -> Option<Columns> {
    let at = |names: &[&str]| header.iter().position(|h| names.iter().any(|n| h.eq_ignore_ascii_case(n)));
    Some(Columns { feature: at(&["feature"])?, what: at(&["what it does", "what"]), phase: at(&["phase"]) })
}

/// One row as the table gives it, before the ADR is asked what is built.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureRow {
    pub feature: String,
    pub what: String,
    pub phases: Vec<String>,
}

/// The rows of a table that name a feature, and how many rows named none.
fn rows_of(table: &Table, cols: &Columns) -> (Vec<FeatureRow>, usize) {
    let cell = |row: &[String], at: Option<usize>| at.and_then(|i| row.get(i)).cloned().unwrap_or_default();
    let mut rows = Vec::new();
    let mut nameless = 0;
    for row in &table.rows {
        let feature = cell(row, Some(cols.feature));
        if feature.is_empty() {
            nameless += 1;
            continue;
        }
        let phases = cell(row, cols.phase)
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|p| p.trim_matches('`'))
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        rows.push(FeatureRow { feature, what: cell(row, cols.what), phases });
    }
    (rows, nameless)
}

/// True when the ADR has a `## Features` section outside any code fence,
/// whatever it holds.
pub fn has_features_section(content: &str) -> bool {
    section_of(&unfenced_lines(content)).is_some()
}

// ── the state ───────────────────────────────────────────────────────────

/// Decision 2. `done` of `total` is what builds the feature; the ADR's own
/// state, retired or unreadable, comes first.
fn state_of(analysis: &Analysis, done: usize, total: usize) -> FeatureState {
    match analysis.state {
        State::Retired => FeatureState::Retired,
        State::Unknown => FeatureState::Unknown,
        _ if total > 0 && done == total => FeatureState::Completed,
        _ if analysis.status == "proposed" && done == 0 => FeatureState::Proposed,
        _ => FeatureState::Accepted,
    }
}

/// The title without the id an ADR's heading starts with: `The power read`
/// from `ADR-7: The power read`.
fn without_id<'a>(id: &str, title: &'a str) -> &'a str {
    let t = title.trim();
    match t.get(..id.len()) {
        Some(head) if head.eq_ignore_ascii_case(id) => {
            let rest = t[id.len()..].trim_start();
            match rest.strip_prefix(':') {
                Some(after) => after.trim_start(),
                None if rest.len() < t.len() - id.len() => rest,
                None => t,
            }
        }
        _ => t,
    }
}

/// Read one ADR. `id` and `title` are the ADR's own; `status` is the
/// normalised status word the adapter already parsed, as `adr_board::analyze`
/// takes it; `content` is the whole file.
pub fn features_of(id: &str, title: &str, status: &str, content: &str) -> AdrFeatures {
    let analysis = analyze(status, content);
    let adr_title = without_id(id, title).to_string();
    let whole = |done: usize, total: usize| Units { done, total, phases: Vec::new() };

    // Decision 3: the title stands in, unless the ADR is retired.
    let undeclared = |warnings: Vec<String>| {
        let features = if analysis.state == State::Retired {
            Vec::new()
        } else {
            let (done, total) = (analysis.units.done, analysis.units.total);
            vec![Feature {
                feature: adr_title.clone(),
                what: String::new(),
                state: state_of(&analysis, done, total),
                adr: id.to_string(),
                adr_title: adr_title.clone(),
                phases: Vec::new(),
                built: whole(done, total),
                declared: false,
                warnings: Vec::new(),
            }]
        };
        AdrFeatures { declared: false, features, warnings }
    };

    let lines = unfenced_lines(content);
    let Some(section) = section_of(&lines) else {
        return undeclared(Vec::new());
    };
    // Decision 1: the first table of the section is the one that is read.
    let Some(table) = tables(&section).into_iter().next() else {
        return AdrFeatures { declared: true, features: Vec::new(), warnings: Vec::new() };
    };
    let Some(cols) = columns(&table.header) else {
        return undeclared(vec![WARN_NO_FEATURE_COLUMN.to_string()]);
    };

    let (rows, nameless) = rows_of(&table, &cols);
    let warnings = vec![WARN_EMPTY_FEATURE.to_string(); nameless];
    let lowered: Vec<String> = rows.iter().map(|r| r.feature.to_lowercase()).collect();

    let features = rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let mut row_warnings = Vec::new();
            let named: Vec<bool> = row
                .phases
                .iter()
                .filter_map(|p| analysis.units.phases.iter().find(|have| &have.id == p))
                .map(|have| have.built)
                .collect();
            let (state, built) = if row.phases.is_empty() {
                let (done, total) = (analysis.units.done, analysis.units.total);
                (state_of(&analysis, done, total), whole(done, total))
            } else if named.len() < row.phases.len() {
                row_warnings.push(WARN_UNKNOWN_PHASE.to_string());
                (FeatureState::Unknown, whole(named.iter().filter(|b| **b).count(), row.phases.len()))
            } else {
                let done = named.iter().filter(|b| **b).count();
                (state_of(&analysis, done, named.len()), whole(done, named.len()))
            };
            if lowered.iter().filter(|name| **name == lowered[i]).count() > 1 {
                row_warnings.push(WARN_DUPLICATE_FEATURE.to_string());
            }
            Feature {
                feature: row.feature,
                what: row.what,
                state,
                adr: id.to_string(),
                adr_title: adr_title.clone(),
                phases: row.phases,
                built,
                declared: true,
                warnings: row_warnings,
            }
        })
        .collect();
    AdrFeatures { declared: true, features, warnings }
}

/// The order of the table: by state as `FeatureState` orders them, then by
/// feature name without regard to case, then by ADR id (decision 5).
pub fn sort_features(rows: &mut [Feature]) {
    rows.sort_by(|a, b| {
        a.state
            .cmp(&b.state)
            .then_with(|| a.feature.to_lowercase().cmp(&b.feature.to_lowercase()))
            .then_with(|| a.adr.cmp(&b.adr))
            .then_with(|| a.feature.cmp(&b.feature))
    });
}

/// The counts above the table (decision 5).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureSummary {
    /// Every row, in every state.
    pub features: usize,
    pub adrs: usize,
    /// ADRs with no `## Features` section whose title stands in for a feature.
    pub undeclared: usize,
    /// Rows by state; every state is named, at zero too.
    pub by_state: BTreeMap<String, usize>,
}

/// Count what the ADRs give. Every row counts, whatever a caller then lists.
pub fn summarize_features<'a>(all: impl IntoIterator<Item = &'a AdrFeatures>) -> FeatureSummary {
    let mut s = FeatureSummary {
        features: 0,
        adrs: 0,
        undeclared: 0,
        by_state: FeatureState::ALL.iter().map(|state| (state.name().to_string(), 0)).collect(),
    };
    for adr in all {
        s.adrs += 1;
        if !adr.declared && !adr.features.is_empty() {
            s.undeclared += 1;
        }
        for f in &adr.features {
            s.features += 1;
            *s.by_state.entry(f.state.name().to_string()).or_insert(0) += 1;
        }
    }
    s
}

// ── the release a feature shipped in (decision 6) ───────────────────────

/// A commit id as a `**Built` line names one: 7 to 40 lower-case hex digits
/// as a whole word, except the number of an `ADR-` id. A long number that is
/// not a commit reads as one that the repository does not hold, which leaves
/// the release unknown and never names a wrong one.
fn commits_on(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let word = &line[start..i];
        let hex = (7..=40).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let after_adr = start >= 4 && line[start - 4..start].eq_ignore_ascii_case("adr-");
        if hex && !after_adr && !found.iter().any(|c| c == word) {
            found.push(word.to_string());
        }
    }
    found
}

/// The commits named on the `**Built` lines that built `phases` of this ADR,
/// or the whole ADR when `phases` is empty: the header's `**Built:**` lines
/// (those naming one of the phases, when phases are asked for) and the
/// `**Built` lines under those phases' headings.
///
/// `None` when no such line exists or when one of them names no commit: the
/// release that holds the feature cannot be told from the record.
pub fn built_commits(content: &str, phases: &[String]) -> Option<Vec<String>> {
    let lines = unfenced_lines(content);
    let mut evidence: Vec<&str> = Vec::new();
    let mut seen_phases = false;
    let mut in_phases = false;
    let mut open: Option<String> = None;
    for line in &lines {
        if line.starts_with("## ") {
            in_phases = !seen_phases && line.trim_end().eq_ignore_ascii_case("## phases");
            seen_phases |= in_phases;
            open = None;
            continue;
        }
        if in_phases && line.starts_with("### ") {
            open = phase_heading(line).map(|(id, _)| id);
            continue;
        }
        let text = line.trim_start();
        if !text.starts_with("**Built") {
            continue;
        }
        let header = text.strip_prefix("**Built:**");
        let under = if in_phases { open.as_deref() } else { None };
        let wanted = if phases.is_empty() {
            header.is_some() || under.is_some()
        } else {
            header.is_some_and(|rest| {
                rest.split(|c: char| !c.is_ascii_alphanumeric()).any(|word| phases.iter().any(|p| p == word))
            }) || under.is_some_and(|id| phases.iter().any(|p| p == id))
        };
        if wanted {
            evidence.push(text);
        }
    }
    if evidence.is_empty() {
        return None;
    }
    let mut commits: Vec<String> = Vec::new();
    for line in evidence {
        let named = commits_on(line);
        if named.is_empty() {
            return None;
        }
        for c in named {
            if !commits.contains(&c) {
                commits.push(c);
            }
        }
    }
    Some(commits)
}

/// `vX.Y.Z` as three numbers; anything else is not a version tag.
fn version_of(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let mut number = || {
        let p = parts.next()?;
        (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse::<u64>().ok()).flatten()
    };
    let version = (number()?, number()?, number()?);
    parts.next().is_none().then_some(version)
}

/// The lowest version tag (`vX.Y.Z`, compared as numbers) among `tags`.
pub fn lowest_version<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<String> {
    tags.into_iter()
        .filter_map(|t| version_of(t).map(|v| (v, t)))
        .min()
        .map(|(_, t)| t.to_string())
}

// ── drafting the section (decision 9) ───────────────────────────────────

/// The rows a drafted reply names: its first table with a `Feature` column,
/// read as decision 1 reads one, whether or not the reply fenced it. Only the
/// name, the sentence and the phases are taken, and a phase the ADR does not
/// have (`known_phases`) is dropped, so a draft cannot say anything about
/// what is built. `None` when the reply holds no such table.
pub fn draft_rows(reply: &str, known_phases: &[String]) -> Option<Vec<FeatureRow>> {
    let lines: Vec<&str> = reply.lines().filter(|l| !l.trim_start().starts_with("```")).collect();
    let (table, cols) = tables(&lines).into_iter().find_map(|t| columns(&t.header).map(|c| (t, c)))?;
    let (mut rows, _) = rows_of(&table, &cols);
    for row in &mut rows {
        row.phases.retain(|p| known_phases.contains(p));
    }
    Some(rows)
}

fn cell_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").replace('|', "\\|")
}

/// The `## Features` section of decision 1 holding `rows`, ending in a newline.
pub fn render_section(rows: &[FeatureRow]) -> String {
    let mut s = String::from("## Features\n\n| Feature | What it does | Phase |\n|---|---|---|\n");
    for r in rows {
        let phase = if r.phases.is_empty() { String::new() } else { format!("{} ", r.phases.join(", ")) };
        s.push_str(&format!("| {} | {} | {}|\n", cell_text(&r.feature), cell_text(&r.what), phase));
    }
    s
}

/// `content` with `section` immediately before `## Consequences`, or at the
/// end when the ADR has none. A `## Consequences` inside a code fence is an
/// example, not the heading.
pub fn insert_section(content: &str, section: &str) -> String {
    let mut fenced = false;
    let mut offset = 0;
    let mut at = None;
    for line in content.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.trim_end().eq_ignore_ascii_case("## consequences") {
            at = Some(offset);
            break;
        }
        offset += line.len();
    }
    let section = format!("{}\n", section.trim_end());
    match at {
        Some(at) => {
            let before = content[..at].trim_end();
            let gap = if before.is_empty() { "" } else { "\n\n" };
            format!("{before}{gap}{section}\n{}", &content[at..])
        }
        None => format!("{}\n\n{section}", content.trim_end()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pipe_in_a_cell_is_escaped_and_read_back() {
        assert_eq!(cells("| a \\| b | c | |"), ["a | b", "c", ""]);
        assert_eq!(cells("| a | b"), ["a", "b"]);
    }

    #[test]
    fn the_title_loses_only_its_own_id() {
        assert_eq!(without_id("ADR-7", "ADR-7: The power read"), "The power read");
        assert_eq!(without_id("ADR-7", "ADR-70: Another"), "ADR-70: Another");
        assert_eq!(without_id("ADR-7", "The power read"), "The power read");
        assert_eq!(without_id("ADR-7", "ADR-7 The power read"), "The power read");
    }

    #[test]
    fn a_proposed_adr_with_part_built_reads_as_the_board_does() {
        let md = "**Built:** R1 on 2026-11-14.\n\n## Phases\n\n### R1. One\n### R2. Two\n\n## Features\n\n| Feature | Phase |\n|---|---|\n| Whole | |\n| First | R1 |\n| Second | R2 |\n";
        let a = features_of("ADR-1", "T", "proposed", md);
        let state = |n: &str| a.features.iter().find(|f| f.feature == n).unwrap().state;
        assert_eq!(state("Whole"), FeatureState::Accepted, "building, as the board says");
        assert_eq!(state("First"), FeatureState::Completed);
        assert_eq!(state("Second"), FeatureState::Proposed);
    }

    #[test]
    fn commits_are_whole_hex_words_and_not_counts_dates_or_adr_ids() {
        assert_eq!(commits_on("**Built:** F1 on 2026-11-14, 91d1e80."), ["91d1e80"]);
        assert_eq!(commits_on("**Built:** F1, `91d1e80` and 1234567, 12 tests, ADR-2611250845."), ["91d1e80", "1234567"]);
        assert!(commits_on("**Built:** F1 on 2026-11-14, on `feat/remote`, 120 tests.").is_empty());
        assert!(commits_on("**Built:** F1, 91D1E80, abcdefg.").is_empty());
    }

    #[test]
    fn built_commits_are_those_of_the_lines_that_built_the_row() {
        let md = "**Built:** R1 on 2026-11-14, aaaaaa1.\n**Built:** R2 on 2026-11-15.\n\n## Phases\n\n### R1. One\n### R2. Two\n### R3. Three\n**Built 2026-11-16, ccccc33.**\n\n```\n**Built:** R1 fffffff\n```\n";
        let p = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(built_commits(md, &p(&["R1"])), Some(vec!["aaaaaa1".to_string()]));
        assert_eq!(built_commits(md, &p(&["R3"])), Some(vec!["ccccc33".to_string()]));
        assert_eq!(built_commits(md, &p(&["R1", "R3"])), Some(vec!["aaaaaa1".to_string(), "ccccc33".to_string()]));
        assert_eq!(built_commits(md, &p(&["R2"])), None, "its line names no commit");
        assert_eq!(built_commits(md, &[]), None, "one line of the whole ADR names no commit");
        assert_eq!(built_commits("Nothing.\n", &[]), None);
    }

    #[test]
    fn the_lowest_version_is_by_number_and_only_of_version_tags() {
        assert_eq!(lowest_version(["v0.10.0", "v0.2.0", "nightly", "v0.2", "v0.1.0-rc1", "0.0.1"]), Some("v0.2.0".to_string()));
        assert_eq!(lowest_version(["nightly"]), None);
    }

    #[test]
    fn a_draft_keeps_three_columns_and_only_phases_the_adr_has() {
        let reply = "Sure.\n\n```markdown\n| Feature | What it does | Phase | State |\n|---|---|---|---|\n| A | does a | R1, R9 | completed |\n|  | nameless | | |\n```\n";
        let rows = draft_rows(reply, &["R1".to_string()]).unwrap();
        assert_eq!(rows, [FeatureRow { feature: "A".into(), what: "does a".into(), phases: vec!["R1".into()] }]);
        assert_eq!(render_section(&rows), "## Features\n\n| Feature | What it does | Phase |\n|---|---|---|\n| A | does a | R1 |\n");
        assert_eq!(draft_rows("I found no features.", &[]), None);
    }

    #[test]
    fn the_section_goes_before_consequences_or_at_the_end() {
        let section = "## Features\n\n| Feature |\n|---|\n";
        assert_eq!(
            insert_section("# T\n\n```\n## Consequences\n```\n\n## Consequences\n\n- x\n", section),
            "# T\n\n```\n## Consequences\n```\n\n## Features\n\n| Feature |\n|---|\n\n## Consequences\n\n- x\n"
        );
        assert_eq!(insert_section("# T\n\nText.\n", section), "# T\n\nText.\n\n## Features\n\n| Feature |\n|---|\n");
        assert!(has_features_section(&insert_section("# T\n", section)));
        assert!(!has_features_section("```\n## Features\n```\n"));
    }
}
