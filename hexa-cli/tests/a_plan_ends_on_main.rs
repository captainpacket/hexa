//! A plan ends on main: its tasks done, its ADR closed, everything landed.
//!
//! On 2026-11-16 a day of work in one repository left a checklist reading
//! 17 of 24 with four of the seven already shipped, 17 workplans that were
//! never planned, and 7 worktrees whose branches were already on main. The
//! hooks let every session stop like that. These tests hold hexa to the end
//! the person asked for: finish all the tasks, close out the ADR, commit
//! everything into the main branch, continuously.
//!
//! Ids are assembled at run time: the citation checker scans source, and a
//! literal id here would claim a decision exists.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SESSION: &str = "plan-ends-on-main-session";

struct Run {
    code: i32,
    out: String,
}

fn hexa_in(dir: &Path, home: &Path, args: &[&str], stdin: Option<&str>) -> Run {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hexa"));
    c.args(args)
        .current_dir(dir)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HEXA_SESSION_ID", SESSION)
        .env_remove("HEXA_SESSION_PID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .env("NO_COLOR", "1")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().expect("run hexa");
    if let Some(s) = stdin {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    }
    let o = child.wait_with_output().unwrap();
    Run { code: o.status.code().unwrap_or(-1), out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)) }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn adr_id() -> String {
    format!("ADR-{}", "2701060000")
}

/// A repository on main with an upstream, one Proposed ADR, a passing
/// counted gate, and everything committed and pushed.
struct Repo {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    dir: PathBuf,
    home: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q", "--bare", "origin.git"]);
        git(&root, &["clone", "-q", "origin.git", "work"]);
        let dir = root.join("work");
        git(&dir, &["checkout", "-q", "-b", "main"]);
        std::fs::create_dir_all(dir.join("docs/adrs")).unwrap();
        let a = adr_id();
        std::fs::write(
            dir.join(format!("docs/adrs/{a}-the-work.md")),
            format!("# {a}: The work\n\n**Status:** Proposed\n**Date:** 2026-11-16\n\n## Context\n\nx\n\n## Decision\n\n1. **It holds.** Always.\n\n## Consequences\n\nx\n"),
        )
        .unwrap();
        // a counted gate hexa reads as one passing test, with no toolchain
        std::fs::write(dir.join("gate.sh"), "#!/bin/sh\necho 'test result: ok. 1 passed; 0 failed; 0 ignored'\n").unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        std::fs::write(dir.join(".gitignore"), ".hexa/\n").unwrap();
        std::fs::create_dir_all(dir.join(".hexa")).unwrap(); // a project hexa has initialised
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        git(&dir, &["push", "-q", "-u", "origin", "main"]);
        Repo { _tmp: tmp, root, dir, home }
    }

    fn hexa(&self, args: &[&str]) -> Run {
        hexa_in(&self.dir, &self.home, args, None)
    }

    fn hexa_with(&self, args: &[&str], stdin: &str) -> Run {
        hexa_in(&self.dir, &self.home, args, Some(stdin))
    }

    fn open(&self) -> serde_json::Value {
        let r = self.hexa(&["finish", "--json"]);
        serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("finish --json is JSON ({e}): {}", r.out))
    }

    fn worktree(&self, name: &str, branch: &str) -> PathBuf {
        let p = self.root.join(name);
        git(&self.dir, &["worktree", "add", "-q", "-b", branch, p.to_str().unwrap(), "main"]);
        p
    }
}

fn kinds(v: &serde_json::Value) -> Vec<String> {
    v["open"].as_array().expect("an open list").iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect()
}

fn item<'a>(v: &'a serde_json::Value, kind: &str) -> &'a serde_json::Value {
    v["open"].as_array().unwrap().iter().find(|i| i["kind"] == kind).unwrap_or_else(|| panic!("no {kind} item in {v}"))
}

