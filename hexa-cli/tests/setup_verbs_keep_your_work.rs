//! The setup verbs never commit, delete or reset what a person already has.
//!
//! An audit ran each of these in a scratch directory and lost work:
//!
//! - `hexa init` in an existing repo ran `git add -A` and committed an
//!   untracked `.env` holding `API_KEY=x`, and a WIP file, as hexa-factory.
//! - `hexa init apps/sub` inside a repo checked only `apps/sub/.git`, so it
//!   nested a second repository.
//! - `hexa init` replaced the whole `hooks` object in `.claude/settings.json`,
//!   deleting the user's own hooks.
//! - `hexa init --force` rewrote `.hexa/project.json` with a fresh id and lost
//!   the configured models and `analyze.exclude`.
//! - `hexa dev worktree cleanup --force` removed unmerged worktrees older than
//!   a day with `git worktree remove --force`, deleting uncommitted files.
//!
//! Each test builds a real git repository in a temp dir and runs the binary.

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    out
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let out = git(dir, args);
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn hexa(dir: &Path, args: &[&str]) -> (bool, String) {
    let home = dir.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(dir)
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env("HEXA_HOME", home.join(".hexa"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("hexa runs");
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

/// A repository on `main` with one commit, and `.test-home` ignored so the
/// test's own HOME is never mistaken for the user's work.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_ok(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    std::fs::write(dir.path().join(".git/info/exclude"), ".test-home/\n").unwrap();
    git_ok(dir.path(), &["add", "README.md"]);
    git_ok(dir.path(), &["commit", "-q", "-m", "first"]);
    dir
}

fn commit_count(dir: &Path) -> String {
    git_ok(dir, &["rev-list", "--count", "HEAD"]).trim().to_string()
}

// ── init in an existing repo ──────────────────────────────────────────────

#[test]
fn init_in_an_existing_repo_commits_neither_your_secrets_nor_your_wip() {
    let dir = repo();
    let d = dir.path();
    std::fs::write(d.join(".env"), "API_KEY=x\n").unwrap();
    std::fs::write(d.join("wip.rs"), "fn half_done() {}\n").unwrap();

    let (ok, text) = hexa(d, &["init"]);
    assert!(ok, "init failed:\n{text}");

    assert_eq!(commit_count(d), "1", "init made a commit in an existing repo:\n{text}");
    let index = git_ok(d, &["ls-files", "--cached"]);
    assert!(!index.lines().any(|l| l == ".env"), ".env was staged:\n{index}");
    assert!(!index.lines().any(|l| l == "wip.rs"), "wip.rs was staged:\n{index}");
    assert_eq!(index.trim(), "README.md", "init staged files in an existing repo:\n{index}");
    assert_eq!(std::fs::read_to_string(d.join(".env")).unwrap(), "API_KEY=x\n");
    assert_eq!(std::fs::read_to_string(d.join("wip.rs")).unwrap(), "fn half_done() {}\n");

    // .env is ignored from now on, and .env.example is not.
    assert!(git(d, &["check-ignore", "-q", ".env"]).status.success(), ".env is not ignored");
    assert!(git(d, &["check-ignore", "-q", ".env.local"]).status.success(), ".env.local is not ignored");
    assert!(!git(d, &["check-ignore", "-q", ".env.example"]).status.success(), ".env.example is ignored");
}

#[test]
fn init_in_a_subdirectory_of_a_repo_nests_no_repository() {
    let dir = repo();
    let d = dir.path();
    std::fs::create_dir_all(d.join("apps/sub")).unwrap();

    let (ok, text) = hexa(d, &["init", "apps/sub"]);
    assert!(ok, "init failed:\n{text}");

    assert!(!d.join("apps/sub/.git").exists(), "init nested a .git inside a repo:\n{text}");
    assert!(d.join("apps/sub/.hexa/project.json").is_file());
    assert_eq!(commit_count(d), "1", "init committed in the enclosing repo:\n{text}");
}

#[test]
fn init_in_a_fresh_directory_commits_only_what_it_wrote_skills_included() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join(".env"), "API_KEY=x\n").unwrap();
    std::fs::write(d.join("notes.txt"), "mine\n").unwrap();

    let (ok, text) = hexa(d, &["init"]);
    assert!(ok, "init failed:\n{text}");
    assert!(d.join(".git").is_dir(), "a fresh directory gets a repository:\n{text}");
    assert_eq!(commit_count(d), "1");

    let tracked = git_ok(d, &["ls-files"]);
    assert!(!tracked.lines().any(|l| l == ".env"), ".env was committed:\n{tracked}");
    assert!(!tracked.lines().any(|l| l == "notes.txt"), "a file init did not write was committed:\n{tracked}");
    assert!(!tracked.lines().any(|l| l.starts_with(".test-home")), "{tracked}");
    for want in [".hexa/project.json", ".claude/settings.json", "CLAUDE.md", ".gitignore", ".claude/skills/hexa-workflow/SKILL.md"] {
        assert!(tracked.lines().any(|l| l == want), "{want} was not committed:\n{tracked}");
    }
    // Nothing init wrote is left untracked.
    let untracked = git_ok(d, &["ls-files", "--others", "--exclude-standard", ".claude", ".hexa"]);
    assert!(untracked.trim().is_empty(), "init left its own files untracked:\n{untracked}");
}

