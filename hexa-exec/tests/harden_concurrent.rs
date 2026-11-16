//! `hexa harden` is safe to run beside a person (ADR-2611160730 §1).
//!
//! Before this ADR, harden soft-reset HEAD whenever it moved during a fix —
//! whoever moved it — reverted or committed every file that changed while the
//! agent ran, and rebuilt batches with `reset --keep` plus cherry-picks of its
//! own fixes, dropping any commit made in the meantime. Each was reproduced in
//! a throwaway repository by replaying the same git commands.
//!
//! Now it refuses to start on a dirty tracked tree or a detached HEAD, holds a
//! per-repo lock, folds back only commits hexa made, never resets away a
//! commit it did not make, and never destroys bytes it did not write.

#[path = "support/harden_fake.rs"]
mod fake;

use std::path::Path;
use std::sync::Arc;

use fake::{commit_as, git, good_fix, is_ancestor, repo, subjects, Fake, FixAction, GATE, TARGET};
use hexa_exec::adversarial::{run_review_gated, run_review_with, HardenGates, ReviewReport};

async fn harden(dir: &Path, frontier: Arc<Fake>) -> ReviewReport {
    run_review_with(frontier, TARGET, GATE, dir, Arc::new(|_| {})).await
}

fn gates(gate: &str, fast: Option<&str>) -> HardenGates {
    HardenGates { gate: gate.to_string(), fast_gate: fast.map(str::to_string), batch: None, baseline: false, lessons: Vec::new() }
}

// ── (a) refuse to start ──────────────────────────────────────────────────────

#[tokio::test]
async fn harden_refuses_on_a_dirty_tracked_tree() {
    let (_tmp, dir) = repo();
    std::fs::write(dir.join("notes.txt"), "a person's edit\n").unwrap();
    let frontier = Fake::new(1, good_fix());
    let report = harden(&dir, frontier.clone()).await;

    let why = report.stopped.clone().expect("the pass refuses to start");
    assert!(why.contains("notes.txt"), "names the dirty file: {why}");
    assert!(why.contains("commit") && why.contains("stash") && why.contains("worktree"), "says what to do: {why}");
    assert_eq!(frontier.calls(), 0, "nothing was asked of any agent");
    assert!(report.gate_runs.is_empty(), "no gate ran");
    assert_eq!(std::fs::read_to_string(dir.join("notes.txt")).unwrap(), "a person's edit\n");
    assert!(report.verdict_line().contains("refused"), "{}", report.verdict_line());
}

#[tokio::test]
async fn untracked_files_are_allowed_and_named() {
    let (_tmp, dir) = repo();
    std::fs::write(dir.join("scratch.txt"), "mine\n").unwrap();
    let report = harden(&dir, Fake::new(1, good_fix())).await;

    assert!(report.stopped.is_none(), "untracked files do not stop the pass: {:?}", report.stopped);
    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    assert!(
        report.notes.iter().any(|n| n.contains("untracked") && n.contains("scratch.txt")),
        "says the untracked files are there: {:?}",
        report.notes
    );
    assert_eq!(std::fs::read_to_string(dir.join("scratch.txt")).unwrap(), "mine\n");
}

#[tokio::test]
async fn harden_refuses_on_a_detached_head() {
    let (_tmp, dir) = repo();
    git(&dir, &["checkout", "-q", "--detach"]);
    let frontier = Fake::new(1, good_fix());
    let report = harden(&dir, frontier.clone()).await;

    let why = report.stopped.expect("a detached HEAD is refused");
    assert!(why.contains("detached"), "{why}");
    assert_eq!(frontier.calls(), 0);
}

// ── (b) one pass per repository ──────────────────────────────────────────────

#[tokio::test]
async fn a_live_lock_refuses_the_pass_and_a_stale_one_is_cleared() {
    let (_tmp, dir) = repo();
    let lock = dir.join(".git/hexa-harden.lock");

    // Held by a live process: this one.
    std::fs::write(&lock, std::process::id().to_string()).unwrap();
    let frontier = Fake::new(1, good_fix());
    let report = harden(&dir, frontier.clone()).await;
    let why = report.stopped.expect("a held lock refuses the pass");
    assert!(why.contains("hexa-harden.lock") && why.contains(&std::process::id().to_string()), "{why}");
    assert_eq!(frontier.calls(), 0);
    assert!(lock.exists(), "another run's lock is not removed");

    // Held by a process that is gone.
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    std::fs::write(&lock, dead.to_string()).unwrap();
    let report = harden(&dir, Fake::new(1, good_fix())).await;
    assert!(report.stopped.is_none(), "a stale lock is cleared: {:?}", report.stopped);
    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    assert!(!lock.exists(), "the pass releases its own lock");
}

// ── (c) HEAD moved during the agent's run ────────────────────────────────────