#[test]
fn finish_lists_every_kind_of_open_work_with_the_command_that_closes_it() {
    let r = Repo::new();
    // landed and clean: nothing open but the ADR, which no loop is under yet
    let clean = r.hexa(&["finish", "--exit-code"]);
    assert_eq!(clean.code, 0, "{}", clean.out);
    assert!(clean.out.contains("nothing open"), "{}", clean.out);

    std::fs::write(r.dir.join("b.txt"), "two\n").unwrap();
    git(&r.dir, &["add", "b.txt"]);
    git(&r.dir, &["commit", "-q", "-m", "not pushed"]);
    std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
    assert_eq!(r.hexa(&["loop", "task", "add", "the step"]).code, 0);
    let ahead = r.worktree("wt-ahead", "feat/ahead");
    std::fs::write(ahead.join("c.txt"), "three\n").unwrap();
    git(&ahead, &["add", "c.txt"]);
    git(&ahead, &["commit", "-q", "-m", "on a branch"]);
    r.worktree("wt-merged", "feat/merged");
    std::fs::create_dir_all(r.dir.join("docs/workplans/drafts")).unwrap();
    std::fs::write(r.dir.join("docs/workplans/drafts/draft-x.json"), r#"{"id":"draft-x","status":"pending-planner","prompt":"p"}"#).unwrap();

    let v = r.open();
    for k in ["uncommitted", "unpushed", "task", "unlanded", "landed", "draft"] {
        assert!(kinds(&v).contains(&k.to_string()), "{k} is listed: {v}");
    }
    assert!(item(&v, "uncommitted")["what"].as_str().unwrap().contains("a.txt"));
    assert!(item(&v, "uncommitted")["fix"].as_str().unwrap().contains("hexa do gate"));
    assert!(item(&v, "unpushed")["fix"].as_str().unwrap().contains("git push"));
    assert!(item(&v, "task")["what"].as_str().unwrap().contains("the step"));
    assert!(item(&v, "unlanded")["what"].as_str().unwrap().contains("feat/ahead"));
    assert!(item(&v, "landed")["what"].as_str().unwrap().contains("feat/merged"));
    assert!(item(&v, "landed")["fix"].as_str().unwrap().contains("hexa dev worktree cleanup"));
    assert!(item(&v, "draft")["fix"].as_str().unwrap().contains("hexa plan drafts clear"));
    let code = r.hexa(&["finish", "--exit-code"]);
    assert_eq!(code.code, 1, "open work exits 1: {}", code.out);
}

#[test]
fn every_plan_ends_with_landing_on_main_and_closing_its_adr() {
    let r = Repo::new();
    let a = adr_id();
    assert_eq!(r.hexa(&["loop", "task", "add", "first"]).code, 0);
    let rec = r.hexa(&["loop", "adr", &a]);
    assert_eq!(rec.code, 0, "{}", rec.out);
    assert_eq!(r.hexa(&["loop", "adr", &a]).code, 0, "recording it again adds nothing");
    assert_eq!(r.hexa(&["loop", "task", "add", "second"]).code, 0);
    let show = r.hexa(&["loop", "show"]);
    let titles: Vec<&str> = show.out.lines().map(str::trim).filter(|l| l.starts_with("[ ]") || l.starts_with("[x]") || l.starts_with("[>]")).map(|l| l[3..].trim()).collect();
    let names: Vec<String> = titles.iter().map(|t| t.split_once(' ').map(|(_, x)| x.to_string()).unwrap_or_default()).collect();
    assert_eq!(names, vec!["first".to_string(), "second".to_string(), "Land on main".to_string(), format!("Close {a}")], "{}", show.out);
}

#[test]
fn land_on_main_is_not_done_while_a_branch_holds_work_off_main() {
    let r = Repo::new();
    assert_eq!(r.hexa(&["loop", "adr", &adr_id()]).code, 0);
    let ahead = r.worktree("wt-ahead", "feat/ahead");
    std::fs::write(ahead.join("c.txt"), "three\n").unwrap();
    git(&ahead, &["add", "c.txt"]);
    git(&ahead, &["commit", "-q", "-m", "on a branch"]);
    let refused = r.hexa(&["loop", "task", "done", "1"]);
    assert_ne!(refused.code, 0, "{}", refused.out);
    assert!(refused.out.contains("feat/ahead"), "the refusal names the branch: {}", refused.out);
    git(&r.dir, &["merge", "-q", "--ff-only", "feat/ahead"]);
    git(&r.dir, &["push", "-q"]);
    let ok = r.hexa(&["loop", "task", "done", "1"]);
    assert_eq!(ok.code, 0, "landed, it is done: {}", ok.out);
}

#[test]
fn stage_done_closes_the_adr_only_when_everything_is_on_main() {
    let r = Repo::new();
    let a = adr_id();
    assert_eq!(r.hexa(&["loop", "adr", &a]).code, 0);
    assert_eq!(r.hexa(&["loop", "gate", "sh gate.sh"]).code, 0);
    assert_eq!(r.hexa(&["loop", "task", "add", "the step"]).code, 0);
    let refused = r.hexa(&["loop", "stage", "done"]);
    assert_ne!(refused.code, 0, "{}", refused.out);
    assert!(refused.out.contains("the step"), "names the open task: {}", refused.out);
    assert!(std::fs::read_to_string(r.dir.join(format!("docs/adrs/{a}-the-work.md"))).unwrap().contains("**Status:** Proposed"));

    assert_eq!(r.hexa(&["loop", "task", "done", "1"]).code, 0);
    let done = r.hexa(&["loop", "stage", "done"]);
    assert_eq!(done.code, 0, "{}", done.out);
    let text = std::fs::read_to_string(r.dir.join(format!("docs/adrs/{a}-the-work.md"))).unwrap();
    assert!(text.contains("**Status:** Completed"), "{text}");
    assert!(text.contains("landed on main at"), "the reason names where it landed: {text}");
    let list = r.hexa(&["loop", "show"]);
    assert!(!list.out.contains("[ ]"), "every step is checked off: {}", list.out);
}

#[test]
fn a_gated_commit_on_a_branch_lands_on_main() {
    let r = Repo::new();
    let wt = r.worktree("wt-feat", "feat/x");
    std::fs::write(wt.join("d.txt"), "four\n").unwrap();
    let run = hexa_in(&wt, &r.home, &["do", "gate", "--files", "d.txt", "-e", "sh gate.sh", "-m", "feat: d"], None);
    assert_eq!(run.code, 0, "{}", run.out);
    let branch_tip = git(&wt, &["rev-parse", "HEAD"]);
    assert_eq!(git(&r.dir, &["rev-parse", "main"]), branch_tip, "main fast-forwarded to the gated commit: {}", run.out);
    assert_ne!(git(&r.dir, &["rev-parse", "origin/main"]), branch_tip, "and not pushed: a push can need someone's approval, so it stays its own step");

    // a change that conflicts with main stays on its branch, and says so
    std::fs::write(r.dir.join("a.txt"), "main's\n").unwrap();
    git(&r.dir, &["commit", "-q", "-am", "main moves"]);
    std::fs::write(wt.join("a.txt"), "branch's\n").unwrap();
    let main_before = git(&r.dir, &["rev-parse", "main"]);
    let clash = hexa_in(&wt, &r.home, &["do", "gate", "--files", "a.txt", "-e", "sh gate.sh", "-m", "feat: a"], None);
    assert_eq!(clash.code, 0, "the commit is made on the branch: {}", clash.out);
    assert_eq!(git(&r.dir, &["rev-parse", "main"]), main_before, "main is not touched");
    assert!(clash.out.contains("not landed"), "the failure to land is reported: {}", clash.out);
}

#[test]
fn a_session_with_open_work_is_sent_back_to_finish_it_three_times_at_most() {
    let r = Repo::new();
    std::fs::write(r.dir.join("a.txt"), "changed\n").unwrap();
    // the session touched a.txt
    let edit = serde_json::json!({ "session_id": SESSION, "tool_name": "Edit", "tool_input": { "file_path": r.dir.join("a.txt") } }).to_string();
    r.hexa_with(&["hook", "post-edit"], &edit);
    let stop = serde_json::json!({ "session_id": SESSION, "stop_hook_active": false, "hook_event_name": "Stop" }).to_string();
    for n in 1..=3 {
        let s = r.hexa_with(&["hook", "stop"], &stop);
        assert!(s.out.contains("\"decision\":\"block\""), "stop {n} is sent back: {}", s.out);
        assert!(s.out.contains("a.txt"), "with the file in the reason: {}", s.out);
    }
    let fourth = r.hexa_with(&["hook", "stop"], &stop);
    assert!(!fourth.out.contains("\"decision\":\"block\""), "the fourth stop goes through: {}", fourth.out);
    assert!(fourth.out.contains("a.txt"), "and still says what is open: {}", fourth.out);

    // finished, the session stops at once, and the count starts again
    git(&r.dir, &["commit", "-q", "-am", "done"]);
    git(&r.dir, &["push", "-q"]);
    let clean = r.hexa_with(&["hook", "stop"], &stop);
    assert!(!clean.out.contains("\"decision\":\"block\""), "{}", clean.out);
}

#[test]
fn a_long_prompt_writes_no_draft_workplan() {
    let r = Repo::new();
    let prompt = "Implement the whole billing subsystem across the domain, ports, use cases and adapters, with tests for every rule. ".repeat(20);
    let p = serde_json::json!({ "session_id": SESSION, "prompt": prompt, "hook_event_name": "UserPromptSubmit" }).to_string();
    r.hexa_with(&["hook", "route"], &p);
    let drafts = r.dir.join("docs/workplans/drafts");
    let n = std::fs::read_dir(&drafts).map(|d| d.count()).unwrap_or(0);
    assert_eq!(n, 0, "no draft was written");
}
