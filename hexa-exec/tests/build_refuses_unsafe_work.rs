//! `hexa build` is safe to run beside a person (ADR-2611160730 §1): it
//! refuses a dirty tracked tree and a second concurrent run, commits only
//! under its `--target`, never commits over a broken seal, and stops when
//! HEAD moved under it by someone else's hand.

#[path = "support/harden_fake.rs"]
mod fake;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fake::{git, repo, subjects};
use hexa_exec::adversarial::{run_build, BuildReport};
use hexa_exec::ports::{Frontier, FrontierError};

const GATE: &str = "echo 'test result: ok. 1 passed; 0 failed;'";

/// A builder: designs are text, and the build writes `src/lib/built.txt`
/// and whatever `extra` asks for. The first call waits on `hold`, when set.
struct Builder {
    calls: AtomicUsize,
    hold: Option<Arc<tokio::sync::Notify>>,
    started: Arc<tokio::sync::Notify>,
    extra: fn(&Path),
}

impl Builder {
    fn new(extra: fn(&Path)) -> Arc<Builder> {
        Arc::new(Builder { calls: AtomicUsize::new(0), hold: None, started: Arc::new(tokio::sync::Notify::new()), extra })
    }
}

#[async_trait]
impl Frontier for Builder {
    async fn run(&self, prompt: &str, cwd: &Path, _t: Duration, _s: &str) -> Result<String, FrontierError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(hold) = &self.hold {
                self.started.notify_one();
                hold.notified().await;
            }
        }
        if prompt.starts_with("Implement the following spec") {
            std::fs::create_dir_all(cwd.join("src/lib")).unwrap();
            std::fs::write(cwd.join("src/lib/built.txt"), "built\n").unwrap();
            (self.extra)(cwd);
            return Ok("built".to_string());
        }
        Ok("a design".to_string())
    }
}

async fn build(dir: &Path, frontier: Arc<Builder>) -> BuildReport {
    run_build(frontier, "a challenge", "src/lib", GATE, 2, dir, 30, 0).await
}

fn nothing(_: &Path) {}

#[tokio::test]
async fn build_refuses_a_second_concurrent_run_via_the_lock() {
    let (_tmp, dir) = repo();
    let hold = Arc::new(tokio::sync::Notify::new());
    let first = Arc::new(Builder {
        calls: AtomicUsize::new(0),
        hold: Some(hold.clone()),
        started: Arc::new(tokio::sync::Notify::new()),
        extra: nothing,
    });
    let started = first.started.clone();
    let d = dir.clone();
    let running = tokio::spawn(async move { build(&d, first).await });
    started.notified().await;

    let second = Builder::new(nothing);
    let refused = build(&dir, second.clone()).await;
    assert!(!refused.build_ok);
    let why = refused.stopped.clone().expect("the second run is refused");
    assert!(why.contains("hexa-harden.lock") && why.contains("another"), "{why}");
    assert_eq!(second.calls.load(Ordering::SeqCst), 0, "the refused run asked no agent");

    hold.notify_one();
    let done = running.await.unwrap();
    assert!(done.build_ok, "the first run finishes: {:?}", done.notes);
    assert_eq!(subjects(&dir), vec!["feat: a challenge", "init"]);
    assert!(!dir.join(".git/hexa-harden.lock").exists(), "the lock is released");
}

#[tokio::test]
async fn build_refuses_on_a_dirty_tracked_tree() {
    let (_tmp, dir) = repo();
    std::fs::write(dir.join("notes.txt"), "a person's edit\n").unwrap();
    let b = Builder::new(nothing);
    let report = build(&dir, b.clone()).await;

    assert!(!report.build_ok);
    let why = report.stopped.expect("refused");
    assert!(why.contains("notes.txt") && why.contains("stash"), "{why}");
    assert_eq!(b.calls.load(Ordering::SeqCst), 0);
}

/// The build agent also writes outside its target — a person's file, or a
/// stray. Only what is under `--target` is committed.
#[tokio::test]
async fn build_commits_only_under_its_target() {
    let (_tmp, dir) = repo();
    fn outside(cwd: &Path) {
        std::fs::write(cwd.join("notes.txt"), "changed outside the target\n").unwrap();
        std::fs::write(cwd.join("stray.txt"), "stray\n").unwrap();
    }
    let report = build(&dir, Builder::new(outside)).await;

    assert!(report.build_ok, "{:?}", report.notes);
    let committed = git(&dir, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(committed.trim(), "src/lib/built.txt", "notes: {:?}", report.notes);
    let status = git(&dir, &["status", "--porcelain", "-uall"]);
    assert!(status.contains(" M notes.txt") && status.contains("?? stray.txt"), "left in the tree: {status}");
    assert!(report.notes.iter().any(|n| n.contains("outside") && n.contains("notes.txt")), "{:?}", report.notes);
}

/// A build that edits a sealed test is not committed (ADR-2610221540 §6).
#[tokio::test]
async fn build_never_commits_over_a_broken_seal() {
    let (_tmp, dir) = repo();
    std::fs::create_dir_all(dir.join("docs/specs")).unwrap();
    std::fs::create_dir_all(dir.join("src/lib")).unwrap();
    std::fs::write(dir.join("src/lib/sealed_test.txt"), "sealed\n").unwrap();
    let sum = sha256_hex(b"sealed\n");
    std::fs::write(
        dir.join("docs/specs/s.json"),
        serde_json::json!({ "sealed": { "src/lib/sealed_test.txt": sum } }).to_string(),
    )
    .unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@x", "commit", "-qm", "seal"]);
    fn tamper(cwd: &Path) {
        std::fs::write(cwd.join("src/lib/sealed_test.txt"), "weakened\n").unwrap();
    }
    let report = build(&dir, Builder::new(tamper)).await;

    assert!(!report.build_ok, "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("sealed")), "{:?}", report.notes);
    assert_eq!(subjects(&dir), vec!["seal", "init"], "nothing committed");
}

/// The build agent's run ends with HEAD moved by someone else: nothing is
/// committed and nothing reset.
#[tokio::test]
async fn build_stops_when_head_moved_under_it() {
    let (_tmp, dir) = repo();
    fn person_commits(cwd: &Path) {
        std::fs::write(cwd.join("person.txt"), "mine\n").unwrap();
        git(cwd, &["add", "person.txt"]);
        git(cwd, &["-c", "user.name=Pat", "-c", "user.email=pat@example.com", "commit", "-qm", "person"]);
    }
    let report = build(&dir, Builder::new(person_commits)).await;

    assert!(!report.build_ok, "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("Pat") && n.contains("not made by hexa")), "{:?}", report.notes);
    assert_eq!(subjects(&dir), vec!["person", "init"], "the person's commit stands, and nothing is added");
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|x| format!("{x:02x}")).collect()
}
