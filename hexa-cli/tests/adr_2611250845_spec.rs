//! `hexa adr features`: the table of every feature the decisions name, the
//! release each shipped in, the fact-sheet check, and extraction (decisions 5
//! to 9). The domain rules (decisions 1 to 4 and 8) are
//! `hexa-core/tests/adr_2611250845_spec.rs`.
//!
//! Written from the scenarios of `docs/specs/<this ADR>.json`, before the
//! code, one test per scenario. Every case runs the built binary in a temp
//! project. No network: where a scenario needs a model, a completion command
//! stands in for one.
//!
//! Fixture ids are assembled at run time: the citation checker scans source,
//! and a literal id here would claim a decision exists.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

// ── harness ─────────────────────────────────────────────────────────────

fn hexa_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hexa"))
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    /// Everything it printed, for messages and for text an error may put on either stream.
    fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    /// Stdout as one JSON value.
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|e| panic!("stdout is not one JSON value ({e}); exit {}:\n{}", self.code, self.all()))
    }
}

fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
    let home = dir.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(hexa_bin());
    cmd.args(args)
        .current_dir(dir)
        .env("HOME", &home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env_remove("HEXA_SESSION_ID")
        .env_remove("HEXA_ON_RUNNER")
        .env_remove("HEXA_PROJECT_ROOT")
        .env_remove("HEXA_COMPLETION_COMMAND")
        .env_remove("HEXA_AGENT")
        .env_remove("CLAUDE_PROJECT_DIR")
        .env("NO_COLOR", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o = cmd.output().expect("run hexa");
    Run {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn run(dir: &Path, args: &[&str]) -> Run {
    run_env(dir, args, &[])
}

/// `hexa adr features <args> --json`, which must succeed: its JSON.
fn features_json(dir: &Path, args: &[&str]) -> Value {
    let mut all = vec!["adr", "features"];
    all.extend_from_slice(args);
    all.push("--json");
    let r = run(dir, &all);
    assert_eq!(r.code, 0, "hexa {all:?}:\n{}", r.all());
    let v = r.json();
    assert_eq!(v["ok"], true, "hexa {all:?}:\n{}", r.all());
    v
}

fn rows(v: &Value) -> &Vec<Value> {
    v["features"].as_array().unwrap_or_else(|| panic!("no `features` array in {v}"))
}

fn row<'a>(v: &'a Value, name: &str) -> &'a Value {
    rows(v)
        .iter()
        .find(|r| r["feature"] == name)
        .unwrap_or_else(|| panic!("no row named {name:?} in {v}"))
}

fn row_names(v: &Value) -> Vec<String> {
    rows(v).iter().map(|r| r["feature"].as_str().unwrap_or("?").to_string()).collect()
}

/// The object that carries an error's `code`: `error`, or the reply itself.
fn error_of(v: &Value) -> &Value {
    match v.get("error") {
        Some(e) if e.is_object() => e,
        _ => v,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git_init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, ".gitignore", ".test-home/\n");
}

/// Commit everything; the new commit's short id.
fn commit_all(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "--short", "HEAD"])
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

// ── fixtures ────────────────────────────────────────────────────────────

/// A ten-digit id that no real decision has.
fn id(n: u32) -> String {
    format!("ADR-{}{:04}", "260101", n)
}

fn table(rows: &[&str]) -> String {
    let mut t = String::from("| Feature | What it does | Phase |\n|---|---|---|\n");
    for r in rows {
        t.push_str(r);
        t.push('\n');
    }
    t
}

struct Adr<'a> {
    n: u32,
    title: &'a str,
    status: &'a str,
    built: Option<String>,
    phases: &'a [&'a str],
    /// The rows of its Features table; `None` for no section.
    features: Option<&'a [&'a str]>,
}

