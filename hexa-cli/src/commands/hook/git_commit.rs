//! Which `git commit` commands would commit project source by hand
//! (ADR-2611160910 finding 1).
//!
//! The usage eval's first baseline found that no commit went through hexa:
//! in every scenario that ended in a commit, the model ran `git add <file> &&
//! git commit` itself, never `hexa do gate`, and nothing refused it. This
//! module reads a command line closely enough to say what a `git commit` in
//! it would include: what is staged, what `-a`/`--all` adds, the paths named
//! on the commit, and what a `git add` earlier in the same line stages, which
//! has not run yet when the hook sees the line. Whether those paths are
//! project source is [`super::source_gate`]'s decision.
//!
//! It is a classifier, not git. [`commits`] is pure; [`included_paths`] asks
//! git what is staged and changed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What one `git commit` in a command line would include, as far as the line
/// says.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CommitPlan {
    /// Where git runs: the command's directory, after `cd` and `git -C`.
    pub dir: PathBuf,
    /// `-a`/`--all`: every modified tracked file.
    pub all: bool,
    /// `--amend`. What it includes is what is staged, as for any commit.
    pub amend: bool,
    /// `-m`/`--message`, when given: reused in the `do gate` suggestion.
    pub message: Option<String>,
    /// Paths named on the commit, or staged by a `git add` before it in the
    /// same line.
    pub paths: Vec<PathBuf>,
    /// A `git add -u`/`-A` (or `add .`) earlier in the line: every change
    /// under `add_under` (tracked only for `-u`).
    pub add_tracked: bool,
    pub add_untracked: bool,
    pub add_under: Option<PathBuf>,
}

fn is_git(word: &str) -> bool {
    word.rsplit('/').next() == Some("git")
}

/// `git [global options] <sub> <args…>`: the directory after `-C`, the
/// subcommand, and its arguments.
fn split_git(dir: &Path, words: &[String]) -> Option<(PathBuf, String, Vec<String>)> {
    let mut dir = dir.to_path_buf();
    let mut i = 1;
    while i < words.len() {
        let w = words[i].as_str();
        match w {
            "-C" => {
                let d = words.get(i + 1)?;
                dir = super::write_shape::resolve_word(&dir, d)?;
                i += 2;
            }
            "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path" | "--config-env" => i += 2,
            _ if w.starts_with('-') => i += 1,
            _ => return Some((dir, w.to_string(), words[i + 1..].to_vec())),
        }
    }
    None
}

/// Short options of `git commit` that take a value: `-m msg`, `-mmsg`.
const COMMIT_SHORT_WITH_VALUE: &[char] = &['m', 'F', 'C', 'c', 't'];
/// Long options of `git commit` that take a value when it is not given with `=`.
const COMMIT_LONG_WITH_VALUE: &[&str] = &[
    "--message", "--file", "--reuse-message", "--reedit-message", "--template", "--author", "--date", "--cleanup",
    "--fixup", "--squash", "--trailer", "--pathspec-from-file",
];

/// One `git commit`'s arguments, or `None` when it commits nothing
/// (`--help`, `--dry-run`).
fn parse_commit(dir: PathBuf, args: &[String]) -> Option<CommitPlan> {
    let mut plan = CommitPlan { dir: dir.clone(), ..Default::default() };
    let mut i = 0;
    let mut only_paths = false;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        if only_paths || !a.starts_with('-') || a == "-" {
            plan.paths.extend(super::write_shape::resolve_word(&dir, a));
            continue;
        }
        if a == "--" {
            only_paths = true;
            continue;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (format!("--{n}"), Some(v.to_string())),
                None => (a.to_string(), None),
            };
            match name.as_str() {
                "--help" | "--dry-run" => return None,
                "--all" => plan.all = true,
                "--amend" => plan.amend = true,
                n if COMMIT_LONG_WITH_VALUE.contains(&n) => {
                    let v = match value {
                        Some(v) => Some(v),
                        None => {
                            i += 1;
                            args.get(i - 1).cloned()
                        }
                    };
                    if n == "--message" {
                        plan.message = v;
                    }
                }
                _ => {}
            }
            continue;
        }
        // A cluster of short options: `-am msg`, `-qam msg`, `-mmsg`.
        if a == "-h" {
            return None;
        }
        let chars: Vec<char> = a[1..].chars().collect();
        for (k, c) in chars.iter().enumerate() {
            if *c == 'a' {
                plan.all = true;
                continue;
            }
            if COMMIT_SHORT_WITH_VALUE.contains(c) {
                let rest: String = chars[k + 1..].iter().collect();
                let v = if rest.is_empty() {
                    i += 1;
                    args.get(i - 1).cloned()
                } else {
                    Some(rest)
                };
                if *c == 'm' {
                    plan.message = v;
                }
                break;
            }
        }
    }
    Some(plan)
}

