//! `hexa adr features`: every feature the ADRs name, where each stands, and
//! the release that first held it (ADR-2611250845).
//!
//! The rules are the domain's (`ports::adr_features`): a feature's state is
//! the board's reading of its ADR. This file reads the ADRs, asks git which
//! tags hold the commits a `**Built:**` line names, and prints. It writes
//! nothing, except that `extract --write` inserts a drafted section into the
//! one ADR it is given.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use colored::Colorize;
use hexa_core::ports::adr_board::analyze;
use hexa_core::ports::adr_features::{
    built_commits, draft_rows, features_of, has_features_section, insert_section, lowest_version, render_section,
    sort_features, summarize_features, AdrFeatures, Feature, FeatureState, FeatureSummary,
};
use hexa_core::ports::ledger::LedgerError;
use serde_json::{json, Value};
use tabled::Tabled;

use super::{canonical_adr_id, collect_adrs, extract_adr_id, extract_title, find_adr_dir, parse_adr_status, write_atomically};
use crate::fmt::{truncate, HexTable};

/// What `hexa adr features` was asked for.
pub struct Options {
    pub json: bool,
    pub text: bool,
    pub md: bool,
    pub check: Option<PathBuf>,
    pub state: Vec<String>,
    pub adr: Option<String>,
    pub all: bool,
    pub strict: bool,
}

/// One ADR as the checkout holds it.
struct Entry {
    id: String,
    path: PathBuf,
    content: String,
    features: AdrFeatures,
}

/// One row of the table: the feature and the release that first held it.
struct Row {
    feature: Feature,
    released: Option<String>,
}

/// How a refusal is printed.
#[derive(Clone, Copy)]
struct Refusal {
    json: bool,
    /// A fact sheet goes to stdout, so its refusal must not.
    to_stderr: bool,
}

