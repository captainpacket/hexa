//! `hexa do gate`: the agent's own patch, gated and committed (ADR-2611160730 §3).
//!
//! Written before the code. An agent writes exact patches itself; `do run`
//! hands the edit to a second model, so agents routed around hexa entirely.
//! `do gate` takes the change already in the tree, runs the evidence, and on a
//! real pass commits ONLY the named files. A failing or vacuous gate commits
//! nothing and leaves the patch in place for the agent to fix.

use std::path::Path;
use std::process::Command;

use hexa_exec::direct_exec::gate_patch;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t"]);
    git(dir, &["config", "user.name", "t"]);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(dir.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(dir.join("src/b.rs"), "// b\n").unwrap();
    std::fs::write(dir.join("src/other.rs"), "// other\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "init"]);
    tmp
}

fn files(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn count(dir: &Path) -> String {
    git(dir, &["rev-list", "--count", "HEAD"]).trim().to_string()
}

const PASSING: &str = "echo 'test result: ok. 3 passed; 0 failed; 0 ignored'";
const VACUOUS: &str = "echo 'test result: ok. 0 passed; 0 failed; 0 ignored'";

#[tokio::test]
async fn a_passing_gate_commits_only_the_named_files() {
    let tmp = repo();
    let dir = tmp.path();
    std::fs::write(dir.join("src/a.rs"), "// a v2\n").unwrap();
    std::fs::write(dir.join("src/new.rs"), "// new\n").unwrap();
    std::fs::write(dir.join("src/other.rs"), "// someone else's edit\n").unwrap();

    let r = gate_patch(dir, &files(&["src/a.rs", "src/new.rs"]), PASSING, "change a", false).await;

    let sha = r.expect("a real pass commits");
    assert!(!sha.is_empty());
    let mut got: Vec<String> = git(dir, &["show", "--name-only", "--format=", "HEAD"]).lines().map(str::to_string).collect();
    got.sort();
    assert_eq!(got, vec!["src/a.rs", "src/new.rs"]);
    assert!(git(dir, &["status", "--porcelain"]).contains(" M src/other.rs"), "the unnamed edit is left as it was");
    let msg = git(dir, &["log", "-1", "--format=%B"]);
    assert!(msg.starts_with("change a"), "the subject is the agent's message: {msg}");
    assert!(msg.contains(PASSING), "the commit records the gate it passed: {msg}");
}

#[tokio::test]
async fn a_failing_gate_commits_nothing_and_leaves_the_patch() {
    let tmp = repo();
    let dir = tmp.path();
    std::fs::write(dir.join("src/a.rs"), "// a v2\n").unwrap();

    let err = gate_patch(dir, &files(&["src/a.rs"]), "false", "change a", false).await.unwrap_err();

    assert!(err.contains("false"), "the error says which gate failed: {err}");
    assert_eq!(count(dir), "1");
    assert_eq!(std::fs::read_to_string(dir.join("src/a.rs")).unwrap(), "// a v2\n", "the agent's patch is not reverted");
}

#[tokio::test]
async fn a_gate_that_ran_no_tests_is_not_a_pass() {
    let tmp = repo();
    let dir = tmp.path();
    std::fs::write(dir.join("src/a.rs"), "// a v2\n").unwrap();

    let err = gate_patch(dir, &files(&["src/a.rs"]), VACUOUS, "change a", false).await.unwrap_err();

    assert!(err.contains("0 tests"), "{err}");
    assert_eq!(count(dir), "1");
}

#[tokio::test]
async fn output_no_runner_wrote_needs_uncounted() {
    let tmp = repo();
    let dir = tmp.path();
    std::fs::write(dir.join("src/a.rs"), "// a v2\n").unwrap();

    assert!(gate_patch(dir, &files(&["src/a.rs"]), "true", "change a", false).await.is_err(), "`true` proves nothing");
    let sha = gate_patch(dir, &files(&["src/a.rs"]), "true", "change a", true).await.expect("declared uncounted, exit 0 decides");
    assert!(!sha.is_empty());
    assert!(git(dir, &["log", "-1", "--format=%B"]).contains("uncounted"), "the commit says the gate was uncounted");
}

#[tokio::test]
async fn a_named_file_with_no_change_is_refused() {
    let tmp = repo();
    let dir = tmp.path();
    std::fs::write(dir.join("src/a.rs"), "// a v2\n").unwrap();

    let err = gate_patch(dir, &files(&["src/a.rs", "src/b.rs"]), PASSING, "change a", false).await.unwrap_err();

    assert!(err.contains("src/b.rs"), "{err}");
    assert_eq!(count(dir), "1");
}

#[tokio::test]
async fn no_files_credentials_and_escaping_paths_are_refused() {
    let tmp = repo();
    let dir = tmp.path();
    std::fs::write(dir.join(".env"), "SECRET=1\n").unwrap();
    assert!(gate_patch(dir, &[], PASSING, "x", false).await.is_err(), "no files named");
    assert!(gate_patch(dir, &files(&[".env"]), PASSING, "x", false).await.unwrap_err().contains(".env"));
    assert!(gate_patch(dir, &files(&["../out.rs"]), PASSING, "x", false).await.is_err());
    assert_eq!(count(dir), "1");
}