/// Every `git commit` in `command`, run from `cwd`, with what a `git add`
/// earlier in the same line stages folded in. A command run by `hexa` is
/// never one: `hexa do gate` and the verbs commit for themselves.
pub fn commits(command: &str, cwd: &Path) -> Vec<CommitPlan> {
    let mut out = Vec::new();
    let mut added: Vec<PathBuf> = Vec::new();
    let (mut tracked, mut untracked, mut under): (bool, bool, Option<PathBuf>) = (false, false, None);
    for (dir, words) in super::write_shape::command_words(command, cwd) {
        if !words.first().is_some_and(|w| is_git(w)) {
            continue;
        }
        let Some((dir, sub, args)) = split_git(&dir, &words) else { continue };
        match sub.as_str() {
            "add" | "stage" => {
                let mut only_paths = false;
                for a in &args {
                    match a.as_str() {
                        "--" => only_paths = true,
                        "-A" | "--all" if !only_paths => {
                            tracked = true;
                            untracked = true;
                            under = None;
                        }
                        "-u" | "--update" if !only_paths => tracked = true,
                        "." | ":/" | ":" => {
                            tracked = true;
                            untracked = true;
                            under = if a == "." { Some(dir.clone()) } else { None };
                        }
                        _ if a.starts_with('-') && !only_paths => {}
                        _ => added.extend(super::write_shape::resolve_word(&dir, a)),
                    }
                }
            }
            "commit" => {
                if let Some(mut plan) = parse_commit(dir, &args) {
                    plan.paths.extend(added.iter().cloned());
                    plan.add_tracked = tracked;
                    plan.add_untracked = untracked;
                    plan.add_under = under.clone();
                    out.push(plan);
                }
            }
            _ => {}
        }
    }
    out
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

/// True while git is replaying commits that already exist: a merge, a
/// cherry-pick, a revert or a rebase stopped for a conflict. A commit then
/// records a resolution, not new work, and is never refused.
pub fn replay_in_progress(dir: &Path) -> bool {
    ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply"].iter().any(|name| {
        git(dir, &["rev-parse", "--git-path", name])
            .map(|p| {
                let p = PathBuf::from(p.trim());
                if p.is_absolute() { p } else { dir.join(p) }
            })
            .is_some_and(|p| p.exists())
    })
}

/// The files `plan` would commit, as absolute paths under `root`: what is
/// staged, plus what the line itself adds or names, among the files git sees
/// as changed. Paths named on the line that git does not see as changed are
/// left out; git would commit nothing for them.
pub fn included_paths(root: &Path, plan: &CommitPlan) -> Vec<PathBuf> {
    let lines = |args: &[&str]| -> Vec<String> {
        git(root, args).map(|s| s.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
    };
    let staged = lines(&["diff", "--cached", "--name-only", "--no-renames"]);
    let modified = lines(&["diff", "--name-only", "--no-renames"]);
    let new = lines(&["ls-files", "--others", "--exclude-standard"]);
    let mut out: Vec<PathBuf> = staged.iter().map(|r| root.join(r)).collect();
    if plan.all {
        out.extend(modified.iter().map(|r| root.join(r)));
    }
    // What a `git add -u`/`-A`/`.` earlier in the line stages, under its
    // directory when it was `add .`.
    let under = plan.add_under.as_deref().map(super::source_gate::canonical_lenient);
    let in_scope = |p: &PathBuf| under.as_ref().is_none_or(|u| p.starts_with(u));
    if plan.add_tracked {
        out.extend(modified.iter().map(|r| root.join(r)).filter(in_scope));
    }
    if plan.add_untracked {
        out.extend(new.iter().map(|r| root.join(r)).filter(in_scope));
    }
    let named: Vec<PathBuf> = plan.paths.iter().map(|p| super::source_gate::canonical_lenient(p)).collect();
    for rel in modified.iter().chain(new.iter()) {
        let abs = root.join(rel);
        if named.iter().any(|n| abs == *n || abs.starts_with(n)) {
            out.push(abs);
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// `s` quoted for a POSIX shell in single quotes.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(cmd: &str) -> CommitPlan {
        let v = commits(cmd, Path::new("/r"));
        assert_eq!(v.len(), 1, "{cmd}: {v:?}");
        v.into_iter().next().unwrap()
    }

    #[test]
    fn options_and_clusters_are_read_as_git_reads_them() {
        let p = one("git commit -am 'fix: b'");
        assert!(p.all && !p.amend && p.paths.is_empty(), "{p:?}");
        assert_eq!(p.message.as_deref(), Some("fix: b"));
        let p = one("git commit -mfix");
        assert!(!p.all);
        assert_eq!(p.message.as_deref(), Some("fix"));
        let p = one("git -C sub commit --amend --no-edit");
        assert!(p.amend && !p.all);
        assert_eq!(p.dir, PathBuf::from("/r/sub"));
        let p = one("git commit --message=x --author 'A <a@x>' -- src/x.rs");
        assert_eq!(p.message.as_deref(), Some("x"));
        assert_eq!(p.paths, vec![PathBuf::from("/r/src/x.rs")]);
        let p = one("cd crate && git commit -qam b src/y.rs");
        assert!(p.all);
        assert_eq!(p.paths, vec![PathBuf::from("/r/crate/src/y.rs")]);
    }

    #[test]
    fn an_add_earlier_in_the_line_is_folded_in() {
        let p = one("git add src/x.rs docs/a.md && git commit -m x");
        assert_eq!(p.paths, vec![PathBuf::from("/r/src/x.rs"), PathBuf::from("/r/docs/a.md")]);
        let p = one("git add -A; git commit -m x");
        assert!(p.add_tracked && p.add_untracked && p.add_under.is_none());
        let p = one("git add -u && git commit -m x");
        assert!(p.add_tracked && !p.add_untracked);
        let p = one("cd sub && git add . && git commit -m x");
        assert_eq!(p.add_under, Some(PathBuf::from("/r/sub")));
    }

    #[test]
    fn what_is_not_a_commit_is_not_one() {
        for cmd in [
            "git status",
            "git log --oneline",
            "echo git commit",
            "git commit --help",
            "git commit --dry-run -a",
            "hexa do gate --files src/x.rs -e 'cargo test' -m 'git commit'",
            "grep -rn 'git commit' docs",
        ] {
            assert!(commits(cmd, Path::new("/r")).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn quoting_survives_a_single_quote() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }
}