pub async fn run(o: Options) -> anyhow::Result<()> {
    let md = o.md || o.check.is_some();
    let machine = o.json || (!o.text && !std::io::stdout().is_terminal());
    let refusal = Refusal { json: machine, to_stderr: md && o.check.is_none() };

    let mut wanted: Vec<FeatureState> = Vec::new();
    for name in &o.state {
        match FeatureState::named(&name.to_lowercase()) {
            Some(state) => wanted.push(state),
            None => return refuse(refusal, bad_state(name)),
        }
    }

    let Some(adr_dir) = find_adr_dir() else {
        return refuse(refusal, no_adrs("there is no docs/adrs/ directory here or above"));
    };
    let mut entries = match read_ledger(&adr_dir).await {
        Ok(e) if e.is_empty() => return refuse(refusal, no_adrs(&format!("{} holds no ADR", adr_dir.display()))),
        Ok(e) => e,
        Err(why) => return refuse(refusal, no_adrs(&why)),
    };
    if let Some(given) = &o.adr {
        let id = canonical_adr_id(given);
        entries.retain(|e| e.id.eq_ignore_ascii_case(&id));
        if entries.is_empty() {
            return refuse(refusal, adr_not_found(&id));
        }
    }
    let root = adr_dir.parent().and_then(Path::parent).unwrap_or(&adr_dir).to_path_buf();

    // The summary and the warnings are of everything read; the states asked
    // for choose only which rows are listed.
    let summary = summarize_features(entries.iter().map(|e| &e.features));
    let warnings = warnings_of(&entries);
    let listed = |state: FeatureState| {
        if o.all {
            true
        } else if wanted.is_empty() {
            state != FeatureState::Retired
        } else {
            wanted.contains(&state)
        }
    };
    let mut features: Vec<Feature> =
        entries.iter().flat_map(|e| e.features.features.iter().cloned()).filter(|f| listed(f.state)).collect();
    sort_features(&mut features);

    let content_of: HashMap<&str, &str> = entries.iter().map(|e| (e.id.as_str(), e.content.as_str())).collect();
    let mut tags = Tags { root, held_by: HashMap::new() };
    let rows: Vec<Row> = features
        .into_iter()
        .map(|feature| {
            let released = content_of.get(feature.adr.as_str()).and_then(|content| released_in(&feature, content, &mut tags));
            Row { feature, released }
        })
        .collect();

    if let Some(file) = &o.check {
        return check(file, &render_md(&rows, &summary), &o, machine);
    }
    if md {
        print!("{}", render_md(&rows, &summary));
    } else if machine {
        let v = json!({
            "ok": true,
            "features": rows.iter().map(row_json).collect::<Vec<_>>(),
            "summary": summary,
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print_text(&rows, &summary, &warnings);
    }

    if o.strict && !warnings.is_empty() {
        crate::commands::events::exit(1);
    }
    Ok(())
}

// ── reading ─────────────────────────────────────────────────────────────

async fn read_ledger(dir: &Path) -> Result<Vec<Entry>, String> {
    let adrs = collect_adrs(dir).await.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    Ok(adrs
        .into_iter()
        .map(|(path, content)| {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("???").to_string();
            let id = extract_adr_id(&stem);
            let title = extract_title(&path, &content);
            let features = features_of(&id, &title, parse_adr_status(&content), &content);
            Entry { id, path, content, features }
        })
        .collect())
}

/// Decision 8: each finding with the ADR it is about, and the row when it is about one.
fn warnings_of(entries: &[Entry]) -> Vec<Value> {
    let mut out = Vec::new();
    for e in entries {
        for code in &e.features.warnings {
            out.push(json!({"code": code, "adr": e.id, "feature": Value::Null, "message": warning_text(code, &e.id, None)}));
        }
        for f in &e.features.features {
            for code in &f.warnings {
                out.push(json!({"code": code, "adr": e.id, "feature": f.feature, "message": warning_text(code, &e.id, Some(&f.feature))}));
            }
        }
    }
    out
}

fn warning_text(code: &str, adr: &str, feature: Option<&str>) -> String {
    let row = feature.map(|f| format!(" `{f}`")).unwrap_or_default();
    match code {
        "unknown-phase" => format!("{adr}: the row{row} names a phase the ADR does not have, so its state is unknown"),
        "no-feature-column" => format!("{adr}: the table under `## Features` has no `Feature` column, so the ADR is read as undeclared"),
        "empty-feature" => format!("{adr}: a row under `## Features` has an empty `Feature` cell and is skipped"),
        "duplicate-feature" => format!("{adr}: the row{row} has the same name as another row of this ADR"),
        other => format!("{adr}: {other}{row}"),
    }
}

/// The tags that hold a commit, asked of git once per commit.
struct Tags {
    root: PathBuf,
    held_by: HashMap<String, Option<Vec<String>>>,
}

impl Tags {
    fn holding(&mut self, commit: &str) -> Option<&Vec<String>> {
        if !self.held_by.contains_key(commit) {
            let tags = hexa_git::tags_containing(&self.root, commit);
            self.held_by.insert(commit.to_string(), tags);
        }
        self.held_by.get(commit).and_then(Option::as_ref)
    }
}

/// Decision 6: the lowest version tag that holds every commit named on the
/// `**Built` lines that built a completed row. `None` when the record or the
/// repository cannot say.
fn released_in(feature: &Feature, content: &str, tags: &mut Tags) -> Option<String> {
    if feature.state != FeatureState::Completed {
        return None;
    }
    let commits = built_commits(content, &feature.phases)?;
    let mut common: Option<Vec<String>> = None;
    for commit in &commits {
        let held = tags.holding(commit)?;
        common = Some(match common {
            None => held.clone(),
            Some(so_far) => so_far.into_iter().filter(|t| held.contains(t)).collect(),
        });
    }
    lowest_version(common?.iter().map(String::as_str))
}

// ── printing ────────────────────────────────────────────────────────────

fn row_json(r: &Row) -> Value {
    let f = &r.feature;
    json!({
        "feature": f.feature,
        "what": f.what,
        "state": f.state.name(),
        "adr": f.adr,
        "adr_title": f.adr_title,
        "phases": f.phases,
        "built": {"done": f.built.done, "total": f.built.total},
        "declared": f.declared,
        "released": r.released,
        "warnings": f.warnings,
    })
}

/// `2 completed, 1 accepted`: the states that have a row, in the table's order.
fn counts(summary: &FeatureSummary) -> String {
    FeatureState::ALL
        .iter()
        .filter_map(|s| summary.by_state.get(s.name()).filter(|n| **n > 0).map(|n| format!("{n} {}", s.name())))
        .collect::<Vec<_>>()
        .join(", ")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[derive(Tabled)]
struct TextRow {
    #[tabled(rename = "Feature")]
    feature: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "Built")]
    built: String,
    #[tabled(rename = "Released")]
    released: String,
    #[tabled(rename = "ADR")]
    adr: String,
    #[tabled(rename = "What it does")]
    what: String,
}

fn print_text(rows: &[Row], summary: &FeatureSummary, warnings: &[Value]) {
    println!("{} Features", "\u{2b21}".cyan());
    println!();
    println!("  {} in {}: {}", plural(summary.features, "feature", "features"), plural(summary.adrs, "ADR", "ADRs"), counts(summary));
    if summary.undeclared > 0 {
        println!(
            "  {}",
            format!("{} with no `## Features` section: the title stands in (marked *)", plural(summary.undeclared, "ADR", "ADRs")).yellow()
        );
    }
    println!();
    let table: Vec<TextRow> = rows
        .iter()
        .map(|r| TextRow {
            feature: format!("{}{}", truncate(&r.feature.feature, 40), if r.feature.declared { "" } else { " *" }),
            state: r.feature.state.name().to_string(),
            built: format!("{}/{}", r.feature.built.done, r.feature.built.total),
            released: r.released.clone().unwrap_or_else(|| "\u{2014}".into()),
            adr: r.feature.adr.clone(),
            what: truncate(&r.feature.what, 56),
        })
        .collect();
    println!("{}", HexTable::render(&table));
    if !warnings.is_empty() {
        println!();
        for w in warnings {
            println!("  {} {}", "\u{26a0}".yellow(), w["message"].as_str().unwrap_or_default());
        }
    }
}

fn md_cell(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").replace('|', "\\|")
}

/// The fact sheet. It carries no date and no commit of its own, so it changes
/// only when a feature does (decision 7).
fn render_md(rows: &[Row], summary: &FeatureSummary) -> String {
    let mut s = String::from("# Features\n\n");
    s.push_str(&format!(
        "{} in {}: {}.",
        plural(summary.features, "feature", "features"),
        plural(summary.adrs, "ADR", "ADRs"),
        if summary.features == 0 { "none".to_string() } else { counts(summary) }
    ));
    if rows.len() != summary.features {
        s.push_str(&format!(" This sheet lists {} of them.", rows.len()));
    }
    s.push('\n');
    if summary.undeclared > 0 {
        s.push_str(&format!(
            "\n{} no `## Features` section: the title stands in for a feature, marked *(ADR title)*.\n",
            if summary.undeclared == 1 { "1 ADR has".to_string() } else { format!("{} ADRs have", summary.undeclared) }
        ));
    }
    s.push_str("\nWritten by `hexa adr features --md` from the ADRs in `docs/adrs/`. Do not edit it by hand.\n");

    for state in FeatureState::ALL {
        let of_state: Vec<&Row> = rows.iter().filter(|r| r.feature.state == state).collect();
        if of_state.is_empty() {
            continue;
        }
        let name = state.name();
        s.push_str(&format!("\n## {}{}\n\n", name[..1].to_uppercase(), &name[1..]));
        s.push_str("| Feature | What it does | ADR | Released |\n|---|---|---|---|\n");
        for r in of_state {
            let f = &r.feature;
            let released = match (&r.released, state) {
                (Some(version), _) => version.clone(),
                (None, FeatureState::Completed) => "not released".to_string(),
                (None, _) => String::new(),
            };
            s.push_str(&format!(
                "| {}{} | {} | {} | {} |\n",
                md_cell(&f.feature),
                if f.declared { "" } else { " *(ADR title)*" },
                md_cell(&f.what),
                f.adr,
                released
            ));
        }
    }
    s
}

/// Decision 7: `file` must be what `--md` prints now. A check writes nothing.
fn check(file: &Path, now: &str, o: &Options, machine: bool) -> anyhow::Result<()> {
    let refusal = Refusal { json: machine, to_stderr: false };
    let shown = file.display().to_string();
    let mut command = String::from("hexa adr features --md");
    if o.all {
        command.push_str(" --all");
    }
    for state in &o.state {
        command.push_str(&format!(" --state {state}"));
    }
    if let Some(adr) = &o.adr {
        command.push_str(&format!(" --adr {adr}"));
    }
    command.push_str(&format!(" > {shown}"));

    let held = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) => {
            return refuse(
                refusal,
                LedgerError::new(
                    "fact-sheet-missing",
                    format!("the fact sheet {shown} cannot be read ({e}), so it is not current"),
                    format!("Write it with `{command}`, and commit it."),
                    Some("check"),
                    false,
                ),
            )
        }
    };
    if held != now {
        return refuse(
            refusal,
            LedgerError::new(
                "fact-sheet-stale",
                format!("the fact sheet {shown} is not what the ADRs say now"),
                format!("Rewrite it with `{command}`, and commit it with the change that moved a feature."),
                Some("check"),
                false,
            ),
        );
    }
    if machine {
        println!("{}", serde_json::to_string_pretty(&json!({"ok": true, "file": shown, "current": true}))?);
    } else {
        println!("{} {shown} is current", "\u{2713}".green());
    }
    Ok(())
}