impl Adr<'_> {
    fn text(&self) -> String {
        let mut s = format!("# {}: {}\n\n**Status:** {}\n", id(self.n), self.title, self.status);
        if let Some(b) = &self.built {
            s.push_str(b);
            s.push('\n');
        }
        s.push_str("**Date:** 2026-11-14\n\n## Context\n\nWhy.\n\n## Decision\n\n1. Do it.\n\n");
        if !self.phases.is_empty() {
            s.push_str("## Phases\n\n");
            for p in self.phases {
                s.push_str(&format!("### {p}. Phase {p}\n\nWork.\n\n"));
            }
        }
        if let Some(rows) = self.features {
            s.push_str(&format!("## Features\n\n{}\n", table(rows)));
        }
        s.push_str("## Consequences\n\n- One.\n");
        s
    }

    fn file(&self) -> String {
        format!("docs/adrs/{}-{}.md", id(self.n), self.title.to_lowercase().replace(' ', "-"))
    }

    fn put(&self, root: &Path) {
        write(root, &self.file(), &self.text());
    }
}

const BUILT: &str = "**Built:** F1 on 2026-11-14.";

const N_BUILT: u32 = 1;
const N_ACCEPTED: u32 = 2;
const N_PROPOSED: u32 = 3;
const N_RETIRED: u32 = 4;

fn accepted_adr() -> Adr<'static> {
    Adr {
        n: N_ACCEPTED,
        title: "Power",
        status: "Accepted",
        built: None,
        phases: &[],
        features: Some(&["| Power budget | Adds up what a rack draws | |"]),
    }
}

/// The project of S14: a built ADR with two declared features, an accepted
/// ADR with one, a proposed ADR with no Features section, and a superseded ADR
/// with one declared feature.
fn project_s14() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    Adr {
        n: N_BUILT,
        title: "Cabling",
        status: "Accepted",
        built: Some(BUILT.into()),
        phases: &[],
        features: Some(&["| Cable trace | Follows a cable end to end | |", "| Rack view | Shows what a rack holds | |"]),
    }
    .put(root);
    accepted_adr().put(root);
    Adr { n: N_PROPOSED, title: "Tenant quotas", status: "Proposed", built: None, phases: &[], features: None }.put(root);
    Adr {
        n: N_RETIRED,
        title: "Maps",
        status: "Superseded",
        built: None,
        phases: &[],
        features: Some(&["| Old map | Drew the floor by hand | |"]),
    }
    .put(root);
    tmp
}

// ── decision 5: the table ───────────────────────────────────────────────

#[test]
fn s14_json_prints_every_row_and_a_summary_by_state() {
    let tmp = project_s14();
    let v = features_json(tmp.path(), &[]);

    assert_eq!(rows(&v).len(), 4, "four rows, the retired one left out: {:?}", row_names(&v));
    assert!(!row_names(&v).contains(&"Old map".to_string()));
    for r in rows(&v) {
        for key in ["feature", "what", "state", "adr", "adr_title", "phases", "built", "declared", "released"] {
            assert!(r.get(key).is_some(), "a row has no `{key}`: {r}");
        }
        assert!(r["built"]["done"].is_u64() && r["built"]["total"].is_u64(), "built is done and total: {r}");
        assert!(r["phases"].is_array(), "{r}");
        assert!(r["released"].is_null(), "nothing here is in a release: {r}");
    }

    let trace = row(&v, "Cable trace");
    assert_eq!(trace["state"], "completed");
    assert_eq!(trace["what"], "Follows a cable end to end");
    assert_eq!(trace["adr"], id(N_BUILT));
    assert!(trace["adr_title"].as_str().unwrap().contains("Cabling"), "{trace}");
    assert_eq!(trace["declared"], true);
    assert_eq!((trace["built"]["done"].as_u64(), trace["built"]["total"].as_u64()), (Some(1), Some(1)));
    assert_eq!(row(&v, "Rack view")["state"], "completed");
    assert_eq!(row(&v, "Power budget")["state"], "accepted");

    let stand_in = row(&v, "Tenant quotas");
    assert_eq!(stand_in["state"], "proposed");
    assert_eq!(stand_in["declared"], false, "its title stands in for a feature");
    assert_eq!(stand_in["adr"], id(N_PROPOSED));

    let by = &v["summary"]["by_state"];
    assert_eq!(by["completed"], 2, "{}", v["summary"]);
    assert_eq!(by["accepted"], 1, "{}", v["summary"]);
    assert_eq!(by["proposed"], 1, "{}", v["summary"]);
    assert_eq!(by["retired"], 1, "the summary counts the retired row the list leaves out: {}", v["summary"]);
    assert_eq!(v["summary"]["undeclared"], 1, "{}", v["summary"]);
}