/// While the agent works, a person commits a file of their own. The pass
/// stops, says why, and touches nothing: the person's commit stays on the
/// branch.
#[tokio::test]
async fn a_commit_by_someone_else_during_a_fix_survives() {
    let (_tmp, dir) = repo();
    let person_sha = Arc::new(std::sync::Mutex::new(String::new()));
    let seen = person_sha.clone();
    let fix: FixAction = Arc::new(move |cwd: &Path, n: usize| {
        good_fix()(cwd, n);
        // The person's work, not the agent's: only their file is staged.
        std::fs::write(cwd.join("person.txt"), "a person's work\n").unwrap();
        git(cwd, &["add", "person.txt"]);
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=Pat", "-c", "user.email=pat@example.com", "commit", "-qm", "person: my work"])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(out.status.success());
        *seen.lock().unwrap() = git(cwd, &["rev-parse", "HEAD"]).trim().to_string();
    });
    let report = harden(&dir, Fake::new(2, fix)).await;

    let sha = person_sha.lock().unwrap().clone();
    assert!(!sha.is_empty(), "the person committed");
    assert!(is_ancestor(&dir, &sha), "the person's commit is still on the branch");
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]).trim(), sha, "HEAD was not moved by the pass");
    let why = report.stopped.expect("the pass stops");
    assert!(why.contains("Pat") && why.contains("not made by hexa"), "{why}");
    assert!(report.fixed.is_empty(), "nothing is claimed: {:?}", report.fixed);
    assert!(
        std::fs::read_to_string(dir.join(TARGET)).unwrap().contains("fix 1"),
        "the agent's uncommitted edit is left for the person to judge"
    );
    assert!(!subjects(&dir).iter().any(|s| s.contains("fix(harden)")), "{:?}", subjects(&dir));
}

/// A commit under hexa's own identity is folded back and gated, as before.
#[tokio::test]
async fn a_commit_under_hexas_identity_is_folded_back_and_gated() {
    let (_tmp, dir) = repo();
    let fix: FixAction = Arc::new(|cwd: &Path, n: usize| {
        good_fix()(cwd, n);
        commit_as(cwd, "hexa-factory", "agent's own commit");
    });
    // `commit_as` uses `<who>@example.com`; hexa's identity is its name.
    let report = harden(&dir, Fake::new(1, fix)).await;

    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    assert_eq!(subjects(&dir), vec!["fix(harden): bug-1", "init"]);
}

// ── (d) never reset away a commit hexa did not make ──────────────────────────

/// With a fast gate, the batch's full gate is red and bisection would reset
/// the branch to the batch's base. A person committed while that gate ran.
/// The pass stops instead of resetting, and the person's commit survives.
#[tokio::test]
async fn a_commit_made_between_batches_is_never_dropped() {
    let (tmp, dir) = repo();
    let marker = tmp.path().join("committed");
    let full = format!(
        "if [ ! -f '{m}' ]; then touch '{m}'; echo work > person.txt; git add person.txt; \
         git -c user.name=Pat -c user.email=pat@example.com commit -qm 'person: between batches'; fi; \
         if grep -rq POISON src; then echo 'test result: FAILED. 0 passed; 1 failed;'; exit 1; fi; \
         echo 'test result: ok. 1 passed; 0 failed;'",
        m = marker.display()
    );
    let fast = "echo 'test result: ok. 1 passed; 0 failed;'";
    let fix: FixAction = Arc::new(|cwd: &Path, n: usize| {
        good_fix()(cwd, n);
        if n == 2 {
            std::fs::write(cwd.join("src/poison.txt"), "POISON\n").unwrap();
        }
    });
    let report = run_review_gated(Fake::new(2, fix), hexa_exec::default_git_history(), TARGET, &gates(&full, Some(fast)), &dir).await;

    let log = subjects(&dir);
    assert!(log.contains(&"person: between batches".to_string()), "the person's commit is on the branch: {log:?}");
    let why = report.stopped.expect("the pass stops rather than reset");
    assert!(why.contains("Pat") && why.contains("not made by hexa"), "{why}");
}

// ── (e) never destroy bytes the agent did not write ─────────────────────────

/// While the agent works, a person edits `notes.txt`. The fix fails its gate
/// and is reverted. The person's bytes are saved under `.hexa/harden-rescue/`
/// first, and the report says where.
#[tokio::test]
async fn a_file_edited_beside_a_failed_fix_is_rescued_before_it_is_reverted() {
    let (_tmp, dir) = repo();
    let fix: FixAction = Arc::new(|cwd: &Path, _n: usize| {
        std::fs::write(cwd.join(TARGET), "original\nBROKEN\n").unwrap();
        std::fs::write(cwd.join("notes.txt"), "notes\na person's line\n").unwrap();
    });
    let report = harden(&dir, Fake::new(1, fix)).await;

    assert!(report.fixed.is_empty(), "{:?}", report.notes);
    let rescued: Vec<_> = walkdir(dir.join(".hexa/harden-rescue")).into_iter().filter(|p| p.ends_with("notes.txt")).collect();
    assert_eq!(rescued.len(), 1, "notes.txt was saved before the revert: {:?}", report.notes);
    assert_eq!(std::fs::read_to_string(&rescued[0]).unwrap(), "notes\na person's line\n");
    assert!(report.notes.iter().any(|n| n.contains(".hexa/harden-rescue/")), "the report says where: {:?}", report.notes);
}

/// The agent edits the target; then, while the gate runs, a person adds a
/// line to it too. The fix fails its gate. The target now holds bytes the
/// agent never wrote, so it is not reverted: it is left as it is, and named.
#[tokio::test]
async fn a_file_changed_after_the_agent_returned_is_not_reverted() {
    let (_tmp, dir) = repo();
    let fix: FixAction = Arc::new(|cwd: &Path, _n: usize| {
        std::fs::write(cwd.join(TARGET), "original\nBROKEN\n").unwrap();
    });
    let gate = format!("echo person >> {TARGET}; {GATE}");
    let report =
        run_review_gated(Fake::new(1, fix), hexa_exec::default_git_history(), TARGET, &gates(&gate, None), &dir).await;

    let body = std::fs::read_to_string(dir.join(TARGET)).unwrap();
    assert!(body.contains("person"), "the person's line is still there: {body:?}");
    assert!(
        report.notes.iter().any(|n| n.contains(TARGET) && n.contains("not reverted")),
        "the report names the file it left alone: {:?}",
        report.notes
    );
}

fn walkdir(root: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}
