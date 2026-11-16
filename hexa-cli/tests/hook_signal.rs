//! The hooks print signal, not noise, and the pre-edit boundary check is real.
//!
//! An audit of a live session found the session-start block naming hexa's own
//! CLAUDE.md template as the project's objective, ADRs summarised as `1.`, a
//! TypeScript rule printed for a Rust project, and the whole block, plus
//! another session's scratch paths, repeated on every prompt. Route hints
//! fired on substrings ("scaffolded", "definitely") and named a skill that
//! does not exist. Every feature-sized prompt, sub-agent briefs included,
//! left an untracked workplan stub behind. `validate_boundary_edit` had empty
//! branches and passed a domain → adapter import. `git push --force` passed
//! in silence. Each test here drives the built binary with the JSON Claude
//! Code sends on stdin, in a temp repo.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SESSION: &str = "hook-signal-test";

/// Lines a CLAUDE.md written by `hexa init` starts with: a Behavioral Rules
/// list, then the managed section.
const MANAGED_CLAUDE_MD: &str = "# p\n\n## Behavioral Rules\n\n- ALWAYS read a file before editing it\n- NEVER commit secrets, credentials, or .env files\n\n<!-- hexa:claude-md:start -->\n## hexa — how to work in this project\n\nhexa is one binary. There is no daemon, no database, and no dashboard.\n<!-- hexa:claude-md:end -->\n";

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

struct Repo {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    dir: PathBuf,
}

/// A hexa project in a git repo. `files` are written before the first commit.
fn repo(project_json: &str, files: &[(&str, &str)]) -> Repo {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let home = base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let dir = base.join("repo");
    std::fs::create_dir_all(dir.join(".hexa")).unwrap();
    std::fs::write(dir.join(".hexa/project.json"), project_json).unwrap();
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    Repo { _tmp: tmp, home, dir }
}

const PROJECT: &str = r#"{"name":"p","id":"00000000-test"}"#;

fn rust_repo() -> Repo {
    repo(
        PROJECT,
        &[
            ("Cargo.toml", "[package]\nname = \"p\"\nversion = \"0.1.0\"\ndescription = \"A manifest description\"\n"),
            ("README.md", "# p\n\n[![ci](https://x/badge.svg)](https://x)\n\nA tiny ledger that records\nwho paid whom.\n\n## Install\n\n- run it\n"),
            ("CLAUDE.md", MANAGED_CLAUDE_MD),
            ("src/core/domain/mod.rs", "pub struct Money;\n"),
            ("src/x.rs", "fn a() {}\n"),
            (
                "docs/adrs/ADR-081-hooks.md",
                "# ADR-081: Hooks print signal, not noise\n\n**Status:** Accepted\n\n## Decision\n\n1. **Print once.** More text.\n2. **Never repeat.**\n",
            ),
            (
                "docs/adrs/ADR-080.md",
                "# ADR-080\n\n## Decision\n\n1. **Secrets are encrypted at rest.** On every host.\n",
            ),
        ],
    )
}

impl Repo {
    fn hexa_env(&self, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_hexa"));
        cmd.args(args)
            .current_dir(&self.dir)
            .env("HOME", &self.home)
            .env("HEXA_HOME", self.home.join(".hexa"))
            .env("CLAUDE_PROJECT_DIR", &self.dir)
            .env("CLAUDE_CODE_SESSION_ID", SESSION)
            .env("HEXA_NO_CLAUDE_MODS", "1")
            .env("HEXA_NO_UPDATE_CHECK", "1")
            .env_remove("HEXA_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("HEXA_INTERNAL")
            .env_remove("HEXA_TASK")
            .env_remove("HEXA_AUTO_PLAN")
            .env_remove("HEXA_BOUNDARY_MODE");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    fn hexa(&self, args: &[&str], stdin: &str) -> Output {
        self.hexa_env(args, stdin, &[])
    }

    fn session_start(&self) -> Output {
        let p = serde_json::json!({"session_id": SESSION, "hook_event_name": "SessionStart", "cwd": self.dir});
        self.hexa(&["hook", "session-start"], &p.to_string())
    }

    fn route_payload(&self, prompt: &str, extra: serde_json::Value) -> Output {
        let mut p = serde_json::json!({"session_id": SESSION, "hook_event_name": "UserPromptSubmit", "cwd": self.dir, "prompt": prompt});
        if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                o.insert(k.clone(), v.clone());
            }
        }
        self.hexa(&["hook", "route"], &p.to_string())
    }

    fn route(&self, prompt: &str) -> Output {
        self.route_payload(prompt, serde_json::json!({}))
    }