#[test]
fn s15_rows_are_ordered_by_state_then_name_without_case_then_adr_id() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let beta: &[&str] = &["| beta | Second | |"];
    // Written out of order, and named so that the file order is not the answer.
    Adr { n: 4, title: "D", status: "Proposed", built: None, phases: &[], features: Some(&["| Gamma | Third | |"]) }.put(root);
    Adr { n: 2, title: "B", status: "Accepted", built: None, phases: &[], features: Some(beta) }.put(root);
    Adr {
        n: 3,
        title: "C",
        status: "Accepted",
        built: Some(BUILT.into()),
        phases: &[],
        features: Some(&["| zeta | Last letter | |", "| Alpha | First letter | |"]),
    }
    .put(root);
    Adr { n: 1, title: "A", status: "Accepted", built: None, phases: &[], features: Some(beta) }.put(root);

    let v = features_json(root, &[]);
    let order: Vec<(String, String)> = rows(&v)
        .iter()
        .map(|r| (r["feature"].as_str().unwrap().to_string(), r["adr"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        order,
        [
            ("Alpha".to_string(), id(3)),
            ("zeta".to_string(), id(3)),
            ("beta".to_string(), id(1)),
            ("beta".to_string(), id(2)),
            ("Gamma".to_string(), id(4)),
        ]
    );
}

#[test]
fn s16_state_all_and_adr_choose_the_rows() {
    let tmp = project_s14();
    let root = tmp.path();

    let accepted = features_json(root, &["--state", "accepted"]);
    assert_eq!(row_names(&accepted), ["Power budget"]);

    let two = features_json(root, &["--state", "accepted", "--state", "proposed"]);
    assert_eq!(row_names(&two), ["Power budget", "Tenant quotas"]);

    let retired = features_json(root, &["--state", "retired"]);
    assert_eq!(row_names(&retired), ["Old map"]);
    assert_eq!(rows(&retired)[0]["state"], "retired");

    let all = features_json(root, &["--all"]);
    assert_eq!(rows(&all).len(), 5, "{:?}", row_names(&all));
    assert!(row_names(&all).contains(&"Old map".to_string()));

    let one = features_json(root, &["--adr", &id(N_ACCEPTED)]);
    assert_eq!(row_names(&one), ["Power budget"]);
    assert_eq!(rows(&one)[0]["adr"], id(N_ACCEPTED));
}

#[test]
fn s17_a_state_or_an_adr_that_does_not_exist_is_refused_by_name() {
    let tmp = project_s14();
    let root = tmp.path();

    let bad_state = run(root, &["adr", "features", "--state", "shipped", "--json"]);
    assert_ne!(bad_state.code, 0, "{}", bad_state.all());
    let v = bad_state.json();
    assert_eq!(v["ok"], false, "{v}");
    let e = error_of(&v);
    assert_eq!(e["code"], "bad-state", "{v}");
    assert_eq!(e["field"], "state", "{v}");
    let hint = e["hint"].as_str().unwrap_or_else(|| panic!("no hint: {v}"));
    for state in ["proposed", "accepted", "completed", "retired"] {
        assert!(hint.contains(state), "the hint names the valid states, {state} is missing: {hint}");
    }

    let missing = id(0);
    let no_adr = run(root, &["adr", "features", "--adr", &missing, "--json"]);
    assert_ne!(no_adr.code, 0, "{}", no_adr.all());
    let v = no_adr.json();
    assert_eq!(v["ok"], false, "{v}");
    let e = error_of(&v);
    assert_eq!(e["code"], "adr-not-found", "{v}");
    assert_eq!(e["field"], "adr", "{v}");
    let hint = e["hint"].as_str().unwrap_or_else(|| panic!("no hint: {v}"));
    assert!(hint.contains("hexa adr list"), "the hint says where the ids are: {hint}");
}

/// The cells of a Markdown table row.
fn cells(line: &str) -> Vec<String> {
    line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect()
}

#[test]
fn s18_md_prints_a_markdown_fact_sheet_grouped_by_state() {
    let tmp = project_s14();
    let r = run(tmp.path(), &["adr", "features", "--md"]);
    assert_eq!(r.code, 0, "{}", r.all());
    let md = &r.stdout;
    let lines: Vec<&str> = md.lines().collect();

    // A heading per state that has rows, in order; none for retired, which is left out.
    let state_headings: Vec<String> = lines
        .iter()
        .filter(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_lowercase())
        .filter(|h| ["completed", "accepted", "proposed", "retired", "unknown"].iter().any(|s| h.starts_with(s)))
        .map(|h| h.split_whitespace().next().unwrap().to_string())
        .collect();
    assert_eq!(state_headings, ["completed", "accepted", "proposed"], "{md}");

    // Each is followed by a table with these columns, before the next heading.
    let wanted = ["Feature", "What it does", "ADR", "Released"];
    let heading_at: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let h = l.trim_start_matches('#').trim().to_lowercase();
            l.starts_with('#') && ["completed", "accepted", "proposed"].iter().any(|s| h.starts_with(s))
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(heading_at.len(), 3, "{md}");
    for (k, &at) in heading_at.iter().enumerate() {
        let end = heading_at.get(k + 1).copied().unwrap_or(lines.len());
        let header = lines[at + 1..end]
            .iter()
            .find(|l| l.trim_start().starts_with('|'))
            .unwrap_or_else(|| panic!("no table under {:?}:\n{md}", lines[at]));
        assert_eq!(cells(header), wanted, "the table under {:?}", lines[at]);
    }

    // The rows sit under their states.
    let under = |k: usize, name: &str| {
        let end = heading_at.get(k + 1).copied().unwrap_or(lines.len());
        lines[heading_at[k]..end].iter().any(|l| l.starts_with('|') && cells(l)[0].contains(name))
    };
    assert!(under(0, "Cable trace") && under(0, "Rack view"), "{md}");
    assert!(under(1, "Power budget"), "{md}");
    assert!(under(2, "Tenant quotas"), "{md}");
    assert!(!md.contains("Old map"), "the retired row is left out:\n{md}");

    // The counts are stated above the first table.
    let first_table = lines.iter().position(|l| l.trim_start().starts_with('|')).unwrap();
    let above = lines[..first_table].join("\n").to_lowercase();
    for count in ["2 completed", "1 accepted", "1 proposed"] {
        assert!(above.contains(count), "`{count}` is not stated above the first table:\n{md}");
    }

    // The undeclared row says it is a title standing in; a declared row does not.
    let stand_in = lines.iter().find(|l| l.starts_with('|') && l.contains("Tenant quotas")).unwrap();
    assert!(stand_in.to_lowercase().contains("title"), "the undeclared row is not marked: {stand_in}");
    let declared = lines.iter().find(|l| l.starts_with('|') && l.contains("Power budget")).unwrap();
    assert!(!declared.to_lowercase().contains("title"), "a declared row is marked as a title: {declared}");
}

// ── decision 6: released ────────────────────────────────────────────────

#[test]
fn s19_released_is_the_lowest_version_tag_that_holds_the_commit_a_built_line_names() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    write(root, "README.md", "start\n");
    commit_all(root, "start");
    git(root, &["tag", "v0.1.0"]);

    write(root, "src/trace.txt", "the feature\n");
    let c = commit_all(root, "the cable trace");
    write(root, "notes.txt", "one\n");
    commit_all(root, "notes");
    git(root, &["tag", "v0.2.0"]);
    write(root, "notes.txt", "two\n");
    commit_all(root, "more notes");
    git(root, &["tag", "v0.10.0"]);

    Adr {
        n: 1,
        title: "Cabling",
        status: "Accepted",
        built: Some(format!("**Built:** F1 on 2026-11-14, {c}.")),
        phases: &[],
        features: Some(&["| Cable trace | Follows a cable end to end | |"]),
    }
    .put(root);
    commit_all(root, "the record");

    let v = features_json(root, &[]);
    let trace = row(&v, "Cable trace");
    assert_eq!(trace["state"], "completed");
    assert_eq!(trace["released"], "v0.2.0", "v0.2.0 is lower than v0.10.0 as numbers, and v0.1.0 does not hold {c}: {trace}");
}

