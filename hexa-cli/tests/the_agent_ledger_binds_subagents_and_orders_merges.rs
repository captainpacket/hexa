//! ADR-2611051850, phase 3, through the built binary: a Claude Code subagent is
//! bound to its task by the hook payload's `agent_id`, held to its lease at
//! `pre-edit`, closed by `subagent-stop` from its gate and its worktree's HEAD,
//! and `hexa dev worktree merge-order` lists finished tasks by finish time.
//! The payloads are hand-made in the shape Claude Code documents; whether a
//! live session sends them is the ADR's manual exit step, not this test's.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn hexa(
    home: &Path,
    repo: &Path,
    args: &[&str],
    stdin: Option<&str>,
    env: &[(&str, &str)],
) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hexa"));
    cmd.args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", repo)
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or("").as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn json(o: &Output) -> serde_json::Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn repo() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(repo.join("src/a")).unwrap();
    std::fs::create_dir_all(repo.join("src/b")).unwrap();
    std::fs::write(repo.join("src/a/x.rs"), "fn a() {}\n").unwrap();
    std::fs::write(repo.join("src/b/y.rs"), "fn b() {}\n").unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "init"]);
    (tmp, home, repo)
}

fn pre_edit(home: &Path, repo: &Path, agent: &str, file: &Path) -> Output {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse", "tool_name": "Edit", "agent_id": agent,
        "tool_input": { "file_path": file.to_str().unwrap() },
    });
    hexa(
        home,
        repo,
        &["hook", "pre-edit"],
        Some(&payload.to_string()),
        &[],
    )
}

