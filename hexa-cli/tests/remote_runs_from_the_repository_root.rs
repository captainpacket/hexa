//! A remote run of a red commit starts from the repository root (gaberger/hexa#68).
//!
//! ADR-2611191120 §3: "The template runs locally with bash from the repository
//! root." The runner script a template names is the project's infrastructure,
//! not part of the commit under test: an old commit's copy may not exist, or
//! may not take the template's flags. So `hexa spec red` and `hexa spec verify
//! --prove` run the template from the root, and the runner checks the red
//! commit out on its own side. Under `test.remote` nothing is checked out
//! here: no `target/hexa-prove`, no `target/` at all.
//!
//! The fixture's red commit predates its runner script, as strata's did: run
//! from a checkout of that commit, the template finds no script.
//!
//! Fixture ids are assembled at run time: the citation checker scans source,
//! and a literal id here would claim a decision exists.

use std::path::{Path, PathBuf};
use std::process::Command;

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
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .output()
        .expect("run hexa");
    Run { code: o.status.code().unwrap_or(-1), out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)) }
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

fn the_id() -> String {
    format!("ADR-{}", "2701190001")
}

/// Passes once `src/impl.txt` says `done`: red before the code, green after,
/// and the test file itself never changes, so its seal holds.
const CHECK: &str = "#!/bin/bash\ngrep -q done src/impl.txt\n";

struct Fixture {
    repo: tempfile::TempDir,
    _side: tempfile::TempDir,
    /// One line per runner invocation: the directory the template ran in.
    log: PathBuf,
    /// The commit the tests were written at, before the runner script existed.
    red: String,
    /// The first commit: it holds `tests/a.sh` but not `tests/b.sh`.
    first: String,
}

fn fixture() -> Fixture {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    let id = the_id();
    write(root, ".gitignore", ".test-home/\ntarget/\n");
    write(root, "package.json", "{}\n");
    write(root, "src/impl.txt", "todo\n");
    write(root, "tests/a.sh", CHECK);
    write(
        root,
        &format!("docs/adrs/{id}-a-decision.md"),
        &format!("# {id}: A decision\n\n**Status:** Accepted\n**Date:** 2026-11-01\n\n## Context\n\nx\n\n## Decision\n\n1. **It holds.** Always.\n\n## Consequences\n\nx\n"),
    );
    let sc = |sid: &str, kind: &str, t: &str| {
        serde_json::json!({ "id": sid, "decision": ["§1"], "kind": kind, "description": format!("{sid} holds"),
                            "given": "a thing", "when": "it runs", "then": "it holds", "test": t })
    };
    let spec = serde_json::json!({ "adr": id, "feature": "a-feature",
        "specs": [sc("S1", "example", "tests/a.sh"), sc("S2", "negative", "tests/b.sh")] });
    write(root, &format!("docs/specs/{id}.json"), &(serde_json::to_string_pretty(&spec).unwrap() + "\n"));
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "init"]);
    let first = git(root, &["rev-parse", "HEAD"]);
    write(root, "tests/b.sh", CHECK);
    let r = run(root, &["spec", "accept", &id, "--by", "a reviewer"]);
    assert_eq!(r.code, 0, "the fixture's spec is accepted:\n{}", r.out);
    let r = run(root, &["spec", "seal", &id]);
    assert_eq!(r.code, 0, "the fixture's tests are sealed:\n{}", r.out);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "accept"]);
    let red = git(root, &["rev-parse", "HEAD"]);

    // Later: the code, and a runner script in the repository that clones the
    // commit it is given and runs the command there, as a build server would.
    let side = tempfile::tempdir().unwrap();
    let origin = side.path().join("origin.git");
    git(side.path(), &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    let log = side.path().join("runner.log");
    write(
        root,
        "ci/runner.sh",
        &format!(
            "#!/bin/bash\npwd -P >> '{log}'\nd=$(mktemp -d)\n\
             git clone -q '{origin}' \"$d/c\" && cd \"$d/c\" && git checkout -q \"$1\" || exit 2\n\
             echo \"runner: testing $(git rev-parse HEAD)\"\nbash -c \"$2\"; rc=$?\ncd /; rm -rf \"$d\"; exit $rc\n",
            log = log.display(),
            origin = origin.display()
        ),
    );
    let pj = serde_json::json!({ "test": { "remote": "bash ci/runner.sh {commit} {cmd}" } });
    write(root, ".hexa/project.json", &serde_json::to_string_pretty(&pj).unwrap());
    write(root, "src/impl.txt", "done\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "the code, and a remote runner"]);
    git(root, &["push", "-q", "origin", "main"]);
    Fixture { repo, _side: side, log, red, first }
}

fn ran_in(f: &Fixture) -> Vec<String> {
    std::fs::read_to_string(&f.log).unwrap_or_default().lines().map(String::from).collect()
}

fn canonical(p: &Path) -> String {
    std::fs::canonicalize(p).unwrap().display().to_string()
}

#[test]
fn spec_red_runs_the_remote_template_from_the_repository_root() {
    let f = fixture();
    let root = f.repo.path();
    let r = run(root, &["spec", "red", &the_id(), &f.red]);
    assert_eq!(r.code, 0, "both tests are red at a commit that predates the runner script:\n{}", r.out);
    let dirs = ran_in(&f);
    assert_eq!(dirs, vec![canonical(root)], "one remote run, from the repository root:\n{}", r.out);
    assert!(!root.join("target").exists(), "a remote run checks nothing out here:\n{}", r.out);
}

#[test]
fn spec_verify_prove_runs_the_red_half_from_the_repository_root() {
    let f = fixture();
    let root = f.repo.path();
    let id = the_id();
    let r = run(root, &["spec", "red", &id, &f.red]);
    assert_eq!(r.code, 0, "{}", r.out);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "red recorded"]);
    git(root, &["push", "-q", "origin", "main"]);
    let before = ran_in(&f).len();

    let r = run(root, &["spec", "verify", &id, "--prove"]);
    assert_eq!(r.code, 0, "green at HEAD and red at its commit, both on the remote:\n{}", r.out);
    let short: String = f.red.chars().take(7).collect();
    assert_eq!(r.out.matches(&format!("red at {short}")).count(), 2, "both scenarios red at {short}:\n{}", r.out);
    let dirs = ran_in(&f);
    assert!(dirs.len() > before, "the red half went to the runner:\n{}", r.out);
    assert!(dirs.iter().all(|d| *d == canonical(root)), "every remote run started from the repository root: {dirs:?}\n{}", r.out);
    assert!(!root.join("target").exists(), "no target/hexa-prove under test.remote:\n{}", r.out);
}

#[test]
fn a_test_absent_at_the_red_commit_is_not_red_on_the_remote() {
    // Without a local checkout, nothing here shows the file is missing at the
    // commit; sent anyway, `bash tests/b.sh` would exit 127 there and be
    // judged a failing test: red, wrongly.
    let f = fixture();
    let root = f.repo.path();
    let r = run(root, &["spec", "red", &the_id(), &f.first]);
    assert_ne!(r.code, 0, "a test that is not at the commit cannot be red there:\n{}", r.out);
    assert!(r.out.contains("tests/b.sh"), "names the missing file:\n{}", r.out);
    assert!(!r.out.contains("S2 red at"), "{}", r.out);
    assert!(!root.join("target").exists(), "{}", r.out);
}
