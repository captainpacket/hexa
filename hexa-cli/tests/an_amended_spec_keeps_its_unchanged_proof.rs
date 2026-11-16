//! An amended spec keeps the proof of every scenario it did not change.
//!
//! Changing an accepted spec's scenarios takes a commit of the edited text
//! (an acceptance binds committed text, ADR-2611161600 §1), and then a
//! person's re-acceptance. The predecessor the re-acceptance compares
//! against is the last acceptance in history that still held: the commit of
//! the edit itself holds none. Before this, the re-acceptance found no
//! predecessor and dropped every scenario's test, red commit and seal
//! (gap `spec-amend-scenario-path`).
//!
//! The changed scenario's test is then rebound with `hexa spec red`, which
//! records a red commit only after it has watched the named tests fail
//! there, and puts back the seal the re-acceptance dropped, so the edit to
//! the sealed file goes through `hexa spec amend` like any other.
//!
//! Ids are assembled at run time: the citation checker scans source, and a
//! literal id here would claim a decision exists.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn hexa_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hexa"))
}

struct Run {
    code: i32,
    out: String,
}

fn run(dir: &Path, args: &[&str]) -> Run {
    let home = dir.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let o = Command::new(hexa_bin())
        .args(args)
        .current_dir(dir)
        .env("HOME", &home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", dir)
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env("NO_COLOR", "1")
        .output()
        .expect("run hexa");
    Run {
        code: o.status.code().unwrap_or(-1),
        out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn id() -> String {
    format!("ADR-{}", "2701030000")
}

fn spec_rel() -> String {
    format!("docs/specs/{}.json", id())
}

const TESTS: &str = "#[test]\nfn one_holds() { assert_eq!(fx::one(), 1); }\n\n#[test]\nfn answer_is_zero() { assert_eq!(fx::answer(), 0); }\n";
const TESTS_AMENDED: &str =
    "#[test]\nfn one_holds() { assert_eq!(fx::one(), 1); }\n\n#[test]\nfn answer_is_forty_two() { assert_eq!(fx::answer(), 42); }\n";

fn scenario(sid: &str, kind: &str, then: &str, test: &str) -> String {
    format!(
        "{{\"id\":\"{sid}\",\"decision\":[\"§1\"],\"kind\":\"{kind}\",\"description\":\"d {sid}\",\"given\":\"g\",\"when\":\"w\",\"then\":\"{then}\",\"test\":\"tests/a_spec.rs::{test}\"}}"
    )
}

/// A crate, an ADR and a spec whose two scenarios are tested, red at the
/// commit that added the tests, accepted, sealed and committed.
fn sealed_repo() -> (tempfile::TempDir, String) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let id = id();
    write(r, ".gitignore", ".test-home/\ntarget/\n");
    write(r, "Cargo.toml", "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n");
    write(r, "src/lib.rs", "pub fn one() -> u32 { 0 }\npub fn answer() -> u32 { 1 }\n");
    write(
        r,
        &format!("docs/adrs/{id}-a-decision.md"),
        &format!(
            "# {id}: A decision\n\n**Status:** Accepted\n**Date:** 2027-01-03\n\n## Context\n\nx\n\n## Decision\n\n1. **It holds.** Always.\n\n## Consequences\n\nx\n"
        ),
    );
    write(r, "tests/a_spec.rs", TESTS);
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "red: the tests, before the code"]);
    let red = git(r, &["rev-parse", "--short", "HEAD"]);
    write(r, "src/lib.rs", "pub fn one() -> u32 { 1 }\npub fn answer() -> u32 { 0 }\n");
    write(
        r,
        &spec_rel(),
        &format!(
            "{{\"adr\":\"{id}\",\"feature\":\"f\",\"red\":\"{red}\",\"specs\":[{},{}]}}",
            scenario("S1", "example", "one is one", "one_holds"),
            scenario("S2", "negative", "the answer is zero", "answer_is_zero")
        ),
    );
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "the code, and the spec"]);
    let a = run(r, &["spec", "accept", &id, "--by", "a reviewer"]);
    assert_eq!(a.code, 0, "{}", a.out);
    let s = run(r, &["spec", "seal", &id]);
    assert_eq!(s.code, 0, "{}", s.out);
    git(r, &["commit", "-qam", "accepted and sealed"]);
    (d, red)
}

fn spec_json(r: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(r.join(spec_rel())).unwrap()).unwrap()
}

fn scenario_of<'a>(spec: &'a Value, sid: &str) -> &'a Value {
    spec["specs"].as_array().unwrap().iter().find(|s| s["id"] == sid).unwrap()
}

/// S2's behaviour changes: the edit is committed, then re-accepted.
fn amend_s2_and_reaccept(r: &Path) -> Run {
    let p = r.join(spec_rel());
    let edited = std::fs::read_to_string(&p).unwrap().replace("the answer is zero", "the answer is forty-two");
    std::fs::write(&p, edited).unwrap();
    git(r, &["commit", "-qam", "S2 amended"]);
    run(r, &["spec", "accept", &id(), "--by", "a reviewer"])
}