#[test]
fn a_subagent_is_bound_by_its_agent_id_held_to_its_lease_and_closed_by_its_gate() {
    let (_tmp, home, repo) = repo();
    let reg = hexa(
        &home,
        &repo,
        &[
            "agent", "register", "--task", "one", "--lease", "src/a/", "--owner", "agent-77",
            "--gate", "true", "--json",
        ],
        None,
        &[],
    );
    assert!(
        reg.status.success(),
        "{}",
        String::from_utf8_lossy(&reg.stdout)
    );
    let id = json(&reg)["id"].as_str().unwrap().to_string();

    // Not yet started, but the owner label already binds: outside the lease is refused.
    let out = pre_edit(&home, &repo, "agent-77", &repo.join("src/b/y.rs"));
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(&id),
        "the refusal names the task"
    );
    assert_eq!(
        pre_edit(&home, &repo, "agent-77", &repo.join("src/a/x.rs"))
            .status
            .code(),
        Some(0)
    );
    // Another agent_id, bound to nothing, is unchanged.
    assert_eq!(
        pre_edit(&home, &repo, "stranger", &repo.join("src/b/y.rs"))
            .status
            .code(),
        Some(0)
    );

    // SubagentStart: the task becomes running.
    let start = serde_json::json!({"hook_event_name":"SubagentStart","agent_id":"agent-77","agent_type":"general-purpose","cwd":repo.to_str().unwrap()});
    let o = hexa(
        &home,
        &repo,
        &["hook", "subagent-start"],
        Some(&start.to_string()),
        &[],
    );
    assert!(o.status.success());
    let list = json(&hexa(&home, &repo, &["agent", "list", "--json"], None, &[]));
    assert_eq!(list["tasks"][0]["status"], "running", "{list}");

    // SubagentStop: the gate passes, the task closes done with the worktree's HEAD.
    let stop = serde_json::json!({"hook_event_name":"SubagentStop","agent_id":"agent-77","agent_type":"general-purpose"});
    let o = hexa(
        &home,
        &repo,
        &["hook", "subagent-stop"],
        Some(&stop.to_string()),
        &[],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let list = json(&hexa(&home, &repo, &["agent", "list", "--json"], None, &[]));
    let t = &list["tasks"][0];
    assert_eq!(
        (t["status"].as_str(), t["outcome"].as_str()),
        (Some("done"), Some("passed")),
        "{list}"
    );
    assert!(
        t["commit"].as_str().is_some_and(|c| c.len() >= 4),
        "the commit is the worktree's HEAD: {list}"
    );
    // Its lease is released: the same agent is no longer bound and can edit anywhere.
    assert_eq!(
        pre_edit(&home, &repo, "agent-77", &repo.join("src/b/y.rs"))
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn a_subagent_stop_with_a_failing_gate_fails_the_task_and_with_no_gate_leaves_it_claimed() {
    let (_tmp, home, repo) = repo();
    hexa(
        &home,
        &repo,
        &[
            "agent", "register", "--task", "red", "--lease", "src/a/", "--owner", "red-1",
            "--gate", "false",
        ],
        None,
        &[],
    );
    hexa(
        &home,
        &repo,
        &[
            "agent", "register", "--task", "nogate", "--lease", "src/b/", "--owner", "ng-1",
        ],
        None,
        &[],
    );
    for a in ["red-1", "ng-1"] {
        let stop = serde_json::json!({"hook_event_name":"SubagentStop","agent_id":a});
        let o = hexa(
            &home,
            &repo,
            &["hook", "subagent-stop"],
            Some(&stop.to_string()),
            &[],
        );
        assert!(o.status.success());
    }
    let list = json(&hexa(&home, &repo, &["agent", "list", "--json"], None, &[]));
    let by = |n: &str| {
        list["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (by("red")["status"].as_str(), by("red")["outcome"].as_str()),
        (Some("failed"), Some("failed"))
    );
    assert_eq!(
        by("nogate")["status"],
        "claimed",
        "no gate, no outcome hexa can judge: the lease is kept"
    );
}

#[test]
fn a_subagent_start_binds_a_claimed_task_registered_for_its_worktree() {
    let (_tmp, home, repo) = repo();
    let wt = repo.parent().unwrap().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"],
    );
    let wt = std::fs::canonicalize(&wt).unwrap();
    hexa(
        &home,
        &repo,
        &[
            "agent",
            "register",
            "--task",
            "wt-task",
            "--lease",
            "src/a/",
            "--worktree",
            wt.to_str().unwrap(),
        ],
        None,
        &[],
    );
    let start = serde_json::json!({"hook_event_name":"SubagentStart","agent_id":"sub-9","agent_type":"general-purpose","cwd":wt.to_str().unwrap()});
    hexa(
        &home,
        &wt,
        &["hook", "subagent-start"],
        Some(&start.to_string()),
        &[],
    );
    let list = json(&hexa(&home, &repo, &["agent", "list", "--json"], None, &[]));
    assert_eq!(
        (
            list["tasks"][0]["status"].as_str(),
            list["tasks"][0]["owner"].as_str()
        ),
        (Some("running"), Some("sub-9")),
        "{list}"
    );
    // The ledger is the main checkout's, seen from the worktree: an edit in the worktree outside the lease is refused.
    let out = pre_edit(&home, &wt, "sub-9", &wt.join("src/b/y.rs"));
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn merge_order_lists_done_tasks_by_finish_time_names_overlap_and_merge_refuses_an_unfinished_task()
{
    let (_tmp, home, repo) = repo();
    let w1 = repo.parent().unwrap().join("w1");
    let w2 = repo.parent().unwrap().join("w2");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            w1.to_str().unwrap(),
            "-b",
            "feat/one",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            w2.to_str().unwrap(),
            "-b",
            "feat/two",
        ],
    );
    let id = |name: &str, lease: &str, w: &Path| {
        let o = hexa(
            &home,
            &repo,
            &[
                "agent",
                "register",
                "--task",
                name,
                "--lease",
                lease,
                "--worktree",
                w.to_str().unwrap(),
                // `passed` is earned: the gate runs when the task reports.
                "--gate",
                "echo 'test result: ok. 1 passed; 0 failed; 0 ignored'",
                "--json",
            ],
            None,
            &[],
        );
        json(&o)["id"].as_str().unwrap().to_string()
    };
    // A commit that exists: `report --outcome passed` resolves it.
    let head = String::from_utf8(
        Command::new("git").args(["rev-parse", "--short", "HEAD"]).current_dir(&repo).output().unwrap().stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let one = id("one", "src/a/", &w1);
    let two = id("two", "src/b/", &w2);
    // Task two is still claimed: merging its worktree is refused, naming it.
    let o = hexa(
        &home,
        &repo,
        &["dev", "worktree", "merge", "feat/two"],
        None,
        &[],
    );
    assert!(!o.status.success());
    let text = String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout);
    assert!(text.contains(&two) && text.contains("not done"), "{text}");

    hexa(
        &home,
        &repo,
        &[
            "agent",
            "report",
            "--id",
            &two,
            "--outcome",
            "passed",
            "--commit",
            &head,
        ],
        None,
        &[],
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    hexa(
        &home,
        &repo,
        &[
            "agent",
            "report",
            "--id",
            &one,
            "--outcome",
            "passed",
            "--commit",
            &head,
        ],
        None,
        &[],
    );
    let order = json(&hexa(
        &home,
        &repo,
        &["dev", "worktree", "merge-order", "--json"],
        None,
        &[],
    ));
    let ids: Vec<&str> = order["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![two.as_str(), one.as_str()],
        "by finish time: {order}"
    );
    assert!(
        order["conflicts"].as_array().unwrap().is_empty(),
        "disjoint leases: {order}"
    );

    // A third task that reuses `src/a/` after one finished overlaps it in history.
    let w3 = repo.parent().unwrap().join("w3");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            w3.to_str().unwrap(),
            "-b",
            "feat/three",
        ],
    );
    let three = id("three", "src/a/x.rs", &w3);
    // Finish times have one-second resolution; keep `three` strictly after `one`.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    hexa(
        &home,
        &repo,
        &[
            "agent",
            "report",
            "--id",
            &three,
            "--outcome",
            "passed",
            "--commit",
            &head,
        ],
        None,
        &[],
    );
    let order = json(&hexa(
        &home,
        &repo,
        &["dev", "worktree", "merge-order", "--json"],
        None,
        &[],
    ));
    assert_eq!(order["conflicts"].as_array().unwrap().len(), 1, "{order}");
    assert_eq!(order["conflicts"][0]["first"], one.as_str());
}