// ── settings.json hooks ───────────────────────────────────────────────────

#[test]
fn a_users_hook_survives_init_and_a_second_run_changes_nothing() {
    let dir = repo();
    let d = dir.path();
    std::fs::create_dir_all(d.join(".claude")).unwrap();
    let mine = serde_json::json!({
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "./scripts/my-guard.sh", "timeout": 5 }]
    });
    let settings = serde_json::json!({
        "statusLine": { "type": "command", "command": "my-line" },
        "hooks": { "PreToolUse": [mine.clone()], "Notification": [{ "hooks": [{ "type": "command", "command": "say hi" }] }] }
    });
    std::fs::write(d.join(".claude/settings.json"), serde_json::to_string_pretty(&settings).unwrap()).unwrap();

    let (ok, text) = hexa(d, &["init"]);
    assert!(ok, "init failed:\n{text}");
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join(".claude/settings.json")).unwrap()).unwrap();
    let pre = after["hooks"]["PreToolUse"].as_array().expect("PreToolUse");
    assert!(pre.contains(&mine), "the user's PreToolUse hook was deleted: {after:#}");
    assert!(
        pre.iter().any(|e| e.to_string().contains("hexa hook pre-edit")),
        "hexa's hook was not added: {after:#}"
    );
    assert_eq!(after["hooks"]["Notification"], settings["hooks"]["Notification"], "an event hexa does not use was dropped");
    assert_eq!(after["statusLine"], settings["statusLine"], "the user's status line was replaced");

    let first_settings = std::fs::read_to_string(d.join(".claude/settings.json")).unwrap();
    let first_project = std::fs::read_to_string(d.join(".hexa/project.json")).unwrap();
    let (ok, text) = hexa(d, &["init", "--force"]);
    assert!(ok, "second init failed:\n{text}");
    assert_eq!(std::fs::read_to_string(d.join(".claude/settings.json")).unwrap(), first_settings, "a second init changed settings.json");
    assert_eq!(std::fs::read_to_string(d.join(".hexa/project.json")).unwrap(), first_project, "a second init changed project.json");
}

#[test]
fn init_force_keeps_the_project_id_models_and_excludes() {
    let dir = repo();
    let d = dir.path();
    let (ok, text) = hexa(d, &["init"]);
    assert!(ok, "init failed:\n{text}");
    let path = d.join(".hexa/project.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let id = v["id"].clone();
    assert!(id.is_string());
    v["inference"] = serde_json::json!({ "tier_models": { "T1": "some-model" }, "budget_usd_per_day": 3.0 });
    v["analyze"]["exclude"] = serde_json::json!(["vendor"]);
    v["custom"] = serde_json::json!("kept");
    std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();

    let (ok, text) = hexa(d, &["init", "--force"]);
    assert!(ok, "init --force failed:\n{text}");
    let after: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["id"], id, "--force minted a new id");
    assert_eq!(after["inference"], v["inference"], "--force lost the inference settings");
    assert_eq!(after["analyze"]["exclude"], serde_json::json!(["vendor"]), "--force lost analyze.exclude");
    assert_eq!(after["custom"], "kept", "--force dropped a key it does not own");
}

// ── the settings template ─────────────────────────────────────────────────

fn walk(v: &serde_json::Value, f: &mut dyn FnMut(&str, &serde_json::Value)) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                f(k, x);
                walk(x, f);
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, f)),
        _ => {}
    }
}

#[test]
fn the_settings_template_uses_seconds_and_only_real_fields() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/templates/hexa-claude-settings.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut timeouts = 0;
    walk(&v, &mut |k, x| {
        assert_ne!(k, "blocking", "Claude Code has no `blocking` hook field");
        if k == "timeout" {
            timeouts += 1;
            let n = x.as_u64().expect("a timeout is a number");
            assert!((1..=60).contains(&n), "hook timeout {n} is in seconds; that is too long");
        }
    });
    assert!(timeouts >= 5, "found {timeouts} timeouts; the walker is broken");
    assert!(!text.contains("TOOL_INPUT_PATH"), "Claude Code sets no $TOOL_INPUT_PATH; hook input is JSON on stdin");
    assert!(!text.contains("--violations-only --exit-code"), "the Stop hook fails every turn in a project with no source");
}

// ── skills ────────────────────────────────────────────────────────────────

const ONCE_FLAT: &[&str] = &["hexa-adr-create", "hexa-adr-review", "hexa-adr-search", "hexa-adr-status", "hexa-status-summary"];

