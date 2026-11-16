//! `hexa build` commits the files the build changed, and never the operator's
//! own uncommitted edits — not even when they are in the build's target.
//!
//! The commit once added the target back after filtering out the operator's
//! dirty paths, so `git commit -- <target>` committed the operator's pre-run
//! edits to it (or, for a directory target, to any file under it) under the
//! hexa-factory identity.
//!
//! Since ADR-2611160730 §1 a tracked edit refuses the build before it
//! starts; an untracked file of the operator's is still never swept in.
//!
//! The frontier is a fake: every design, critique and spec is a line of text,
//! and the build agent writes `src/lib/built.txt`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hexa_exec::ports::{Frontier, FrontierError};

struct FakeFrontier;

#[async_trait]
impl Frontier for FakeFrontier {
    async fn run(&self, prompt: &str, cwd: &Path, _t: Duration, _s: &str) -> Result<String, FrontierError> {
        if prompt.starts_with("Implement the following spec") {
            std::fs::write(cwd.join("src/lib/built.txt"), "built\n").unwrap();
            return Ok("built".to_string());
        }
        Ok("a design".to_string())
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repository whose `src/lib/` holds a committed `mine.txt` and an
/// untracked `draft.txt` of the operator's.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    git(&dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join("src/lib")).unwrap();
    std::fs::write(dir.join("src/lib/mine.txt"), "committed\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@x", "commit", "-qm", "init"]);
    std::fs::write(dir.join("src/lib/draft.txt"), "operator draft\n").unwrap();
    (tmp, dir)
}

async fn build(dir: &Path, target: &str) -> hexa_exec::adversarial::BuildReport {
    hexa_exec::adversarial::run_build(Arc::new(FakeFrontier), "a challenge", target, "true", 2, dir, 30, 0).await
}

fn committed_files(dir: &Path) -> Vec<String> {
    git(dir, &["show", "--name-only", "--format=", "HEAD"]).lines().map(str::to_string).collect()
}

fn assert_operator_work_left_alone(dir: &Path) {
    let status = git(dir, &["status", "--porcelain", "-uall"]);
    assert!(status.contains("?? src/lib/draft.txt"), "the operator's draft is still untracked: {status}");
    assert_eq!(git(dir, &["show", "HEAD:src/lib/built.txt"]), "built\n");
}

/// A target file the operator had edited refuses the build: nothing is
/// built, and nothing is committed over their edit (ADR-2611160730 §1).
#[tokio::test]
async fn a_target_file_the_operator_had_edited_is_not_committed() {
    let (_tmp, dir) = repo();
    std::fs::write(dir.join("src/lib/mine.txt"), "operator edit\n").unwrap();
    let report = build(&dir, "src/lib/mine.txt").await;

    assert!(!report.build_ok, "notes: {:?}", report.notes);
    assert!(report.stopped.as_deref().is_some_and(|w| w.contains("src/lib/mine.txt")), "{:?}", report.stopped);
    assert_eq!(git(&dir, &["log", "--format=%s"]).trim(), "init", "nothing committed");
    assert_eq!(std::fs::read_to_string(dir.join("src/lib/mine.txt")).unwrap(), "operator edit\n");
}

/// A file target scopes the commit to its directory.
#[tokio::test]
async fn a_target_file_scopes_the_commit_to_its_directory() {
    let (_tmp, dir) = repo();
    let report = build(&dir, "src/lib/mine.txt").await;

    assert!(report.build_ok, "notes: {:?}", report.notes);
    assert_eq!(committed_files(&dir), vec!["src/lib/built.txt"], "notes: {:?}", report.notes);
    assert_operator_work_left_alone(&dir);
}

#[tokio::test]
async fn a_target_directory_does_not_sweep_in_the_operators_files() {
    let (_tmp, dir) = repo();
    let report = build(&dir, "src/lib").await;

    assert!(report.build_ok, "notes: {:?}", report.notes);
    assert_eq!(committed_files(&dir), vec!["src/lib/built.txt"], "notes: {:?}", report.notes);
    assert_operator_work_left_alone(&dir);
}