#[test]
fn s20_released_is_null_when_the_commit_is_unknown_untagged_or_not_named_and_for_a_row_not_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    write(root, "README.md", "start\n");
    commit_all(root, "start");
    git(root, &["tag", "v0.1.0"]);
    write(root, "src/untagged.txt", "built, in no release\n");
    let untagged = commit_all(root, "built after the last tag");

    Adr {
        n: 1,
        title: "No commit",
        status: "Accepted",
        built: Some(BUILT.into()),
        phases: &[],
        features: Some(&["| Not named | Its Built line names no commit | |"]),
    }
    .put(root);
    Adr {
        n: 2,
        title: "Unknown commit",
        status: "Accepted",
        built: Some(format!("**Built:** F1 on 2026-11-14, {}.", "abcdef0123456")),
        phases: &[],
        features: Some(&["| Not here | Its commit is not in the repository | |"]),
    }
    .put(root);
    Adr {
        n: 3,
        title: "Untagged commit",
        status: "Accepted",
        built: Some(format!("**Built:** F1 on 2026-11-14, {untagged}.")),
        phases: &[],
        features: Some(&["| Not tagged | No version tag contains its commit | |"]),
    }
    .put(root);
    Adr { n: 4, title: "Not built", status: "Accepted", built: None, phases: &[], features: Some(&["| Not built | Decided only | |"]) }
        .put(root);
    commit_all(root, "the records");

    let v = features_json(root, &[]);
    for name in ["Not named", "Not here", "Not tagged"] {
        let r = row(&v, name);
        assert_eq!(r["state"], "completed", "{r}");
        assert!(r["released"].is_null(), "{name} is built and not released: {r}");
    }
    let accepted = row(&v, "Not built");
    assert_eq!(accepted["state"], "accepted");
    assert!(accepted["released"].is_null(), "{accepted}");
    for r in rows(&v) {
        assert!(r.get("released").is_some(), "a row has no `released`: {r}");
    }
}