#[test]
fn the_five_once_flat_skills_are_directories_claude_code_can_load() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for base in ["hexa-cli/assets/skills", ".claude/skills"] {
        for name in ONCE_FLAT {
            let flat = root.join(base).join(format!("{name}.md"));
            assert!(!flat.exists(), "{} is still a flat file", flat.display());
            let skill = root.join(base).join(name).join("SKILL.md");
            let text = std::fs::read_to_string(&skill).unwrap_or_else(|_| panic!("{} is missing", skill.display()));
            let fm = text.strip_prefix("---\n").and_then(|r| r.split_once("\n---")).map(|(f, _)| f);
            let fm = fm.unwrap_or_else(|| panic!("{} has no frontmatter", skill.display()));
            let field = |k: &str| fm.lines().find_map(|l| l.strip_prefix(&format!("{k}:")).map(|v| v.trim().to_string()));
            assert_eq!(field("name").as_deref(), Some(*name), "{}: name must match its directory", skill.display());
            assert!(field("description").is_some_and(|d| !d.is_empty()), "{}: no description", skill.display());
        }
    }
}

#[test]
fn skill_sync_removes_the_legacy_flat_copies_and_nothing_else() {
    let dir = repo();
    let d = dir.path();
    let skills = d.join(".claude/skills");
    std::fs::create_dir_all(&skills).unwrap();
    for name in ONCE_FLAT {
        std::fs::write(skills.join(format!("{name}.md")), "old flat copy\n").unwrap();
    }
    std::fs::write(skills.join("my-notes.md"), "mine\n").unwrap();

    let (ok, text) = hexa(d, &["skill", "sync"]);
    assert!(ok, "skill sync failed:\n{text}");
    for name in ONCE_FLAT {
        assert!(!skills.join(format!("{name}.md")).exists(), "{name}.md was left behind:\n{text}");
        assert!(skills.join(name).join("SKILL.md").is_file(), "{name}/SKILL.md was not written:\n{text}");
    }
    assert_eq!(std::fs::read_to_string(skills.join("my-notes.md")).unwrap(), "mine\n", "a file of the user's was removed");
}

// ── dev worktree cleanup ──────────────────────────────────────────────────

/// A worktree on `branch` at `<repo>/../<name>` whose one commit is `days` old.
fn worktree(d: &Path, wt_root: &Path, branch: &str, name: &str, days: u64) -> std::path::PathBuf {
    let path = wt_root.join(name);
    git_ok(d, &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap()]);
    std::fs::write(path.join(format!("{name}.txt")), "committed\n").unwrap();
    git_ok(&path, &["add", "."]);
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .saturating_sub(days * 86_400);
    let date = format!("@{when} +0000");
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", name])
        .current_dir(&path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    path
}

#[test]
fn cleanup_leaves_an_unmerged_dirty_worktree_and_its_files_intact() {
    let dir = repo();
    let d = dir.path();
    let wts = tempfile::tempdir().unwrap();
    let wip = worktree(d, wts.path(), "claude/old-work", "old-work", 3);
    std::fs::write(wip.join("old-work.txt"), "edited, not committed\n").unwrap();
    std::fs::write(wip.join("untracked.txt"), "new, not committed\n").unwrap();

    for args in [&["dev", "worktree", "cleanup", "--force"][..], &["dev", "worktree", "cleanup", "--force", "--stale"][..]] {
        let (_, text) = hexa(d, args);
        assert!(wip.is_dir(), "`hexa {}` removed a worktree with uncommitted work:\n{text}", args.join(" "));
        assert_eq!(std::fs::read_to_string(wip.join("old-work.txt")).unwrap(), "edited, not committed\n");
        assert_eq!(std::fs::read_to_string(wip.join("untracked.txt")).unwrap(), "new, not committed\n");
        assert!(git_ok(d, &["branch", "--list", "claude/old-work"]).contains("claude/old-work"), "the branch was deleted");
    }
}

#[test]
fn cleanup_removes_merged_by_default_and_stale_only_when_asked_and_reports_the_branch_truthfully() {
    let dir = repo();
    let d = dir.path();
    let wts = tempfile::tempdir().unwrap();
    // A merged, clean worktree: its branch points at main.
    let merged = wts.path().join("merged");
    git_ok(d, &["worktree", "add", "-q", "-b", "feat/merged", merged.to_str().unwrap()]);
    // An unmerged, clean worktree three days old.
    let stale = worktree(d, wts.path(), "claude/stale", "stale", 3);

    let (ok, text) = hexa(d, &["dev", "worktree", "cleanup", "--force"]);
    assert!(ok, "{text}");
    assert!(!merged.exists(), "a merged clean worktree was not removed:\n{text}");
    assert!(!git_ok(d, &["branch", "--list", "feat/merged"]).contains("feat/merged"), "{text}");
    assert!(stale.is_dir(), "an unmerged worktree was removed without --stale:\n{text}");

    let (ok, text) = hexa(d, &["dev", "worktree", "cleanup", "--force", "--stale"]);
    assert!(ok, "{text}");
    assert!(!stale.exists(), "--stale did not remove a clean stale worktree:\n{text}");
    assert!(
        git_ok(d, &["branch", "--list", "claude/stale"]).contains("claude/stale"),
        "an unmerged branch was deleted:\n{text}"
    );
    assert!(text.contains("branch claude/stale kept"), "the kept branch was not reported as kept:\n{text}");
    assert!(!text.contains("(branch claude/stale)"), "reported a branch deletion that did not happen:\n{text}");
}
