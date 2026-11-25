//! Which tags hold a commit: what `hexa adr features` asks to say the release
//! a feature first shipped in (ADR-2611250845, decision 6).
//!
//! Reads only. Uses the git CLI, like `history.rs`.

use std::path::Path;
use std::process::Command;

/// A commit id as it may be given to git here: hex digits only, so it is
/// never read as an option or a revision expression.
fn is_commit_id(s: &str) -> bool {
    (4..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Every tag whose history contains `commit`, or `None` when that cannot be
/// told: `repo` is not a repository, git is absent, or `repo` holds no such
/// commit (an id that names nothing, or more than one object).
pub fn tags_containing(repo: &Path, commit: &str) -> Option<Vec<String>> {
    if !is_commit_id(commit) {
        return None;
    }
    let spec = format!("{commit}^{{commit}}");
    let full = git(repo, &["rev-parse", "--verify", "--quiet", &spec])?;
    let tags = git(repo, &["tag", "--contains", full.trim()])?;
    Some(tags.lines().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn the_tags_that_hold_a_commit_and_none_for_what_is_not_there() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        run(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a"), "1").unwrap();
        run(dir, &["add", "-A"]);
        run(dir, &["commit", "-q", "-m", "one"]);
        run(dir, &["tag", "v0.1.0"]);
        std::fs::write(dir.join("a"), "2").unwrap();
        run(dir, &["commit", "-q", "-am", "two"]);
        let two = run(dir, &["rev-parse", "--short", "HEAD"]);
        run(dir, &["tag", "v0.2.0"]);
        run(dir, &["tag", "nightly"]);
        std::fs::write(dir.join("a"), "3").unwrap();
        run(dir, &["commit", "-q", "-am", "three"]);
        let three = run(dir, &["rev-parse", "HEAD"]);

        let mut tags = tags_containing(dir, &two).unwrap();
        tags.sort();
        assert_eq!(tags, ["nightly", "v0.2.0"]);
        assert_eq!(tags_containing(dir, &three), Some(Vec::new()), "a commit no tag holds");
        assert_eq!(tags_containing(dir, "abcdef0123456"), None, "a commit that is not here");
        assert_eq!(tags_containing(dir, "--all"), None);
        assert_eq!(tags_containing(tmp.path().join("nowhere").as_path(), &two), None);
    }
}
