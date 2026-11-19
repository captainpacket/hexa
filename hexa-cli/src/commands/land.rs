//! Landing a gated commit on the base branch (ADR-2611161800 §4).
//!
//! Integration is continuous: a commit `hexa do gate` makes on a branch goes
//! onto the base as soon as it passes. The branch is rebased onto the base,
//! the gate re-runs if the rebase moved anything, and the base is
//! fast-forwarded. Only ever a fast-forward, never forced, and never a push:
//! a push can need someone's approval, so it stays a step of its own, and
//! `hexa finish` lists the commits not pushed. When the rebase conflicts, the
//! gate fails on the base, or the base's checkout has uncommitted changes, the
//! commit stays on its branch and the reason is returned.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git").args(args).current_dir(dir).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// How a landing went.
#[derive(Debug, PartialEq)]
pub enum Landing {
    /// Already on the base: nothing to land.
    OnBase,
    /// The base now points at this commit.
    Landed { base: String, sha: String },
    /// The commit stays on its branch, for this reason.
    NotLanded { base: String, why: String },
}

/// The worktree that has `branch` checked out, if any.
fn checkout_of(dir: &Path, branch: &str) -> Option<PathBuf> {
    let listing = git(dir, &["worktree", "list", "--porcelain"]).ok()?;
    let mut path = None;
    for line in listing.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if line == format!("branch refs/heads/{branch}") {
            return path;
        }
    }
    None
}

/// Land the current branch of the worktree at `root` on the base. `gate`
/// re-runs on the rebased tree when the rebase moved the branch: here, or
/// through test.remote unless `local` (ADR-2611191120 §4).
pub async fn land(root: &Path, gate: &str, uncounted: bool, local: bool) -> Landing {
    let base = crate::commands::finish::base_branch(root);
    let branch = match git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(b) => b,
        Err(_) => return Landing::NotLanded { base, why: "HEAD is detached: there is no branch to land".into() },
    };
    if branch == base {
        return Landing::OnBase;
    }
    let not = |why: String| Landing::NotLanded { base: base.clone(), why };
    let before = git(root, &["rev-parse", "HEAD"]).unwrap_or_default();
    // onto the base first, so what lands is what was gated against it
    if git(root, &["merge-base", "--is-ancestor", &base, "HEAD"]).is_err() {
        if let Err(e) = git(root, &["rebase", "--quiet", &base]) {
            let files = git(root, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default().replace('\n', ", ");
            let _ = git(root, &["rebase", "--abort"]);
            let what = if files.is_empty() { e.lines().next().unwrap_or("the rebase failed").to_string() } else { format!("it conflicts with {base} in {files}") };
            return not(format!("{branch} could not be rebased onto {base}: {what}; rebase it by hand, then land it with `hexa dev worktree merge {branch} --force`"));
        }
        let after = git(root, &["rev-parse", "HEAD"]).unwrap_or_default();
        if after != before {
            let ran = match hexa_exec::test_exec::test_config(root) {
                Ok(cfg) => hexa_exec::test_exec::run_gate(root, gate, &cfg, local).await,
                Err(e) => Err(e),
            };
            let (passed, output) = match ran {
                Ok(r) => r,
                Err(e) => return not(format!("the gate could not be re-run on {branch} rebased onto {base}: {e}")),
            };
            let verdict = hexa_core::ports::evidence::read_gate_evidence(gate, passed, &output, uncounted);
            if !verdict.passes() {
                return not(format!("the gate does not pass on {branch} rebased onto {base}; the rebased branch is kept for you to fix"));
            }
        }
    }
    let sha = git(root, &["rev-parse", "HEAD"]).unwrap_or_default();
    match checkout_of(root, &base) {
        Some(main_dir) => {
            let dirty = git(&main_dir, &["status", "--porcelain=v1", "--untracked-files=no"])
                .unwrap_or_default()
                .lines()
                .any(|l| l.len() > 3 && !l[3..].starts_with(".hexa/"));
            if dirty {
                return not(format!("the {base} checkout at {} has uncommitted changes, and landing never writes over them", main_dir.display()));
            }
            if let Err(e) = git(&main_dir, &["merge", "--quiet", "--ff-only", &sha]) {
                return not(format!("{base} could not be fast-forwarded: {e}"));
            }
        }
        None => {
            let old = git(root, &["rev-parse", &base]).unwrap_or_default();
            if let Err(e) = git(root, &["update-ref", &format!("refs/heads/{base}"), &sha, &old]) {
                return not(format!("{base} could not be moved: {e}"));
            }
        }
    }
    Landing::Landed { base, sha: sha.chars().take(7).collect() }
}
