//! What an ADR says it has built, read from its text (ADR-2611151030).
//!
//! Progress is evidence, never status: `Accepted` is a decision, not a build.
//! A unit of work is a phase, or the whole ADR when it has none. Everything
//! here is a pure function of the ADR's text, so the rules can be tested
//! without a filesystem and cannot disagree with the adapter that prints them.

use serde::Serialize;
use std::collections::BTreeMap;

/// One phase of an ADR, from a `### R1. Title` heading under `## Phases`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Phase {
    pub id: String,
    pub title: String,
    pub built: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Units {
    pub done: usize,
    pub total: usize,
    pub phases: Vec<Phase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Every unit built.
    Built,
    /// At least one unit built, not all.
    Building,
    /// Accepted, with no unit built: decided, no evidence of a build.
    Accepted,
    /// Proposed, with no unit built.
    Proposed,
    /// Superseded, rejected, deprecated or abandoned.
    Retired,
    /// The status line could not be read.
    Unknown,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Built => "built",
            State::Building => "building",
            State::Accepted => "accepted",
            State::Proposed => "proposed",
            State::Retired => "retired",
            State::Unknown => "unknown",
        }
    }

    /// Counted in the summary: work that is neither retired nor unreadable.
    pub fn is_counted(self) -> bool {
        !matches!(self, State::Retired | State::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Analysis {
    /// Lower-case status word as the adapter's status parser reports it.
    pub status: String,
    pub state: State,
    pub units: Units,
    /// Findings where the record disagrees with the evidence.
    pub drift: Vec<String>,
}

const DRIFT_STATUS_BEHIND_EVIDENCE: &str = "status-behind-evidence";

/// Read one ADR. `status` is the normalised status word (`proposed`,
/// `accepted`, `completed`, ...) the adapter already parsed; `content` is the
/// whole file.
pub fn analyze(status: &str, content: &str) -> Analysis {
    let lines = unfenced_lines(content);
    let header_ids = header_built_ids(&lines);
    let has_header_built = lines.iter().any(|l| l.trim_start().starts_with("**Built:**"));
    let completed = status == "completed";

    let mut phases = phases_of(&lines, &header_ids);
    if completed {
        for p in &mut phases {
            p.built = true;
        }
    }

    let units = if phases.is_empty() {
        let built = completed || has_header_built;
        Units { done: usize::from(built), total: 1, phases }
    } else {
        let done = phases.iter().filter(|p| p.built).count();
        Units { done, total: phases.len(), phases }
    };

    let state = match status {
        "superseded" | "rejected" | "deprecated" | "abandoned" => State::Retired,
        "proposed" | "accepted" | "completed" => {
            if units.done == units.total {
                State::Built
            } else if units.done > 0 {
                State::Building
            } else if status == "accepted" {
                State::Accepted
            } else {
                State::Proposed
            }
        }
        _ => State::Unknown,
    };

    let mut drift = Vec::new();
    if status == "proposed" && matches!(state, State::Building | State::Built) {
        drift.push(DRIFT_STATUS_BEHIND_EVIDENCE.to_string());
    }

    Analysis { status: status.to_string(), state, units, drift }
}

/// The lines outside a fenced code block. A `###` or `**Built` inside a fence
/// is an example, not structure.
fn unfenced_lines(content: &str) -> Vec<&str> {
    let mut fenced = false;
    let mut out = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push(line);
        }
    }
    out
}

