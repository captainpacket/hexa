//! ADR-2611160730 §4, through the built binary: the hooks see what agents
//! really use. An audit found 33 of 35 source edits in one session went
//! through Bash (`python3` heredoc patches, `sed -i`, `cat >`), and the
//! `pre-bash` hook only ever warned. Here a write-shaped Bash command aimed at
//! tracked project source is refused with exit 2 unless a gate is recorded
//! for the current task, and read-only commands, test runners, scratch writes
//! and build output are never refused.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SESSION: &str = "pre-bash-test";

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A hexa project in a git repo: `.hexa/project.json` (mandatory by default),
/// a tracked `src/x.rs`, an ignored `target/`.
fn project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(repo.join(".hexa")).unwrap();
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    std::fs::write(repo.join("src/x.rs"), "fn a() {}\n").unwrap();
    std::fs::write(repo.join("docs/notes.md"), "# notes\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    std::fs::write(repo.join(".hexa/project.json"), r#"{"name":"p","id":"00000000-test"}"#).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "init"]);
    (tmp, home, repo)
}

fn hexa(home: &Path, repo: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
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
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn pre_bash(home: &Path, repo: &Path, command: &str) -> Output {
    let payload = serde_json::json!({
        "session_id": SESSION,
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": repo.to_str().unwrap(),
        "tool_input": { "command": command },
    });
    hexa(home, repo, &["hook", "pre-bash"], &payload.to_string())
}

fn record_gate(home: &Path, repo: &Path) {
    let o = hexa(home, repo, &["loop", "gate", "cargo test"], "");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn code(o: &Output) -> Option<i32> {
    o.status.code()
}

fn why(o: &Output) -> String {
    format!("stdout: {}\nstderr: {}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn sed_in_place_on_tracked_source_with_no_gate_is_refused_and_names_the_file() {
    let (_t, home, repo) = project();
    let o = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(code(&o), Some(2), "{}", why(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("src/x.rs"), "the refusal names the file: {err}");
    assert!(err.contains("hexa loop gate"), "the refusal says how to proceed: {err}");
}

#[test]
fn sed_in_place_with_a_gate_recorded_this_session_is_allowed_and_touches_the_target() {
    let (_t, home, repo) = project();
    record_gate(&home, &repo);
    let o = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
    // The touched-files list records the file the command edits, not a script.
    let loop_json = std::fs::read_to_string(repo.join(".hexa/loop.json")).unwrap();
    assert!(loop_json.contains("\"src/x.rs\""), "the target is recorded as touched: {loop_json}");
}

#[test]
fn a_gate_recorded_before_the_last_task_was_done_does_not_cover_the_next() {
    let (_t, home, repo) = project();
    record_gate(&home, &repo);
    let add = hexa(&home, &repo, &["loop", "task", "add", "first step"], "");
    assert!(add.status.success(), "{}", why(&add));
    // Timestamps are compared; make the order unambiguous.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let done = hexa(&home, &repo, &["loop", "task", "done", "1"], "");
    assert!(done.status.success(), "{}", why(&done));
    let o = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(code(&o), Some(2), "a stale gate must not cover new work: {}", why(&o));
    // Re-confirming the gate covers the next step.
    std::thread::sleep(std::time::Duration::from_millis(20));
    record_gate(&home, &repo);
    let o = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
}

#[test]
fn cargo_test_is_never_refused() {
    let (_t, home, repo) = project();
    for cmd in ["cargo test", "cargo test -p x 2>&1 | tail -20", "git status", "grep -rn foo src", "node --test", "npm test"] {
        let o = pre_bash(&home, &repo, cmd);
        assert_eq!(code(&o), Some(0), "{cmd}: {}", why(&o));
    }
}

#[test]
fn a_python_heredoc_that_writes_tracked_source_is_refused() {
    let (_t, home, repo) = project();
    let o = pre_bash(&home, &repo, "python3 - <<'EOF'\nopen('src/x.rs','w').write('x')\nEOF");
    assert_eq!(code(&o), Some(2), "{}", why(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("src/x.rs"), "{}", why(&o));
}

#[test]
fn a_python_script_file_that_writes_tracked_source_is_refused() {
    let (t, home, repo) = project();
    let script = t.path().join("patch.py");
    std::fs::write(&script, "p = 'src/x.rs'\ns = open(p).read()\nwith open(p, 'w') as f:\n    f.write(s.replace('a', 'b'))\n").unwrap();
    let o = pre_bash(&home, &repo, &format!("python3 {}", script.display()));
    assert_eq!(code(&o), Some(2), "{}", why(&o));
}

#[test]
fn a_scratch_write_outside_the_repo_is_allowed() {
    let (_t, home, repo) = project();
    let o = pre_bash(&home, &repo, "cat > /tmp/scratch.txt");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
}

#[test]
fn a_write_into_build_output_is_allowed() {
    let (_t, home, repo) = project();
    let o = pre_bash(&home, &repo, "echo hi > target/foo");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
}

#[test]
fn redirection_tee_and_cp_onto_tracked_source_are_refused() {
    let (_t, home, repo) = project();
    for cmd in [
        "cat > src/x.rs <<'EOF'\nfn b() {}\nEOF",
        "echo 'fn c() {}' >> src/x.rs",
        "echo x | tee -a src/x.rs",
        "cp /tmp/y.rs src/x.rs",
        "cd src && sed -i '' 's/a/b/' x.rs",
        "perl -pi -e 's/a/b/' src/x.rs",
    ] {
        let o = pre_bash(&home, &repo, cmd);
        assert_eq!(code(&o), Some(2), "{cmd}: {}", why(&o));
    }
}

#[test]
fn docs_and_hexa_itself_are_allowed() {
    let (_t, home, repo) = project();
    for cmd in ["echo more >> docs/notes.md", "hexa loop gate 'cargo test'", "hexa do run 'x' --file src/x.rs --evidence 'cargo test'"] {
        let o = pre_bash(&home, &repo, cmd);
        assert_eq!(code(&o), Some(0), "{cmd}: {}", why(&o));
    }
}

#[test]
fn advisory_mode_warns_instead_of_refusing() {
    let (_t, home, repo) = project();
    std::fs::write(
        repo.join(".hexa/project.json"),
        r#"{"name":"p","id":"00000000-test","lifecycle_enforcement":"advisory"}"#,
    )
    .unwrap();
    let o = pre_bash(&home, &repo, "sed -i s/a/b/ src/x.rs");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("src/x.rs"), "the warning names the file: {}", why(&o));
}

#[test]
fn pre_edit_holds_every_source_edit_to_the_current_gate_but_not_docs() {
    let (_t, home, repo) = project();
    let edit = |f: &str| {
        let payload = serde_json::json!({
            "session_id": SESSION,
            "tool_name": "Edit",
            "tool_input": { "file_path": repo.join(f).to_str().unwrap() },
        });
        hexa(&home, &repo, &["hook", "pre-edit"], &payload.to_string())
    };
    // No prompt was sized at all: the check still runs for source.
    let o = edit("src/x.rs");
    assert_eq!(code(&o), Some(2), "{}", why(&o));
    let o = edit("docs/notes.md");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
    let o = edit("README.md");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
    record_gate(&home, &repo);
    let o = edit("src/x.rs");
    assert_eq!(code(&o), Some(0), "{}", why(&o));
}

#[test]
fn init_strict_merges_deny_and_allow_without_clobbering_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    std::fs::write(
        repo.join(".claude/settings.json"),
        r#"{"model":"keep-me","permissions":{"allow":["Bash(ls)"],"deny":["Read(./secret)"]}}"#,
    )
    .unwrap();
    let init = |extra: &[&str]| {
        let mut args = vec!["init", repo.to_str().unwrap(), "--strict"];
        args.extend_from_slice(extra);
        hexa(&home, &repo, &args, "")
    };
    let o = init(&[]);
    assert!(o.status.success(), "{}", why(&o));
    // A second run on an initialized project applies --strict again, no --force needed.
    let o = init(&[]);
    assert!(o.status.success(), "{}", why(&o));
    let s: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(s["model"], "keep-me", "{s}");
    let list = |k: &str| -> Vec<String> {
        s["permissions"][k].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
    };
    let deny = list("deny");
    let allow = list("allow");
    for want in ["Edit(src/**)", "Write(src/**)", "Read(./secret)"] {
        assert_eq!(deny.iter().filter(|d| *d == want).count(), 1, "{want} exactly once in {deny:?}");
    }
    for want in ["Bash(hexa do *)", "Bash(ls)"] {
        assert_eq!(allow.iter().filter(|d| *d == want).count(), 1, "{want} exactly once in {allow:?}");
    }
    assert!(s["hooks"]["PreToolUse"].is_array(), "init still installs the hooks: {s}");
}