// ── decision 7: the fact-sheet check ────────────────────────────────────

#[test]
fn s21_md_check_passes_on_a_current_fact_sheet_and_fails_on_a_stale_one() {
    let tmp = project_s14();
    let root = tmp.path();
    let sheet = run(root, &["adr", "features", "--md"]);
    assert_eq!(sheet.code, 0, "{}", sheet.all());
    assert!(sheet.stdout.contains("Power budget"), "{}", sheet.all());
    write(root, "FEATURES.md", &sheet.stdout);

    let current = run(root, &["adr", "features", "--md", "--check", "FEATURES.md"]);
    assert_eq!(current.code, 0, "a fact sheet just written is current:\n{}", current.all());

    // The accepted feature is built: the sheet no longer says what is true.
    let mut built = accepted_adr();
    built.built = Some(BUILT.into());
    built.put(root);

    let stale = run(root, &["adr", "features", "--md", "--check", "FEATURES.md"]);
    assert_eq!(stale.code, 1, "{}", stale.all());
    let said = stale.all();
    assert!(said.contains("fact-sheet-stale"), "{said}");
    assert!(said.contains("FEATURES.md"), "{said}");
    assert!(said.contains("hexa adr features --md > FEATURES.md"), "it names the command that rewrites the file: {said}");
    assert_eq!(read(root, "FEATURES.md"), sheet.stdout, "a check writes nothing");
}