/// Whole alphanumeric tokens on the `**Built:**` header line. `P2` is not
/// `P2a`, and a phase is named only by its exact id.
fn header_built_ids(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| l.trim_start().strip_prefix("**Built:**"))
        .flat_map(|rest| {
            rest.split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `R1`, `P2a`, `F1`: one to three letters, digits, then at most one
/// lower-case letter.
fn is_phase_id(s: &str) -> bool {
    let letters = s.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    if !(1..=3).contains(&letters) {
        return false;
    }
    let rest = &s[letters..];
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let tail = &rest[digits..];
    tail.is_empty() || (tail.len() == 1 && tail.chars().all(|c| c.is_ascii_lowercase()))
}

/// `### R1. Title` -> ("R1", "Title"). Anything else is not a phase heading.
fn phase_heading(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("### ")?;
    let (id, title) = rest.split_once('.')?;
    is_phase_id(id).then(|| (id.to_string(), title.trim().to_string()))
}

fn is_h2(line: &str) -> bool {
    line.starts_with("## ")
}

fn phases_of(lines: &[&str], header_ids: &[String]) -> Vec<Phase> {
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_end().eq_ignore_ascii_case("## phases"))
    else {
        return Vec::new();
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| is_h2(l))
        .map_or(lines.len(), |i| start + 1 + i);
    let section = &lines[start + 1..end];

    let mut phases: Vec<Phase> = Vec::new();
    // Whether the heading currently open is a phase; any other `###` closes it.
    let mut open = false;
    for line in section {
        if line.starts_with("### ") {
            match phase_heading(line) {
                Some((id, title)) => {
                    let built = header_ids.contains(&id);
                    phases.push(Phase { id, title, built });
                    open = true;
                }
                None => open = false,
            }
        } else if open && line.trim_start().starts_with("**Built") {
            if let Some(p) = phases.last_mut() {
                p.built = true;
            }
        }
    }
    phases
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub adrs: usize,
    pub units: usize,
    pub built: usize,
    /// `floor(100 * built / units)`, so 100 means done.
    pub percent: usize,
    pub by_state: BTreeMap<String, usize>,
    pub drift: usize,
}

/// Units over ADRs that are not retired or unreadable; every ADR counts in
/// `by_state`.
pub fn summarize<'a>(all: impl IntoIterator<Item = &'a Analysis>) -> Summary {
    let mut s = Summary {
        adrs: 0,
        units: 0,
        built: 0,
        percent: 0,
        by_state: BTreeMap::new(),
        drift: 0,
    };
    for a in all {
        s.adrs += 1;
        *s.by_state.entry(a.state.name().to_string()).or_insert(0) += 1;
        if !a.drift.is_empty() {
            s.drift += 1;
        }
        if a.state.is_counted() {
            s.units += a.units.total;
            s.built += a.units.done;
        }
    }
    s.percent = (s.built * 100).checked_div(s.units).unwrap_or(0);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHASED: &str = "# ADR\n\n**Status:** Proposed\n\n## Phases\n\n### R1. First\n**Built 2026-11-14.**\n\n### Decision gate (R1)\n```sh\ntrue\n```\n\n### R2. Second\nNot yet.\n\n## Gate\n\n`true`\n";

    #[test]
    fn a_decision_gate_heading_is_not_a_phase() {
        let a = analyze("proposed", PHASED);
        let ids: Vec<_> = a.units.phases.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["R1", "R2"]);
        assert_eq!((a.units.done, a.units.total), (1, 2));
        assert_eq!(a.state, State::Building);
    }

    #[test]
    fn a_built_line_in_a_decision_gate_section_is_not_a_phases_evidence() {
        let md = "## Phases\n\n### R1. One\nText.\n\n### Decision gate (R1)\n**Built 2026-11-14.**\n\n### R2. Two\nText.\n";
        let a = analyze("accepted", md);
        assert_eq!(a.units.done, 0, "the **Built line belongs to the gate heading, not R1");
    }

    #[test]
    fn a_fenced_heading_or_built_line_is_an_example() {
        let md = "## Phases\n\n### R1. One\n```\n**Built 2026-11-14.**\n### R9. Fake\n```\n";
        let a = analyze("accepted", md);
        assert_eq!((a.units.done, a.units.total), (0, 1));
    }

    #[test]
    fn the_header_names_phases_as_whole_tokens() {
        let md = "**Built:** P1 and P2a on 2026-11-14.\n\n## Phases\n\n### P1. A\n### P2. B\n### P2a. C\n";
        let a = analyze("accepted", md);
        let built: Vec<_> = a.units.phases.iter().filter(|p| p.built).map(|p| p.id.as_str()).collect();
        assert_eq!(built, ["P1", "P2a"]);
    }

    #[test]
    fn a_phaseless_adr_is_one_unit_built_by_its_header_line() {
        assert_eq!(analyze("accepted", "**Built:** F1 on 2026-11-14.\n").state, State::Built);
        assert_eq!(analyze("accepted", "Nothing.\n").state, State::Accepted);
        assert_eq!(analyze("proposed", "Nothing.\n").state, State::Proposed);
    }

    #[test]
    fn accepted_alone_is_not_progress() {
        let a = analyze("accepted", "## Phases\n\n### R1. One\n### R2. Two\n");
        assert_eq!((a.units.done, a.units.total, a.state), (0, 2, State::Accepted));
    }

    #[test]
    fn completed_has_every_unit_built() {
        let a = analyze("completed", "## Phases\n\n### R1. One\n### R2. Two\n");
        assert_eq!((a.units.done, a.units.total, a.state), (2, 2, State::Built));
    }

    #[test]
    fn a_proposed_adr_with_built_evidence_drifts() {
        assert_eq!(analyze("proposed", PHASED).drift, [DRIFT_STATUS_BEHIND_EVIDENCE]);
        assert!(analyze("accepted", PHASED).drift.is_empty());
        assert!(analyze("proposed", "Nothing.\n").drift.is_empty());
    }

    #[test]
    fn retired_and_unreadable_statuses_have_their_own_states() {
        assert_eq!(analyze("superseded", "").state, State::Retired);
        assert_eq!(analyze("rejected", "").state, State::Retired);
        assert_eq!(analyze("unknown", "").state, State::Unknown);
    }

    #[test]
    fn phase_ids_are_letters_digits_and_at_most_one_lower_letter() {
        for ok in ["R1", "P2a", "F1", "ABC12"] {
            assert!(is_phase_id(ok), "{ok}");
        }
        for bad in ["", "1", "R", "R1A", "R1ab", "ABCD1", "Decision gate (R1)", "R 1"] {
            assert!(!is_phase_id(bad), "{bad}");
        }
    }

    #[test]
    fn the_summary_rounds_down_and_skips_retired_and_unknown() {
        let a = [
            analyze("proposed", PHASED),                                   // 1 of 2
            analyze("accepted", "**Built:** x\n"),                         // 1 of 1
            analyze("proposed", "Nothing\n"),                              // 0 of 1
            analyze("superseded", "**Built:** x\n"),                       // retired
            analyze("unknown", "**Built:** x\n"),                          // unknown
        ];
        let s = summarize(&a);
        assert_eq!((s.adrs, s.units, s.built, s.percent), (5, 4, 2, 50));
        assert_eq!(s.by_state["retired"], 1);
        assert_eq!(s.by_state["unknown"], 1);
        assert_eq!(s.drift, 1);
    }

    #[test]
    fn a_hundred_percent_means_every_unit_is_built() {
        let a = [analyze("accepted", "**Built:** x\n"), analyze("accepted", "**Built:** y\n")];
        assert_eq!(summarize(&a).percent, 100);
        let b = [analyze("accepted", "**Built:** x\n"), analyze("accepted", "Nothing\n"), analyze("accepted", "Nothing\n")];
        assert_eq!(summarize(&b).percent, 33, "one of three is 33, not 34");
    }

    #[test]
    fn nothing_counted_is_zero_not_a_division_by_zero() {
        assert_eq!(summarize(&[]).percent, 0);
    }
}