// ── extraction (decision 9) ─────────────────────────────────────────────

const EXTRACT_SYSTEM: &str = "You read one architecture decision record and list the features it gives a user of the product.\n\
A feature is something a user can do or see, named in a few words as a person would say it.\n\
A decision about process, tooling, naming or CI gives no feature.\n\
Reply with one Markdown table and nothing else, with exactly these columns:\n\n\
| Feature | What it does | Phase |\n|---|---|---|\n\n\
`What it does` is one sentence. `Phase` is the id of the phase of this record that builds the feature, \
or empty when the record has no phases or the feature needs all of it.\n\
Do not say whether anything is built, released or done.\n\
If the decision gives no feature, reply with the header row and the separator row alone.";

/// `hexa adr features extract <ADR>`: draft the `## Features` section of an
/// ADR that has none. Without `--write` it prints the section and writes
/// nothing.
pub async fn extract(adr: String, write: bool, as_json: bool) -> anyhow::Result<()> {
    let refusal = Refusal { json: as_json || !std::io::stdout().is_terminal(), to_stderr: false };
    let Some(adr_dir) = find_adr_dir() else {
        return refuse(refusal, no_adrs("there is no docs/adrs/ directory here or above"));
    };
    let entries = match read_ledger(&adr_dir).await {
        Ok(e) => e,
        Err(why) => return refuse(refusal, no_adrs(&why)),
    };
    let id = canonical_adr_id(&adr);
    let Some(entry) = entries.into_iter().find(|e| e.id.eq_ignore_ascii_case(&id)) else {
        return refuse(refusal, adr_not_found(&id));
    };
    let file = entry.path.display().to_string();
    if has_features_section(&entry.content) {
        return refuse(
            refusal,
            LedgerError::new(
                "features-declared",
                format!("{} already has a `## Features` section; extract drafts one only where there is none", entry.id),
                format!("Edit the table in {file} by hand, and read it back with `hexa adr features --adr {}`.", entry.id),
                Some("adr"),
                false,
            ),
        );
    }

    let phases: Vec<String> =
        analyze(parse_adr_status(&entry.content), &entry.content).units.phases.into_iter().map(|p| p.id).collect();
    let Some(model) = hexa_infer::tier_model("t1") else {
        return refuse(
            refusal,
            LedgerError::new(
                "no-t1-model",
                "no T1 model is configured, and extraction asks the T1 tier".to_string(),
                "Set one with `hexa config model`, or write the `## Features` table by hand: `hexa adr schema` prints its shape.".to_string(),
                None,
                false,
            ),
        );
    };
    let user = format!(
        "{}\n\n{}",
        if phases.is_empty() { "This record has no phases: leave Phase empty.".to_string() } else { format!("The phases of this record: {}.", phases.join(", ")) },
        entry.content
    );
    let reply = match hexa_infer::complete_text(&model, EXTRACT_SYSTEM, &user, 1500).await {
        Ok(r) => r,
        Err(why) => {
            return refuse(
                refusal,
                LedgerError::new(
                    "extract-failed",
                    format!("the T1 tier did not answer: {why}"),
                    "Check the tier with `hexa config model`, then run it again.".to_string(),
                    None,
                    true,
                ),
            )
        }
    };
    let Some(rows) = draft_rows(&reply, &phases) else {
        return refuse(
            refusal,
            LedgerError::new(
                "extract-unreadable",
                "the reply holds no table with a `Feature` column, so there is nothing to draft".to_string(),
                "Run it again, or write the `## Features` table by hand: `hexa adr schema` prints its shape.".to_string(),
                None,
                true,
            ),
        );
    };
    if write && rows.is_empty() {
        return refuse(
            refusal,
            LedgerError::new(
                "extract-empty",
                format!("the draft names no feature for {}, and an empty section is a statement a person makes", entry.id),
                format!("If the decision gives no feature, add `## Features` with the table's header alone to {file} by hand."),
                None,
                false,
            ),
        );
    }

    let section = render_section(&rows);
    if write {
        write_atomically(&entry.path, &insert_section(&entry.content, &section))?;
    }
    if as_json {
        let v = json!({"ok": true, "adr": entry.id, "file": file, "written": write, "section": section, "rows": rows});
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print!("{section}");
        if write {
            eprintln!("{} wrote the section to {file}: it is a draft, read the rows", "\u{2b21}".cyan());
        } else {
            eprintln!("{} a draft, nothing written: `hexa adr features extract {} --write` inserts it into {file}", "\u{2b21}".cyan(), entry.id);
        }
    }
    Ok(())
}