#[test]
fn s22_the_fact_sheet_changes_only_when_a_feature_does_and_a_missing_file_is_not_a_pass() {
    let tmp = project_s14();
    let root = tmp.path();
    git_init(root);
    commit_all(root, "the records");

    let first = run(root, &["adr", "features", "--md"]);
    assert_eq!(first.code, 0, "{}", first.all());
    assert!(first.stdout.contains("Cable trace"), "{}", first.all());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = run(root, &["adr", "features", "--md"]);
    assert_eq!(second.code, 0, "{}", second.all());

    write(root, "unrelated.txt", "nothing to do with a feature\n");
    commit_all(root, "an unrelated commit");
    let third = run(root, &["adr", "features", "--md"]);
    assert_eq!(third.code, 0, "{}", third.all());

    assert_eq!(first.stdout, second.stdout, "no date or time of its own");
    assert_eq!(first.stdout, third.stdout, "no commit of its own");

    let missing = run(root, &["adr", "features", "--md", "--check", "NOT-WRITTEN.md"]);
    assert_eq!(missing.code, 1, "{}", missing.all());
    let said = missing.all();
    assert!(said.contains("fact-sheet-missing"), "{said}");
    assert!(said.contains("hint"), "{said}");
    assert!(said.contains("hexa adr features --md > NOT-WRITTEN.md"), "the hint names the command that writes it: {said}");
    assert!(!root.join("NOT-WRITTEN.md").exists(), "a check writes nothing");
}

// ── decision 8: warnings ────────────────────────────────────────────────

#[test]
fn s25_warnings_do_not_fail_the_command_unless_strict_is_given() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    Adr {
        n: 23,
        title: "Ghosts",
        status: "Accepted",
        built: None,
        phases: &["R1", "R2"],
        features: Some(&["| Ghost | Names a phase that is not there | R9 |"]),
    }
    .put(root);

    let lenient = run(root, &["adr", "features", "--json"]);
    assert_eq!(lenient.code, 0, "a warning alone does not fail:\n{}", lenient.all());
    let v = lenient.json();
    assert_eq!(row(&v, "Ghost")["state"], "unknown");
    let warnings = v["warnings"].as_array().unwrap_or_else(|| panic!("no `warnings` array: {v}"));
    assert!(
        warnings.iter().any(|w| {
            let w = w.to_string();
            w.contains("unknown-phase") && w.contains(&id(23))
        }),
        "no warning names unknown-phase with {}: {}",
        id(23),
        v["warnings"]
    );

    let strict = run(root, &["adr", "features", "--json", "--strict"]);
    assert_eq!(strict.code, 1, "{}", strict.all());
    assert_eq!(strict.stdout, lenient.stdout, "--strict prints the same");
}

// ── decision 9: extraction ──────────────────────────────────────────────

/// A project whose T1 tier is served by a completion command that prints `reply`.
fn with_stub_model(root: &Path, reply: &str) -> PathBuf {
    write(root, ".hexa/project.json", "{\"name\":\"fixture\",\"inference\":{\"tier_models\":{\"t1\":\"stub\"}}}\n");
    write(root, ".test-home/reply.txt", reply);
    let stub = root.join(".test-home").join("stub-model.sh");
    std::fs::write(&stub, "#!/bin/sh\ncat >/dev/null\ncat \"$(dirname \"$0\")/reply.txt\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    stub
}

fn extract(root: &Path, stub: &Path, args: &[&str]) -> Run {
    let mut all = vec!["adr", "features", "extract"];
    all.extend_from_slice(args);
    run_env(root, &all, &[("HEXA_COMPLETION_COMMAND", stub.to_str().unwrap())])
}

/// The lines of the `## Features` section of `text`, without its heading.
fn features_section(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.trim_end() == "## Features").unwrap_or_else(|| panic!("no Features section:\n{text}"));
    let end = lines[start + 1..].iter().position(|l| l.starts_with("## ")).map_or(lines.len(), |i| start + 1 + i);
    lines[start + 1..end].to_vec()
}

