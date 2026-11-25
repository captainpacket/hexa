//! hexa-git — pure git plumbing (status, log, diff, blame, worktree, commit
//! correlation, and history: reset the current branch to a commit, cherry-pick
//! a commit onto it — ADR-2610160756) over libgit2 + the git CLI. No
//! daemon/runtime dependencies, so
//! it is reusable outside hexa-nexus (ADR-2606071340 P1). Daemon-coupled adapters
//! (the git poller that pushes to SharedState/websocket, and the swarm-task
//! timeline join) stay in hexa-nexus and consume this crate.

pub mod history;
pub mod release;
pub mod worktree;

use std::path::{Path, PathBuf};

/// Every tag whose history contains `commit`, or `None` when `repo` is not a
/// repository or holds no such commit (ADR-2611250845, decision 6). A caller
/// asks here; which adapter answers is this crate's wiring.
pub fn tags_containing(repo: &Path, commit: &str) -> Option<Vec<String>> {
    release::tags_containing(repo, commit)
}

/// Validates that `root_path` is a real git repository and returns its canonical
/// working directory (or the repo path for a bare repo). Returns an error string
/// if invalid.
pub fn validate_repo_path(root_path: &str) -> Result<PathBuf, String> {
    let p = Path::new(root_path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", root_path));
    }
    git2::Repository::open(p)
        .map(|repo| repo.workdir().unwrap_or(repo.path()).to_path_buf())
        .map_err(|e| format!("Not a git repository: {} ({})", root_path, e))
}
