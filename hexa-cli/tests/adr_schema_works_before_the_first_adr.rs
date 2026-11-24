//! `hexa adr schema` is the command a person runs to learn the format of their
//! first ADR, so it has to work before there is one (gaberger/hexa#25).
//!
//! 1. With no `docs/adrs/`, it prints the schema and names the folder the
//!    first ADR will create, instead of failing.
//! 2. With no `TEMPLATE.md`, it prints a template, not a warning.
//! 3. The readable status list and the JSON status list are the same list.

use std::path::Path;
use std::process::Command;

fn schema(dir: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(["adr", "schema"])
        .current_dir(dir)
        .output()
        .expect("run hexa");
    (
        out.status.code().unwrap_or(-1),
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
    )
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    Command::new("git").args(["init", "-q"]).current_dir(d.path()).status().expect("git init");
    d
}

/// The statuses on the readable line under "Valid statuses".
fn readable_statuses(out: &str) -> Vec<String> {
    let mut lines = out.lines().skip_while(|l| !l.contains("Valid statuses"));
    lines.next();
    let line = lines.next().expect("a line after the Valid statuses heading");
    line.split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// The `valid_statuses` array in the JSON block.
fn json_statuses(out: &str) -> Vec<String> {
    let start = out.find("{\n").expect("a JSON block");
    let v: serde_json::Value = serde_json::from_str(&out[start..]).expect("the JSON block parses");
    v["valid_statuses"]
        .as_array()
        .expect("valid_statuses is an array")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn schema_with_no_adr_folder_prints_the_schema_and_names_the_folder() {
    let d = repo();
    let (code, out) = schema(d.path());
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Next ID:"), "{out}");
    assert!(out.contains("docs/adrs"), "names the folder the first ADR goes in:\n{out}");
    assert!(!d.path().join("docs/adrs").exists(), "schema only reads; it creates nothing");
}

#[test]
fn schema_with_no_template_prints_a_template_not_a_warning() {
    let d = repo();
    std::fs::create_dir_all(d.path().join("docs/adrs")).unwrap();
    let (code, out) = schema(d.path());
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("TEMPLATE.md not found"), "{out}");
    let template = out.split("── Template ──").nth(1).expect("a Template section");
    assert!(template.contains("## Context") && template.contains("## Decision"), "{out}");
}

#[test]
fn a_projects_own_template_still_wins() {
    let d = repo();
    std::fs::create_dir_all(d.path().join("docs/adrs")).unwrap();
    std::fs::write(d.path().join("docs/adrs/TEMPLATE.md"), "# ADR-{YYMMDDHHMM}: our own shape\n").unwrap();
    let (_, out) = schema(d.path());
    assert!(out.contains(": our own shape"), "{out}");
    assert!(!out.contains("{YYMMDDHHMM}: our own shape"), "the placeholder is filled:\n{out}");
}

#[test]
fn the_readable_and_json_status_lists_agree() {
    let d = repo();
    let (_, out) = schema(d.path());
    let readable = readable_statuses(&out);
    let json = json_statuses(&out);
    assert_eq!(readable, json, "{out}");
    for s in ["Proposed", "Accepted", "Completed", "Rejected"] {
        assert!(json.iter().any(|j| j == s), "{s} is a status hexa reads:\n{out}");
    }
}