    fn pre_edit(&self, tool: &str, input: serde_json::Value) -> Output {
        let p = serde_json::json!({"session_id": SESSION, "hook_event_name": "PreToolUse", "tool_name": tool, "cwd": self.dir, "tool_input": input});
        self.hexa(&["hook", "pre-edit"], &p.to_string())
    }

    fn pre_bash(&self, command: &str) -> Output {
        let p = serde_json::json!({"session_id": SESSION, "hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": self.dir, "tool_input": {"command": command}});
        self.hexa(&["hook", "pre-bash"], &p.to_string())
    }

    fn record_gate(&self) {
        let o = self.hexa(&["loop", "gate", "cargo test"], "");
        assert!(o.status.success(), "{}", why(&o));
    }

    fn drafts(&self) -> Vec<String> {
        std::fs::read_dir(self.dir.join("docs/workplans/drafts"))
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default()
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn why(o: &Output) -> String {
    format!("exit {:?}\nstdout: {}\nstderr: {}", o.status.code(), out(o), err(o))
}

const FEATURE: &str = "implement a new payments adapter with a port, wire it into the composition root, add the usecase and tests across the http and db adapters";

// ── 1. The session-start block ───────────────────────────────────────

#[test]
fn the_objective_is_the_projects_own_description_not_hexas_template() {
    let r = rust_repo();
    let o = r.session_start();
    let s = out(&o);
    assert!(s.contains("Objective: A tiny ledger that records who paid whom."), "{}", why(&o));
    assert!(!s.contains("ALWAYS read a file"), "the managed template is not the objective: {s}");
}

#[test]
fn with_no_readme_the_manifest_description_is_the_objective_and_with_neither_there_is_none() {
    let r = repo(
        PROJECT,
        &[("Cargo.toml", "[package]\nname = \"p\"\ndescription = \"A manifest description\"\n"), ("CLAUDE.md", MANAGED_CLAUDE_MD)],
    );
    let s = out(&r.session_start());
    assert!(s.contains("Objective: A manifest description"), "{s}");

    let r = repo(PROJECT, &[("Cargo.toml", "[package]\nname = \"p\"\n"), ("CLAUDE.md", MANAGED_CLAUDE_MD)]);
    let s = out(&r.session_start());
    assert!(!s.contains("Objective:"), "nothing good found, no objective: {s}");
}

#[test]
fn an_adr_is_summarised_by_its_title_or_first_bold_clause_never_a_list_number() {
    let r = rust_repo();
    let s = out(&r.session_start());
    assert!(s.contains("- ADR-081-hooks: Hooks print signal, not noise"), "{s}");
    assert!(s.contains("- ADR-080: Secrets are encrypted at rest"), "{s}");
    assert!(!s.contains(": 1."), "no bare list number: {s}");
}

#[test]
fn the_nodenext_rule_is_printed_only_for_typescript() {
    let r = rust_repo();
    let s = out(&r.session_start());
    assert!(s.contains("Language: rust"), "{s}");
    assert!(s.contains("domain/ imports nothing outside domain/"), "the layer rules stay: {s}");
    assert!(!s.contains("NodeNext"), "a Rust project gets no .js rule: {s}");

    let r = repo(PROJECT, &[("package.json", "{\"name\":\"p\"}"), ("src/core/domain/x.ts", "export {}\n")]);
    let s = out(&r.session_start());
    assert!(s.contains("NodeNext"), "a TypeScript project keeps it: {s}");
}

// ── 1. The route hook prints the block once ──────────────────────────

#[test]
fn after_session_start_the_route_hook_never_reprints_the_block() {
    let r = rust_repo();
    assert!(out(&r.session_start()).contains("## Project Architecture Context"));
    for _ in 0..2 {
        let o = r.route(FEATURE);
        assert!(!out(&o).contains("## Project Architecture Context"), "{}", why(&o));
        assert!(!out(&o).contains("Loop (p)"), "the loop status is session-start's: {}", why(&o));
    }
}

#[test]
fn without_a_session_start_the_route_hook_prints_the_block_once() {
    let r = rust_repo();
    let first = out(&r.route("what does this do?"));
    assert!(first.contains("## Project Architecture Context"), "{first}");
    let second = out(&r.route("and this?"));
    assert!(!second.contains("## Project Architecture Context"), "{second}");
}

#[test]
fn a_feature_sized_prompt_with_no_gate_says_so_and_with_a_gate_is_silent() {
    let r = rust_repo();
    r.session_start();
    let s = out(&r.route(FEATURE));
    assert!(s.contains("hexa loop gate"), "the missing gate is named: {s}");
    r.record_gate();
    let s = out(&r.route(FEATURE));
    assert!(!s.contains("hexa loop gate"), "a recorded gate silences it: {s}");
    assert!(out(&r.route("what is the time?")).trim().is_empty(), "a question gets nothing");
}

/// `.hexa/loop.json` with this session and one live other session.
fn write_sessions(r: &Repo, mine: &[&str], theirs: &[&str]) {
    let loop_json = serde_json::json!({"sessions": {
        SESSION: {"stage": "build", "files": mine, "pid": 0, "updated": "2026-11-16T08:00:00Z"},
        "other-session-1234": {"stage": "gate", "adr": "ADR-081", "files": theirs, "pid": std::process::id(), "updated": "2026-11-16T08:01:00Z"},
    }});
    std::fs::write(r.dir.join(".hexa/loop.json"), loop_json.to_string()).unwrap();
}

#[test]
fn an_overlap_with_a_live_session_is_reported_once_with_repo_files_only_at_most_five() {
    let r = rust_repo();
    r.session_start();
    let shared: Vec<String> = (0..8).map(|i| format!("src/f{i}.rs")).collect();
    let mut mine: Vec<&str> = shared.iter().map(String::as_str).collect();
    mine.push("/private/tmp/claude-501/x/scratchpad/patch.py");
    let mut theirs = mine.clone();
    theirs.push("src/only_theirs.rs");
    write_sessions(&r, &mine, &theirs);

    let s = out(&r.route("fix the typo in f0"));
    assert!(s.contains("other-se"), "the other session is named: {s}");
    assert!(s.contains("src/f"), "the shared files are named: {s}");
    assert!(!s.contains("/tmp/") && !s.contains("scratchpad"), "never a scratch path: {s}");
    assert!(!s.contains("only_theirs"), "only files this session touched: {s}");
    let named = s.matches("src/f").count();
    assert!((1..=5).contains(&named), "at most five files, got {named}: {s}");

    let again = out(&r.route("fix the typo in f1"));
    assert!(!again.contains("other-se"), "an overlap already reported is not repeated: {again}");
}

#[test]
fn session_start_never_lists_another_sessions_scratch_paths() {
    let r = rust_repo();
    write_sessions(&r, &[], &["/private/tmp/claude-501/x/scratchpad/a.py", "/Users/someone/elsewhere/b.rs", "src/x.rs"]);
    let s = out(&r.session_start());
    assert!(s.contains("other-se"), "the other live session is visible: {s}");
    assert!(!s.contains("scratchpad") && !s.contains("/Users/someone"), "{s}");
}

// ── 2. Route hints ──────────────────────────────────────────────────

#[test]
fn hints_match_whole_words_only() {
    let r = rust_repo();
    r.session_start();
    let s = out(&r.route("the scaffolded files are definitely fine, thanks"));
    assert!(!s.contains("Relevant"), "{s}");
}

#[test]
fn hints_are_one_line_at_most_three_and_name_no_missing_skill() {
    let r = rust_repo();
    r.session_start();
    let s = out(&r.route("Scaffold a new project, check the architecture boundary, list the ADR, run parallel worktree agents and implement the feature"));
    assert_eq!(s.matches("Relevant").count(), 1, "one Relevant: line: {s}");
    let line = s.lines().find(|l| l.contains("Relevant")).unwrap();
    assert!(line.matches("; ").count() <= 2, "at most three hints: {line}");
    assert!(!s.contains("hexa-feature-dev"), "{s}");
}

#[test]
fn every_hint_target_exists() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
    let hints = hexa_cli::commands::hook::ROUTE_HINTS;
    assert!(!hints.is_empty(), "a vacuous table is a failed gate");
    let mut checked = 0;
    for h in hints {
        assert!(!h.targets.is_empty(), "{} names no target", h.text);
        for t in h.targets {
            if let Some(skill) = t.strip_prefix('/') {
                let exists = skills.join(skill).is_dir() || skills.join(format!("{skill}.md")).is_file();
                assert!(exists, "hint `{}` names skill /{skill}, which is not in assets/skills", h.text);
            } else {
                let words: Vec<&str> = t.strip_prefix("hexa ").unwrap_or_else(|| panic!("target `{t}` is neither /skill nor hexa verb")).split(' ').collect();
                let o = Command::new(env!("CARGO_BIN_EXE_hexa")).args(&words).arg("--help").output().unwrap();
                assert!(o.status.success(), "hint `{}` names `{t}`, which `hexa --help` does not know", h.text);
                assert!(h.text.contains(t), "hint `{}` shows its target `{t}`", h.text);
            }
            checked += 1;
        }
    }
    assert!(checked >= 4, "checked {checked} targets");
}

// ── 3. Workplan auto-drafts ──────────────────────────────────────────

#[test]
fn a_feature_sized_prompt_drafts_nothing_by_default() {
    let r = rust_repo();
    r.session_start();
    let o = r.route(FEATURE);
    assert!(r.drafts().is_empty(), "no draft unless the project opts in: {:?}\n{}", r.drafts(), why(&o));
    assert!(!out(&o).contains("draft"), "{}", why(&o));
}

fn opted_in() -> Repo {
    repo(
        r#"{"name":"p","id":"00000000-test","workplan":{"auto_invoke":{"enabled":true}}}"#,
        &[("Cargo.toml", "[package]\nname = \"p\"\n")],
    )
}

#[test]
fn an_opted_in_project_drafts_but_never_for_a_subagent_brief() {
    let r = opted_in();
    r.session_start();
    for brief in [format!("Act as the implementation sub-agent for this task. {FEATURE}"), format!("Delegated task node: {FEATURE}")] {
        r.route(&brief);
        assert!(r.drafts().is_empty(), "{brief}: {:?}", r.drafts());
    }
    r.route_payload(FEATURE, serde_json::json!({"agent_id": "a1b2", "agent_type": "general-purpose"}));
    assert!(r.drafts().is_empty(), "a subagent's prompt: {:?}", r.drafts());

    let o = r.route(FEATURE);
    assert_eq!(r.drafts().len(), 1, "{}", why(&o));
    let name = &r.drafts()[0];
    assert!(out(&o).contains(name.as_str()), "the hook names the draft it wrote: {}", why(&o));
}

#[test]
fn two_drafts_of_one_prompt_in_one_minute_are_two_files_and_point_at_a_real_skill() {
    let r = opted_in();
    for _ in 0..2 {
        let o = r.hexa(&["plan", "draft", "--background", "add a payments adapter"], "");
        assert!(o.status.success(), "{}", why(&o));
    }
    let drafts = r.drafts();
    assert_eq!(drafts.len(), 2, "the second draft must not overwrite the first: {drafts:?}");
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
    for d in drafts {
        let text = std::fs::read_to_string(r.dir.join("docs/workplans/drafts").join(&d)).unwrap();
        assert!(!text.contains("hexa-feature-dev"), "{text}");
        let skill = text.split('/').find_map(|s| s.strip_prefix("hexa-").map(|t| t.split(|c: char| !c.is_ascii_alphanumeric() && c != '-').next().unwrap_or(""))).unwrap_or("");
        assert!(skills.join(format!("hexa-{skill}")).is_dir(), "the stub names an existing skill, got hexa-{skill}: {text}");
    }
}

// ── 4. The pre-edit boundary check ───────────────────────────────────

fn layered_repo() -> Repo {
    let r = repo(
        PROJECT,
        &[
            ("Cargo.toml", "[package]\nname = \"p\"\nversion = \"0.1.0\"\n"),
            ("src/lib.rs", "pub mod domain;\npub mod ports;\npub mod usecases;\npub mod adapters;\n"),
            ("src/domain/mod.rs", "pub struct Money;\n"),
            ("src/ports/mod.rs", "pub trait Store {}\n"),
            ("src/usecases/mod.rs", "pub fn run() {}\n"),
            ("src/adapters/mod.rs", "pub mod secondary;\n"),
            ("src/adapters/secondary/mod.rs", "pub mod db;\n"),
            ("src/adapters/secondary/db.rs", "use crate::ports::Store;\n\npub struct Db;\n"),
            ("src/adapters/secondary/legacy.rs", "use crate::usecases::run;\n\npub fn old() {}\n"),
        ],
    );
    r.record_gate();
    r
}

#[test]
fn a_write_that_adds_a_domain_to_adapter_import_is_refused_with_rule_and_line() {
    let r = layered_repo();
    let path = r.dir.join("src/domain/mod.rs");
    let o = r.pre_edit("Write", serde_json::json!({"file_path": path, "content": "pub struct Money;\n\nuse crate::adapters::secondary::db::Db;\n"}));
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
    let e = err(&o);
    assert!(e.contains("domain must not import"), "names the rule: {e}");
    assert!(e.contains("line 3"), "names the line: {e}");
}

#[test]
fn an_edit_that_adds_a_secondary_adapter_to_usecase_import_is_refused() {
    let r = layered_repo();
    let path = r.dir.join("src/adapters/secondary/db.rs");
    let o = r.pre_edit(
        "Edit",
        serde_json::json!({"file_path": path, "old_string": "use crate::ports::Store;\n", "new_string": "use crate::ports::Store;\nuse crate::usecases::run;\n"}),
    );
    assert_eq!(o.status.code(), Some(2), "{}", why(&o));
    assert!(err(&o).contains("adapters must not import from usecases"), "{}", why(&o));
    assert!(err(&o).contains("line 2"), "{}", why(&o));
}

#[test]
fn an_allowed_import_and_an_edit_beside_an_old_violation_pass() {
    let r = layered_repo();
    let db = r.dir.join("src/adapters/secondary/db.rs");
    let o = r.pre_edit("Edit", serde_json::json!({"file_path": db, "old_string": "pub struct Db;", "new_string": "pub struct Db;\npub struct Pool;"}));
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));

    // legacy.rs already imports a use case; touching another line is not blocked for it.
    let legacy = r.dir.join("src/adapters/secondary/legacy.rs");
    let o = r.pre_edit("Edit", serde_json::json!({"file_path": legacy, "old_string": "pub fn old() {}", "new_string": "pub fn old() {}\npub fn older() {}"}));
    assert_eq!(o.status.code(), Some(0), "an old violation does not block a new edit: {}", why(&o));

    let o = r.pre_edit(
        "MultiEdit",
        serde_json::json!({"file_path": legacy, "edits": [
            {"old_string": "pub fn old() {}", "new_string": "pub fn old() {}\n"},
            {"old_string": "use crate::usecases::run;\n", "new_string": "use crate::usecases::run;\nuse crate::domain::Money;\n"}
        ]}),
    );
    assert_eq!(o.status.code(), Some(2), "a MultiEdit that adds one is refused: {}", why(&o));
    assert!(err(&o).contains("adapters must not import from domain"), "{}", why(&o));
}