#[test]
fn reaccepting_a_committed_edit_keeps_the_unchanged_scenarios_tests_and_red() {
    let (d, red) = sealed_repo();
    let r = d.path();
    let a = amend_s2_and_reaccept(r);
    assert_eq!(a.code, 0, "{}", a.out);
    let spec = spec_json(r);
    let s1 = scenario_of(&spec, "S1");
    assert_eq!(s1["test"], "tests/a_spec.rs::one_holds", "the unchanged scenario keeps its test:\n{spec:#}");
    assert_eq!(s1["red"], red.as_str(), "and its red commit:\n{spec:#}");
    assert!(scenario_of(&spec, "S2").get("test").is_none(), "the changed scenario loses its proof:\n{spec:#}");
    assert!(a.out.contains("S2") && a.out.contains("hexa spec red"), "the next step is named:\n{}", a.out);
}

#[test]
fn spec_red_refuses_a_commit_at_which_a_named_test_passes() {
    let (d, _) = sealed_repo();
    let r = d.path();
    assert_eq!(amend_s2_and_reaccept(r).code, 0);
    git(r, &["commit", "-qam", "accepted as amended"]);
    // The amended test, committed after the code that already satisfies it.
    write(r, "src/lib.rs", "pub fn one() -> u32 { 1 }\npub fn answer() -> u32 { 42 }\n");
    write(r, "tests/a_spec.rs", TESTS_AMENDED);
    git(r, &["commit", "-qam", "code and test together"]);
    let late = git(r, &["rev-parse", "--short", "HEAD"]);
    let before = std::fs::read_to_string(r.join(spec_rel())).unwrap();
    let out = run(r, &["spec", "red", &id(), &late, "--test", "S2=tests/a_spec.rs::answer_is_forty_two"]);
    assert_ne!(out.code, 0, "a test green at the commit was not written first:\n{}", out.out);
    assert!(out.out.contains("passes") && out.out.contains("S2"), "{}", out.out);
    assert_eq!(std::fs::read_to_string(r.join(spec_rel())).unwrap(), before, "nothing is recorded");
}

#[test]
fn spec_red_records_a_commit_at_which_the_named_tests_fail_and_the_amended_spec_verifies() {
    let (d, _) = sealed_repo();
    let r = d.path();
    assert_eq!(amend_s2_and_reaccept(r).code, 0);
    git(r, &["commit", "-qam", "accepted as amended"]);
    write(r, "tests/a_spec.rs", TESTS_AMENDED);
    git(r, &["commit", "-qam", "S2's test as amended, red"]);
    let red2 = git(r, &["rev-parse", "--short", "HEAD"]);
    write(r, "src/lib.rs", "pub fn one() -> u32 { 1 }\npub fn answer() -> u32 { 42 }\n");
    git(r, &["commit", "-qam", "the code"]);

    let out = run(r, &["spec", "red", &id(), &red2, "--test", "S2=tests/a_spec.rs::answer_is_forty_two"]);
    assert_eq!(out.code, 0, "{}", out.out);
    let spec = spec_json(r);
    let s2 = scenario_of(&spec, "S2");
    assert_eq!(s2["test"], "tests/a_spec.rs::answer_is_forty_two", "{spec:#}");
    assert_eq!(s2["red"], red2.as_str(), "{spec:#}");
    assert!(spec["sealed"].get("tests/a_spec.rs").is_some(), "the dropped seal is put back:\n{spec:#}");
    assert!(out.out.contains("hexa spec amend"), "the edit since the seal goes through amend:\n{}", out.out);

    let shown = run(r, &["spec", "amend", &id(), "--file", "tests/a_spec.rs", "--reason", "S2 amended", "--by", "a reviewer"]);
    assert_eq!(shown.code, 0, "{}", shown.out);
    let sha = shown.out.split("--confirm ").nth(1).and_then(|s| s.split_whitespace().next()).expect("a confirm hash").to_string();
    let rec =
        run(r, &["spec", "amend", &id(), "--file", "tests/a_spec.rs", "--reason", "S2 amended", "--by", "a reviewer", "--confirm", &sha]);
    assert_eq!(rec.code, 0, "{}", rec.out);
    git(r, &["commit", "-qam", "S2 rebound, seal amended"]);
    let v = run(r, &["spec", "verify", &id()]);
    assert_eq!(v.code, 0, "the amended spec verifies:\n{}", v.out);
    assert!(v.out.contains("2 passed") && v.out.contains("0 failed") && v.out.contains("0 pending"), "{}", v.out);
}

#[test]
fn spec_red_refuses_a_scenario_that_still_has_its_test() {
    let (d, red) = sealed_repo();
    let r = d.path();
    let before = std::fs::read_to_string(r.join(spec_rel())).unwrap();
    let out = run(r, &["spec", "red", &id(), &red, "--test", "S1=tests/a_spec.rs::answer_is_zero"]);
    assert_ne!(out.code, 0, "a scenario's test changes through the spec:\n{}", out.out);
    assert!(out.out.contains("S1"), "{}", out.out);
    assert_eq!(std::fs::read_to_string(r.join(spec_rel())).unwrap(), before, "nothing is recorded");
}
