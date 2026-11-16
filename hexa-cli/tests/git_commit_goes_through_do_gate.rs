//! The usage eval's first finding (ADR-2611160910, baseline 20261116T144539Z):
//! no commit went through hexa. In every scenario that ended in a commit the
//! model ran `git add … && git commit` itself, never `hexa do gate`, and
//! nothing refused it. Here, through the built binary, the `pre-bash` hook
//! refuses a `git commit` that would commit project source and names the
//! `hexa do gate` command that does it instead, filled in from the paths and
//! the loop's gate. Commits of prose, a merge in progress, and hexa's own
//! commits are never refused.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SESSION: &str = "commit-hook-test";

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A hexa project in a git repo: a tracked `src/x.rs` and `docs/notes.md`.
fn project(project_json: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(repo.join(".hexa")).unwrap();
    std::fs::create_dir_all(repo.join("docs/adrs")).unwrap();
    std::fs::write(repo.join("src/x.rs"), "fn a() {}\n").unwrap();
    std::fs::write(repo.join("docs/notes.md"), "# notes\n").unwrap();
    std::fs::write(repo.join("README.md"), "# p\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "target/\n.hexa/loop.json\n.hexa/loop.lock\n").unwrap();
    std::fs::write(repo.join(".hexa/project.json"), project_json).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "init"]);
    (tmp, home, repo)
}

fn plain() -> (tempfile::TempDir, PathBuf, PathBuf) {
    project(r#"{"name":"p","id":"00000000-test"}"#)
}

fn hexa_env(home: &Path, repo: &Path, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hexa"));
    cmd.args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", repo)
        .env("CLAUDE_CODE_SESSION_ID", SESSION)
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env_remove("HEXA_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("HEXA_INTERNAL")
        .env_remove("HEXA_TASK")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn pre_bash_env(home: &Path, repo: &Path, command: &str, env: &[(&str, &str)]) -> Output {
    let payload = serde_json::json!({
        "session_id": SESSION,
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": repo.to_str().unwrap(),
        "tool_input": { "command": command },
    });
    hexa_env(home, repo, &["hook", "pre-bash"], &payload.to_string(), env)
}

fn pre_bash(home: &Path, repo: &Path, command: &str) -> Output {
    pre_bash_env(home, repo, command, &[])
}

fn why(o: &Output) -> String {
    format!("stdout: {}\nstderr: {}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn committing_a_staged_source_file_with_no_gate_is_refused_and_names_do_gate() {
    let (_t, home, repo) = plain();
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "src/x.rs"]);
    let o = pre_bash(&home, &repo, "git commit -m 'fix: b'");
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
    let e = err(&o);
    assert!(e.contains("hexa do gate --files src/x.rs"), "names the do gate command with the file: {e}");
    assert!(e.contains("-e '"), "with the evidence flag: {e}");
    assert!(e.contains("hexa loop gate"), "with no gate recorded, says how to record one: {e}");
}

#[test]
fn the_refusal_fills_in_the_recorded_gate() {
    let (_t, home, repo) = plain();
    let g = hexa_env(&home, &repo, &["loop", "gate", "cargo test --test x"], "", &[]);
    assert!(g.status.success(), "{}", why(&g));
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "src/x.rs"]);
    let o = pre_bash(&home, &repo, "git commit -m 'fix: b'");
    assert_eq!(o.status.code(), Some(2), "a recorded gate does not make a hand commit ok: {}", why(&o));
    assert!(err(&o).contains("hexa do gate --files src/x.rs -e 'cargo test --test x'"), "{}", why(&o));
}

#[test]
fn a_declared_full_gate_is_the_one_named() {
    let (_t, home, repo) = project(r#"{"name":"p","id":"00000000-test","gate":{"full":"cargo test"}}"#);
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "src/x.rs"]);
    let o = pre_bash(&home, &repo, "git commit -m 'fix: b'");
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
    assert!(err(&o).contains("-e 'cargo test'"), "{}", why(&o));
}

#[test]
fn add_then_commit_in_one_command_is_seen_before_the_add_runs() {
    let (_t, home, repo) = plain();
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    // Nothing is staged yet: the hook runs before the command does.
    let o = pre_bash(&home, &repo, "git add src/x.rs && git commit -m 'fix: b'");
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
    assert!(err(&o).contains("--files src/x.rs"), "{}", why(&o));
}

#[test]
fn commit_all_and_amend_count_the_files_they_include() {
    let (_t, home, repo) = plain();
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    for cmd in ["git commit -am 'fix: b'", "git commit -a -m 'fix: b'", "git commit --all -m b", "git -C . commit -qam b"] {
        let o = pre_bash(&home, &repo, cmd);
        assert_eq!(o.status.code(), Some(2), "{cmd}: {}", why(&o));
        assert!(err(&o).contains("src/x.rs"), "{cmd}: {}", why(&o));
    }
    git(&repo, &["add", "src/x.rs"]);
    let o = pre_bash(&home, &repo, "git commit --amend --no-edit");
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
    // A path named on the command line is committed whatever is staged.
    git(&repo, &["reset", "-q"]);
    let o = pre_bash(&home, &repo, "git commit -m b -- src/x.rs");
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
}

#[test]
fn a_commit_of_prose_only_passes() {
    let (_t, home, repo) = plain();
    std::fs::write(repo.join("docs/notes.md"), "# more notes\n").unwrap();
    std::fs::write(repo.join("README.md"), "# p, again\n").unwrap();
    std::fs::write(repo.join("docs/adrs/ADR-1-x.md"), "# ADR-1\n").unwrap();
    git(&repo, &["add", "docs", "README.md"]);
    let o = pre_bash(&home, &repo, "git commit -m 'docs: notes'");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
    let o = pre_bash(&home, &repo, "git add docs/notes.md README.md && git commit -m 'docs: notes'");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
    // -a with only prose modified, and source untouched.
    let o = pre_bash(&home, &repo, "git commit -am 'docs'");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
}

#[test]
fn a_merge_in_progress_passes() {
    let (_t, home, repo) = plain();
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "src/x.rs"]);
    let head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&repo).output().unwrap();
    std::fs::write(repo.join(".git/MERGE_HEAD"), head.stdout).unwrap();
    let o = pre_bash(&home, &repo, "git commit --no-edit");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
}

#[test]
fn hexa_itself_and_hexa_internal_pass() {
    let (_t, home, repo) = plain();
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "src/x.rs"]);
    let o = pre_bash(&home, &repo, "hexa do gate --files src/x.rs -e 'cargo test' -m 'fix: b'");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
    let o = pre_bash_env(&home, &repo, "git commit -m 'fix: b'", &[("HEXA_INTERNAL", "1")]);
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
}

#[test]
fn nothing_to_commit_and_unrelated_git_pass() {
    let (_t, home, repo) = plain();
    for cmd in ["git commit -m nothing", "git log --oneline -3", "git status", "echo git commit", "git commit --help"] {
        let o = pre_bash(&home, &repo, cmd);
        assert_eq!(o.status.code(), Some(0), "{cmd}: {}", why(&o));
    }
}

#[test]
fn advisory_mode_warns_and_names_do_gate() {
    let (_t, home, repo) = project(r#"{"name":"p","id":"00000000-test","lifecycle_enforcement":"advisory"}"#);
    std::fs::write(repo.join("src/x.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["add", "src/x.rs"]);
    let o = pre_bash(&home, &repo, "git commit -m 'fix: b'");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
    assert!(why(&o).contains("hexa do gate --files src/x.rs"), "{}", why(&o));
}