#[test]
fn in_advisory_mode_a_new_violation_warns_and_passes() {
    let r = repo(r#"{"name":"p","id":"00000000-test","lifecycle_enforcement":"advisory"}"#, &[("src/domain/mod.rs", "pub struct Money;\n")]);
    let path = r.dir.join("src/domain/mod.rs");
    let o = r.pre_edit("Write", serde_json::json!({"file_path": path, "content": "use crate::adapters::secondary::db::Db;\n"}));
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
    assert!(err(&o).contains("domain must not import"), "{}", why(&o));
}

// ── 5. Destructive commands ─────────────────────────────────────────

#[test]
fn destructive_commands_warn_on_stderr_without_any_workplan() {
    let r = rust_repo();
    for cmd in [
        "git push --force origin feature",
        "git push -f origin feature",
        "git reset --hard HEAD~1",
        "git clean -fd",
        "rm -rf build",
        "git branch -D old-branch",
        "cd x && git -C sub push --force-with-lease origin feature",
    ] {
        let o = r.pre_bash(cmd);
        assert_eq!(o.status.code(), Some(0), "{cmd}: {}", why(&o));
        assert!(err(&o).to_lowercase().contains("destructive"), "{cmd} warns on stderr: {}", why(&o));
    }
    for cmd in ["git status", "git push origin feature", "rm -rf /tmp/scratch-1", "git branch -d merged", "git reset --soft HEAD~1", "rm -f a.txt"] {
        let o = r.pre_bash(cmd);
        assert_eq!(o.status.code(), Some(0), "{cmd}: {}", why(&o));
        assert!(!err(&o).to_lowercase().contains("destructive"), "{cmd} is not destructive: {}", why(&o));
    }
}

#[test]
fn a_force_push_to_main_is_refused_in_mandatory_mode_and_warned_in_advisory() {
    let r = rust_repo();
    for cmd in ["git push --force origin main", "git push -f origin HEAD:master", "git push origin +main", "git push --force"] {
        let o = r.pre_bash(cmd);
        assert_eq!(o.status.code(), Some(2), "{cmd} (the repo is on main): {}", why(&o));
        assert!(err(&o).contains("main") || err(&o).contains("master"), "{}", why(&o));
    }
    let r = repo(r#"{"name":"p","id":"00000000-test","lifecycle_enforcement":"advisory"}"#, &[]);
    let o = r.pre_bash("git push --force origin main");
    assert_eq!(o.status.code(), Some(0), "{}", why(&o));
    assert!(err(&o).to_lowercase().contains("destructive"), "{}", why(&o));
}