#[test]
fn s26_extract_prints_a_features_section_and_writes_nothing_without_write() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let adr = Adr {
        n: 26,
        title: "Cabling",
        status: "Accepted",
        built: Some("**Built:** R1 on 2026-11-14.".into()),
        phases: &["R1", "R2"],
        features: None,
    };
    adr.put(root);
    let original = adr.text();
    assert!(original.contains("## Consequences") && !original.contains("## Features"));
    let stub = with_stub_model(
        root,
        "Here are the features this decision gives a user.\n\n\
         | Feature | What it does | Phase |\n|---|---|---|\n\
         | Cable trace | Follows a cable end to end | R1 |\n\
         | Rack view | Shows what a rack holds | |\n",
    );

    let printed = extract(root, &stub, &[&id(26)]);
    assert_eq!(printed.code, 0, "{}", printed.all());
    let section = features_section(&printed.stdout);
    let table_rows: Vec<Vec<String>> = section.iter().filter(|l| l.trim_start().starts_with('|')).map(|l| cells(l)).collect();
    assert_eq!(table_rows[0], ["Feature", "What it does", "Phase"], "the table of decision 1:\n{}", printed.stdout);
    assert_eq!(table_rows.len(), 4, "a header, a separator and the two rows:\n{}", printed.stdout);
    assert_eq!(table_rows[2], ["Cable trace", "Follows a cable end to end", "R1"]);
    assert_eq!(table_rows[3], ["Rack view", "Shows what a rack holds", ""]);
    assert_eq!(read(root, &adr.file()), original, "without --write the file is as it was");

    let written = extract(root, &stub, &[&id(26), "--write"]);
    assert_eq!(written.code, 0, "{}", written.all());
    let after = read(root, &adr.file());
    let at = after.find("## Features").unwrap_or_else(|| panic!("no Features section was written:\n{after}"));
    let consequences = after.find("## Consequences").unwrap();
    assert!(at < consequences, "the section goes before Consequences:\n{after}");
    let between = features_section(&after);
    assert!(
        between.iter().all(|l| l.trim().is_empty() || l.trim_start().starts_with('|')),
        "nothing but the table stands between the heading and Consequences:\n{after}"
    );
    let original_at = original.find("## Consequences").unwrap();
    assert_eq!(after[..at].trim_end(), original[..original_at].trim_end(), "what came before is untouched");
    assert_eq!(&after[consequences..], &original[original_at..], "Consequences and what follows are untouched");

    let v = features_json(root, &["--adr", &id(26)]);
    assert_eq!(row_names(&v), ["Cable trace", "Rack view"]);
    let trace = row(&v, "Cable trace");
    assert_eq!(trace["declared"], true);
    assert_eq!(trace["phases"], serde_json::json!(["R1"]));
    assert_eq!(trace["state"], "completed", "R1 is built");
    let rack = row(&v, "Rack view");
    assert_eq!(rack["declared"], true);
    assert_eq!(rack["phases"], serde_json::json!([]));
    assert_eq!(rack["state"], "accepted", "one phase of two is built");
}

#[test]
fn s27_extract_refuses_a_declared_adr_and_cannot_call_a_feature_built() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let stub = with_stub_model(
        root,
        "| Feature | What it does | Phase | State |\n|---|---|---|---|\n\
         | Fast path | Skips the queue | | completed |\n",
    );

    let declared = Adr {
        n: 27,
        title: "Declared",
        status: "Accepted",
        built: None,
        phases: &[],
        features: Some(&["| Already here | Named by a person | |"]),
    };
    declared.put(root);
    let refused = extract(root, &stub, &[&id(27), "--write"]);
    assert_ne!(refused.code, 0, "{}", refused.all());
    assert!(refused.all().contains("features-declared"), "{}", refused.all());
    assert_eq!(read(root, &declared.file()), declared.text(), "a refusal changes nothing");

    let unbuilt = Adr { n: 28, title: "Queueing", status: "Accepted", built: None, phases: &[], features: None };
    unbuilt.put(root);
    let written = extract(root, &stub, &[&id(28), "--write"]);
    assert_eq!(written.code, 0, "{}", written.all());
    let after = read(root, &unbuilt.file());
    let section = features_section(&after);
    let table_rows: Vec<Vec<String>> = section.iter().filter(|l| l.trim_start().starts_with('|')).map(|l| cells(l)).collect();
    assert_eq!(table_rows[0], ["Feature", "What it does", "Phase"], "only the three columns:\n{after}");
    assert_eq!(table_rows.len(), 3, "a header, a separator and the one row:\n{after}");
    assert_eq!(table_rows[2], ["Fast path", "Skips the queue", ""]);
    assert!(!section.join("\n").to_lowercase().contains("completed"), "the model's word for the state is not written:\n{after}");

    let v = features_json(root, &["--adr", &id(28)]);
    let fast = row(&v, "Fast path");
    assert_eq!(fast["state"], "accepted", "nothing is built, whatever the model said: {fast}");
    assert_eq!(fast["declared"], true);
}