// ── refusal ─────────────────────────────────────────────────────────────

fn no_adrs(why: &str) -> LedgerError {
    LedgerError::new(
        "no_adrs",
        format!("no ADRs to read: {why}"),
        "Run from a repository that has docs/adrs/ADR-*.md.".into(),
        None,
        false,
    )
}

fn bad_state(given: &str) -> LedgerError {
    LedgerError::new(
        "bad-state",
        format!("`{given}` is not a state a feature has"),
        "Give --state one of: proposed, accepted, completed, retired. It may be given more than once.".into(),
        Some("state"),
        false,
    )
}

fn adr_not_found(id: &str) -> LedgerError {
    LedgerError::new(
        "adr-not-found",
        format!("no ADR with the id {id} under docs/adrs/ (ids match exactly)"),
        "`hexa adr list` prints every id.".into(),
        Some("adr"),
        false,
    )
}

fn refuse(how: Refusal, e: LedgerError) -> anyhow::Result<()> {
    if how.json {
        let mut v = serde_json::to_value(&e)?;
        v["ok"] = json!(false);
        let text = serde_json::to_string_pretty(&v)?;
        if how.to_stderr {
            eprintln!("{text}");
        } else {
            println!("{text}");
        }
    } else {
        eprintln!("\u{2717} {}\n  hint: {}\n  code: {}", e.message, e.hint, e.code);
    }
    crate::commands::events::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(id: &str, title: &str, status: &str, content: &str) -> Vec<Row> {
        features_of(id, title, status, content).features.into_iter().map(|feature| Row { feature, released: None }).collect()
    }

    #[test]
    fn the_fact_sheet_escapes_a_pipe_and_says_what_is_not_released() {
        let content = "**Built:** F1 on 2026-11-14.\n\n## Features\n\n| Feature | What it does |\n|---|---|\n| A \\| B | does a |\n";
        let adr = features_of("ADR-1", "T", "accepted", content);
        let summary = summarize_features([&adr]);
        let md = render_md(&rows_of("ADR-1", "T", "accepted", content), &summary);
        assert!(md.contains("1 feature in 1 ADR: 1 completed.\n"), "{md}");
        assert!(md.contains("\n## Completed\n\n| Feature | What it does | ADR | Released |\n|---|---|---|---|\n| A \\| B | does a | ADR-1 | not released |\n"), "{md}");
        assert!(!md.contains("This sheet lists"), "{md}");
    }

    #[test]
    fn a_sheet_that_lists_fewer_rows_than_it_counts_says_so() {
        let content = "## Features\n\n| Feature |\n|---|\n| A |\n";
        let adr = features_of("ADR-1", "T", "superseded", content);
        let md = render_md(&[], &summarize_features([&adr]));
        assert!(md.contains("1 feature in 1 ADR: 1 retired. This sheet lists 0 of them.\n"), "{md}");
    }
}
