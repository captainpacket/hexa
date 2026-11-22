//! The code graph is always built and fresh, and `hexa do gate` runs where the
//! tests run.
//!
//! Written from the accepted scenarios of `docs/specs/<this ADR>.json`, before
//! the code. Every case runs the built binary in a temp repository; a remote
//! runner is a local script that clones the pushed commit, as a build server
//! would.
//!
//! Fixture ids are assembled at run time: the citation checker scans source,
//! and a literal id here would claim a decision exists.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ── harness ──────────────────────────────────────────────────────────────────

const SESSION: &str = "graph-fresh-test";

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Run {
    fn all(&self) -> String {
        format!("{}{}", self.out, self.err)
    }
}

fn hexa_in(dir: &Path, project: &Path, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Run {
    let home = project.join(".test-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hexa"));
    cmd.args(args)
        .current_dir(dir)
        .env("HOME", &home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("CLAUDE_PROJECT_DIR", project)
        .env("CLAUDE_CODE_SESSION_ID", SESSION)
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env("NO_COLOR", "1")
        .env_remove("HEXA_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("HEXA_INTERNAL")
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_PROJECT_ROOT")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("run hexa");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).to_string(),
        err: String::from_utf8_lossy(&o.stderr).to_string(),
    }
}

fn hexa(root: &Path, args: &[&str]) -> Run {
    hexa_in(root, root, args, "", &[])
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@x",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

fn executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    (t, root)
}

/// A small Rust crate where `src/b.rs` imports `src/a.rs`, in a git repo with
/// one commit, and a hexa project file. `project_json` is its whole content.
fn crate_repo(project_json: &str) -> (tempfile::TempDir, PathBuf) {
    let (t, root) = tmp();
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root, "src/lib.rs", "pub mod a;\npub mod b;\n");
    write(&root, "src/a.rs", "pub fn helper() -> u32 { 1 }\n");
    write(
        &root,
        "src/b.rs",
        "use crate::a::helper;\npub fn user() -> u32 { helper() }\n",
    );
    write(
        &root,
        ".gitignore",
        ".test-home/\ntarget/\n.hexa/loop.json\n.hexa/loop.lock\n.hexa/graph/\n",
    );
    write(&root, ".hexa/project.json", project_json);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    (t, root)
}

fn advisory() -> &'static str {
    r#"{"name":"p","id":"00000000-test","lifecycle_enforcement":"advisory"}"#
}

/// The `file` of every node in the built graph.
fn node_files(root: &Path) -> Vec<String> {
    let v: serde_json::Value =
        serde_json::from_str(&read(root, ".hexa/graph/graph.json")).expect("graph.json is JSON");
    v["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|n| n["file"].as_str().unwrap_or("").to_string())
        .collect()
}

fn node_count(root: &Path) -> usize {
    let v: serde_json::Value = serde_json::from_str(&read(root, ".hexa/graph/graph.json")).unwrap();
    v["nodes"].as_array().map(Vec::len).unwrap_or(0)
}

fn build(root: &Path) {
    let r = hexa(root, &["graph", "build"]);
    assert_eq!(r.code, 0, "graph build:\n{}", r.all());
}

fn status(root: &Path) -> serde_json::Value {
    let r = hexa(root, &["graph", "status", "--json"]);
    assert_eq!(
        r.code,
        0,
        "graph status exits 0 in every state:\n{}",
        r.all()
    );
    serde_json::from_str(r.out.trim())
        .unwrap_or_else(|e| panic!("status --json is JSON ({e}):\n{}", r.all()))
}

fn stamp(root: &Path) -> serde_json::Value {
    serde_json::from_str(&read(root, ".hexa/graph/stamp.json")).unwrap_or(serde_json::Value::Null)
}

// ── §1 the graph reads the project's files only ─────────────────────────────

/// S01: The graph reads the project's files and no nested worktree's
#[test]
fn s01_a_nested_worktree_is_not_read() {
    let (_t, root) = crate_repo(advisory());
    git(&root, &["worktree", "add", "-q", "origin/t3/x", "-b", "x"]);
    write(
        &root,
        "origin/t3/x/src/extra.rs",
        "pub fn only_in_the_worktree() {}\n",
    );
    assert!(
        root.join("origin/t3/x/.git").is_file(),
        "the fixture holds a real nested worktree"
    );
    build(&root);
    let files = node_files(&root);
    assert!(
        files.iter().any(|f| f == "src/a.rs"),
        "the project's own file is read: {files:?}"
    );
    let nested: Vec<&String> = files.iter().filter(|f| f.starts_with("origin/")).collect();
    assert!(
        nested.is_empty(),
        "no node comes from the nested worktree: {nested:?}"
    );
}

/// S02: A gitignored file is not read
#[test]
fn s02_a_gitignored_file_is_not_read() {
    let (_t, root) = crate_repo(advisory());
    write(
        &root,
        ".gitignore",
        ".test-home/\ntarget/\n.hexa/loop.json\n.hexa/loop.lock\n.hexa/graph/\ngen/\n",
    );
    write(&root, "gen/big.rs", "pub fn generated() {}\n");
    build(&root);
    let files = node_files(&root);
    assert!(files.iter().any(|f| f == "src/a.rs"), "{files:?}");
    assert!(
        !files.iter().any(|f| f.starts_with("gen/")),
        "no node from the ignored file: {files:?}"
    );
}

// ── §2 a build records what it read ──────────────────────────────────────────

/// S03: A build records a stamp with its fingerprint
#[test]
fn s03_a_build_records_its_stamp() {
    let (_t, root) = crate_repo(advisory());
    build(&root);
    let s = stamp(&root);
    assert!(
        s["built_at"].as_str().is_some_and(|b| !b.is_empty()),
        "built_at: {s}"
    );
    assert_eq!(
        s["nodes"].as_u64(),
        Some(node_count(&root) as u64),
        "nodes is the graph's count: {s}"
    );
    let fp = s["fingerprint"].as_str().unwrap_or("");
    assert!(
        fp.len() == 64 && fp.chars().all(|c| c.is_ascii_hexdigit()),
        "a sha256 fingerprint: {s}"
    );
}

/// S04: A deleted file changes the fingerprint though no file is newer
#[test]
fn s04_a_deletion_makes_the_graph_stale() {
    let (_t, root) = crate_repo(advisory());
    build(&root);
    assert_eq!(status(&root)["state"], "fresh");
    std::fs::remove_file(root.join("src/b.rs")).unwrap();
    let s = status(&root);
    assert_eq!(s["state"], "stale", "a deletion is a change: {s}");
}

// ── §3 hexa graph status ─────────────────────────────────────────────────────

/// S05: Status reports missing, then fresh, then stale after an edit
#[test]
fn s05_status_reports_missing_fresh_and_stale() {
    let (_t, root) = crate_repo(advisory());
    let s = status(&root);
    assert_eq!(s["state"], "missing", "{s}");
    assert_eq!(s["built"], false, "{s}");
    build(&root);
    let s = status(&root);
    assert_eq!(s["state"], "fresh", "{s}");
    assert_eq!(s["built"], true, "{s}");
    assert_eq!(s["fresh"], true, "{s}");
    assert!(s["nodes"].as_u64().unwrap_or(0) > 0, "{s}");
    assert!(s["built_at"].as_str().is_some(), "{s}");
    std::thread::sleep(Duration::from_millis(1100));
    write(
        &root,
        "src/a.rs",
        "pub fn helper() -> u32 { 2 }\npub fn more() {}\n",
    );
    let s = status(&root);
    assert_eq!(s["state"], "stale", "{s}");
    assert_eq!(s["fresh"], false, "{s}");
    assert!(
        s["changed"].as_u64().unwrap_or(0) >= 1,
        "the edited file is counted: {s}"
    );
}

/// S06: A graph with no stamp is stale, not fresh
#[test]
fn s06_a_graph_without_a_stamp_is_stale() {
    let (_t, root) = crate_repo(advisory());
    build(&root);
    std::fs::remove_file(root.join(".hexa/graph/stamp.json")).unwrap();
    let s = status(&root);
    assert_eq!(s["state"], "stale", "{s}");
    assert_eq!(s["built"], true, "{s}");
}

// ── §4 hexa init builds the graph ────────────────────────────────────────────

fn init_dir() -> (tempfile::TempDir, PathBuf) {
    let (t, root) = tmp();
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root, "src/lib.rs", "pub fn hello() -> u32 { 1 }\n");
    (t, root)
}

/// S07: `hexa init` builds the graph
#[test]
fn s07_init_builds_the_graph() {
    let (_t, root) = init_dir();
    let r = hexa(&root, &["init", "."]);
    assert_eq!(r.code, 0, "init:\n{}", r.all());
    assert!(
        root.join(".hexa/graph/graph.json").is_file(),
        "init built the graph:\n{}",
        r.all()
    );
    assert_eq!(status(&root)["state"], "fresh");
}

/// S08: The graph init builds holds nothing init wrote under `.claude/`
#[test]
fn s08_init_graph_holds_no_claude_files() {
    let (_t, root) = init_dir();
    let r = hexa(&root, &["init", "."]);
    assert_eq!(r.code, 0, "init:\n{}", r.all());
    let files = node_files(&root);
    assert!(files.iter().any(|f| f == "src/lib.rs"), "{files:?}");
    let own: Vec<&String> = files
        .iter()
        .filter(|f| f.starts_with(".claude/") || f.starts_with(".hexa/"))
        .collect();
    assert!(own.is_empty(), "nothing of hexa's or Claude's own: {own:?}");
}

// ── §5 SessionStart rebuilds a stale graph ───────────────────────────────────

fn session_start(root: &Path) -> Run {
    let payload = serde_json::json!({ "session_id": SESSION, "hook_event_name": "SessionStart", "cwd": root.to_str().unwrap() });
    hexa_in(
        root,
        root,
        &["hook", "session-start"],
        &payload.to_string(),
        &[],
    )
}

/// S09: SessionStart rebuilds a stale graph in the background
#[test]
fn s09_session_start_rebuilds_a_stale_graph() {
    let (_t, root) = crate_repo(advisory());
    build(&root);
    std::thread::sleep(Duration::from_millis(1100));
    write(
        &root,
        "src/a.rs",
        "pub fn helper() -> u32 { 3 }\npub fn other() {}\n",
    );
    assert_eq!(status(&root)["state"], "stale");
    let r = session_start(&root);
    assert_eq!(r.code, 0, "{}", r.all());
    let line = r
        .all()
        .lines()
        .find(|l| l.contains("graph:"))
        .map(str::to_string);
    assert!(
        line.as_deref().is_some_and(|l| l.contains("stale")),
        "a graph: line names the stale state:\n{}",
        r.all()
    );
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        if status(&root)["state"] == "fresh" {
            break;
        }
        assert!(
            Instant::now() < until,
            "the background rebuild made the graph fresh within 60s"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// S10: SessionStart leaves a fresh graph alone
#[test]
fn s10_session_start_leaves_a_fresh_graph() {
    let (_t, root) = crate_repo(advisory());
    build(&root);
    let before = stamp(&root)["built_at"].clone();
    let r = session_start(&root);
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(r.all().contains("graph: fresh"), "{}", r.all());
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        stamp(&root)["built_at"],
        before,
        "no rebuild of a fresh graph"
    );
}

// ── §6 the edit hook prints the file's graph context ────────────────────────

fn pre_edit(root: &Path, file: &str) -> Run {
    let payload = serde_json::json!({
        "session_id": SESSION, "hook_event_name": "PreToolUse", "tool_name": "Edit",
        "cwd": root.to_str().unwrap(),
        "tool_input": { "file_path": root.join(file).to_str().unwrap(), "old_string": "1", "new_string": "2" },
    });
    hexa_in(root, root, &["hook", "pre-edit"], &payload.to_string(), &[])
}

/// The lines of the graph block a hook printed: from its first `graph` line on,
/// up to the first blank line or line that is not part of it.
fn graph_lines(out: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut inside = false;
    for l in out.lines() {
        if !inside && l.contains("[HEX] graph") {
            inside = true;
        }
        if inside {
            if l.trim().is_empty() {
                break;
            }
            v.push(l.to_string());
        }
    }
    v
}

/// S11: The edit hook prints the file's graph context, bounded
#[test]
fn s11_the_edit_hook_prints_graph_context() {
    let (_t, root) = crate_repo(advisory());
    build(&root);
    let r = pre_edit(&root, "src/a.rs");
    assert_eq!(r.code, 0, "never blocks:\n{}", r.all());
    let lines = graph_lines(&r.out);
    assert!(!lines.is_empty(), "graph context printed:\n{}", r.all());
    assert!(
        lines.len() <= 12,
        "bounded to 12 lines, got {}:\n{}",
        lines.len(),
        lines.join("\n")
    );
    let block = lines.join("\n");
    assert!(block.contains("src/a.rs"), "names the file:\n{block}");
    assert!(block.contains("src/b.rs"), "names its consumer:\n{block}");
    let again = pre_edit(&root, "src/a.rs");
    assert_eq!(again.code, 0, "{}", again.all());
    assert!(
        graph_lines(&again.out).is_empty(),
        "once per file per session:\n{}",
        again.all()
    );
}

/// S12: With no graph, or for a file the graph does not hold, the edit hook prints no context and does not block
#[test]
fn s12_no_graph_means_no_context() {
    let (_t, root) = crate_repo(advisory());
    let r = pre_edit(&root, "src/a.rs");
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(
        graph_lines(&r.out).is_empty(),
        "no graph, no context:\n{}",
        r.all()
    );
    build(&root);
    write(&root, "src/new.rs", "pub fn fresh() {}\n");
    let r = pre_edit(&root, "src/new.rs");
    assert_eq!(r.code, 0, "{}", r.all());
    assert!(
        graph_lines(&r.out).is_empty(),
        "a file the graph does not hold gets no context:\n{}",
        r.all()
    );
    // The control: the same hook, in the same session, does print for a file the graph holds.
    let r = pre_edit(&root, "src/b.rs");
    assert!(
        !graph_lines(&r.out).is_empty(),
        "the hook is live for a file the graph holds:\n{}",
        r.all()
    );
}

// ── §7 a remote do gate ──────────────────────────────────────────────────────

/// A bare origin for `root`, and a runner script beside it that clones the
/// commit it is given, prints the id it checked out, runs the command there,
/// and appends one line per invocation to `log`.
struct Remote {
    _dir: tempfile::TempDir,
    log: PathBuf,
    template: String,
}

fn remote_for(root: &Path) -> Remote {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", origin.to_str().unwrap()],
    );
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    let log = dir.path().join("runner.log");
    let runner = dir.path().join("runner.sh");
    let body = format!(
        "#!/bin/bash\necho \"$1\" >> '{log}'\nd=$(mktemp -d)\n\
         git clone -q '{origin}' \"$d/c\" && cd \"$d/c\" && git checkout -q \"$1\" || exit 2\n\
         echo \"runner: testing $(git rev-parse HEAD)\"\nbash -c \"$2\"; rc=$?\ncd /; rm -rf \"$d\"; exit $rc\n",
        log = log.display(),
        origin = origin.display()
    );
    std::fs::write(&runner, body).unwrap();
    executable(&runner);
    Remote {
        template: format!("bash {} {{commit}} {{cmd}}", runner.display()),
        log,
        _dir: dir,
    }
}

/// A project with `test.remote`, `ci/ok.sh` and `ci/fail.sh`, on branch `feat`
/// tracking origin, with an uncommitted change to `src/a.rs`.
fn remote_project() -> (tempfile::TempDir, PathBuf, Remote) {
    let (t, root) = crate_repo(advisory());
    let remote = remote_for(&root);
    let pj = serde_json::json!({ "name": "p", "id": "00000000-test", "lifecycle_enforcement": "advisory",
                                 "test": { "remote": remote.template } });
    write(
        &root,
        ".hexa/project.json",
        &serde_json::to_string_pretty(&pj).unwrap(),
    );
    write(
        &root,
        "ci/ok.sh",
        "#!/bin/bash\ntouch ran-here\necho ok\nexit 0\n",
    );
    write(
        &root,
        "ci/fail.sh",
        "#!/bin/bash\ntouch ran-here\necho failing\nexit 1\n",
    );
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "remote"]);
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["checkout", "-q", "-b", "feat"]);
    git(&root, &["push", "-q", "-u", "origin", "feat"]);
    write(&root, "src/a.rs", "pub fn helper() -> u32 { 42 }\n");
    (t, root, remote)
}

fn gate_refs(root: &Path) -> String {
    git(root, &["ls-remote", "origin", "refs/heads/hexa-gate/*"])
}

/// S13: A remote `do gate` commits the commit the runner tested, on exit 0
#[test]
fn s13_a_remote_gate_commits_on_exit_zero() {
    let (_t, root, remote) = remote_project();
    let before = git(&root, &["rev-parse", "HEAD"]);
    let r = hexa(
        &root,
        &[
            "do",
            "gate",
            "--files",
            "src/a.rs",
            "-e",
            "bash ci/ok.sh",
            "-m",
            "fix: a",
            "--no-land",
        ],
    );
    assert_eq!(
        r.code,
        0,
        "the remote gate passed and committed:\n{}",
        r.all()
    );
    let head = git(&root, &["rev-parse", "HEAD"]);
    assert_ne!(head, before, "the branch moved");
    assert_eq!(
        git(&root, &["rev-parse", "HEAD^"]),
        before,
        "one commit on top of the old HEAD"
    );
    let tested = std::fs::read_to_string(&remote.log).unwrap();
    assert!(
        tested.lines().any(|l| l.trim() == head),
        "HEAD is the commit the runner was sent ({tested})"
    );
    assert!(
        r.all().contains(&format!("runner: testing {head}")),
        "the runner's output is shown:\n{}",
        r.all()
    );
    assert!(
        git(&root, &["show", "HEAD:src/a.rs"]).contains("42"),
        "the commit holds the change"
    );
    assert_eq!(
        git(&root, &["ls-remote", "origin", "refs/heads/feat"])
            .split_whitespace()
            .next(),
        Some(head.as_str()),
        "pushed"
    );
    assert_eq!(
        gate_refs(&root),
        "",
        "no temporary gate ref is left on origin"
    );
    let note = git(&root, &["notes", "--ref=hexa-gate", "show", "HEAD"]);
    assert!(
        note.contains("runner: testing"),
        "the evidence note holds the runner's output: {note}"
    );
    assert!(
        !root.join("ran-here").exists(),
        "the gate ran on the remote, not here"
    );
    assert_eq!(
        git(
            &root,
            &[
                "status",
                "--porcelain",
                "--untracked-files=no",
                "--",
                "src/a.rs"
            ]
        ),
        "",
        "the file is committed and clean"
    );
}

/// S14: A remote `do gate` whose gate fails commits nothing
#[test]
fn s14_a_failing_remote_gate_commits_nothing() {
    let (_t, root, remote) = remote_project();
    let before = git(&root, &["rev-parse", "HEAD"]);
    let r = hexa(
        &root,
        &[
            "do",
            "gate",
            "--files",
            "src/a.rs",
            "-e",
            "bash ci/fail.sh",
            "-m",
            "fix: a",
            "--no-land",
        ],
    );
    assert_ne!(r.code, 0, "a failing remote gate fails:\n{}", r.all());
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        before,
        "HEAD has not moved"
    );
    assert!(
        read(&root, "src/a.rs").contains("42"),
        "the change is still in the tree"
    );
    assert!(
        !git(&root, &["status", "--porcelain", "--", "src/a.rs"]).is_empty(),
        "and still uncommitted"
    );
    assert_eq!(
        gate_refs(&root),
        "",
        "no temporary gate ref is left on origin"
    );
    assert!(
        std::fs::read_to_string(&remote.log)
            .map(|s| s.lines().count())
            .unwrap_or(0)
            >= 1,
        "it went to the runner"
    );
    assert!(!root.join("ran-here").exists(), "nothing ran the gate here");
    assert_eq!(
        git(&root, &["ls-remote", "origin", "refs/heads/feat"])
            .split_whitespace()
            .next(),
        Some(before.as_str()),
        "origin's branch is unchanged"
    );
}

// ── §8 a recorded amendment passes the gate ──────────────────────────────────

fn the_id() -> String {
    format!("ADR-{}", "2701220000")
}

fn spec_rel() -> String {
    format!("docs/specs/{}.json", the_id())
}

/// A repo with an accepted spec whose one scenario's test, `tests/t.sh`, is sealed.
fn sealed_repo() -> (tempfile::TempDir, PathBuf) {
    let (t, root) = tmp();
    let id = the_id();
    write(
        &root,
        "Cargo.toml",
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root, "src/lib.rs", "pub fn f() {}\n");
    write(&root, "tests/t.sh", "#!/bin/bash\necho checking\nexit 0\n");
    write(
        &root,
        ".gitignore",
        ".test-home/\ntarget/\n.hexa/loop.json\n.hexa/loop.lock\n.hexa/graph/\n",
    );
    write(&root, ".hexa/project.json", advisory());
    write(
        &root,
        &format!("docs/adrs/{id}-a-decision.md"),
        &format!(
            "# {id}: A decision\n\n**Status:** Accepted\n**Date:** 2026-11-01\n\n## Context\n\nx\n\n\
             ## Decision\n\n1. **It holds.** Always.\n\n## Consequences\n\nx\n"
        ),
    );
    let sc = |sid: &str, kind: &str| {
        serde_json::json!({
            "id": sid, "decision": ["§1"], "kind": kind, "description": format!("{sid} holds"),
            "given": "a thing", "when": "it runs", "then": "it holds", "test": "tests/t.sh"
        })
    };
    let spec = serde_json::json!({
        "adr": id, "feature": "f", "description": "d", "accepted": null, "sealed": {},
        "specs": [sc("S1", "example"), sc("S2", "negative")]
    });
    write(
        &root,
        &spec_rel(),
        &serde_json::to_string_pretty(&spec).unwrap(),
    );
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    let r = hexa(&root, &["spec", "accept", &id, "--by", "a reviewer"]);
    assert_eq!(r.code, 0, "accepted:\n{}", r.all());
    let r = hexa(&root, &["spec", "seal", &id]);
    assert_eq!(r.code, 0, "sealed:\n{}", r.all());
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "accept"]);
    let spec: serde_json::Value = serde_json::from_str(&read(&root, &spec_rel())).unwrap();
    assert!(
        spec["sealed"]["tests/t.sh"].is_string(),
        "the fixture's test is sealed: {spec}"
    );
    (t, root)
}

fn sha256_of(root: &Path, rel: &str) -> String {
    let o = Command::new("sha256sum")
        .arg(root.join(rel))
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// S15: A sealed test whose amendment was recorded is committed through the gate
#[test]
fn s15_an_amended_sealed_test_is_accepted() {
    let (_t, root) = sealed_repo();
    let id = the_id();
    write(
        &root,
        "tests/t.sh",
        "#!/bin/bash\necho checking more\nexit 0\n",
    );
    let sum = sha256_of(&root, "tests/t.sh");
    let r = hexa(
        &root,
        &[
            "spec",
            "amend",
            &id,
            "--file",
            "tests/t.sh",
            "--reason",
            "says more",
            "--by",
            "t",
            "--confirm",
            &sum,
        ],
    );
    assert_eq!(r.code, 0, "the amendment is recorded:\n{}", r.all());
    let before = git(&root, &["rev-parse", "HEAD"]);
    let spec = spec_rel();
    let r = hexa(
        &root,
        &[
            "do",
            "gate",
            "--local",
            "--no-land",
            "--files",
            "tests/t.sh",
            &spec,
            "-e",
            "bash tests/t.sh",
            "-m",
            "test: amend",
        ],
    );
    assert_eq!(
        r.code,
        0,
        "a recorded amendment passes the gate:\n{}",
        r.all()
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD^"]), before, "one commit");
    let changed = git(&root, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(changed.lines().any(|l| l == "tests/t.sh"), "{changed}");
    assert!(changed.lines().any(|l| l == spec), "{changed}");
}

/// S16: An unrecorded edit to a sealed test is still refused
#[test]
fn s16_an_unamended_sealed_test_is_refused() {
    let (_t, root) = sealed_repo();
    let id = the_id();
    write(&root, "tests/t.sh", "#!/bin/bash\nexit 0\n");
    let before = git(&root, &["rev-parse", "HEAD"]);
    let spec = spec_rel();
    let gate = |root: &Path| {
        hexa(
            root,
            &[
                "do",
                "gate",
                "--local",
                "--no-land",
                "--files",
                "tests/t.sh",
                &spec,
                "-e",
                "bash tests/t.sh",
                "-m",
                "test: loosen",
            ],
        )
    };
    let r = hexa(
        &root,
        &[
            "do",
            "gate",
            "--local",
            "--no-land",
            "--files",
            "tests/t.sh",
            "-e",
            "bash tests/t.sh",
            "-m",
            "test: loosen",
        ],
    );
    assert_ne!(r.code, 0, "refused:\n{}", r.all());
    assert!(
        r.all().contains("tests/t.sh") && r.all().contains("sealed"),
        "names the sealed file:\n{}",
        r.all()
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        before,
        "nothing committed"
    );
    // An amendment recorded for other content does not cover this edit.
    let sum = sha256_of(&root, "tests/t.sh");
    let r = hexa(
        &root,
        &[
            "spec",
            "amend",
            &id,
            "--file",
            "tests/t.sh",
            "--reason",
            "shorter",
            "--by",
            "t",
            "--confirm",
            &sum,
        ],
    );
    assert_eq!(r.code, 0, "{}", r.all());
    write(&root, "tests/t.sh", "#!/bin/bash\ntrue\n");
    let r = gate(&root);
    assert_ne!(
        r.code,
        0,
        "an amendment whose `to` is not the file's content is refused:\n{}",
        r.all()
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        before,
        "nothing committed"
    );
    // The control: the edit the amendment recorded does pass.
    write(&root, "tests/t.sh", "#!/bin/bash\nexit 0\n");
    let r = gate(&root);
    assert_eq!(
        r.code,
        0,
        "the recorded edit passes, so the refusals were for the missing amendment:\n{}",
        r.all()
    );
}

// ── §9 the commit hook names the remote gate ─────────────────────────────────

fn pre_bash(root: &Path, command: &str) -> Run {
    let payload = serde_json::json!({
        "session_id": SESSION, "hook_event_name": "PreToolUse", "tool_name": "Bash",
        "cwd": root.to_str().unwrap(), "tool_input": { "command": command },
    });
    hexa_in(root, root, &["hook", "pre-bash"], &payload.to_string(), &[])
}

fn staged_project(test_remote: bool) -> (tempfile::TempDir, PathBuf) {
    let mut pj = serde_json::json!({ "name": "p", "id": "00000000-test" });
    if test_remote {
        pj["test"] =
            serde_json::json!({ "remote": "bash ci/remote.sh --commit {commit} --extra {cmd}" });
    }
    let (t, root) = crate_repo(&pj.to_string());
    write(&root, "src/a.rs", "pub fn helper() -> u32 { 7 }\n");
    git(&root, &["add", "src/a.rs"]);
    (t, root)
}

/// S17: Under test.remote the commit hook's refusal names the remote gate
#[test]
fn s17_the_commit_hook_names_the_remote_gate() {
    let (_t, root) = staged_project(true);
    let r = pre_bash(&root, "git commit -m x");
    assert_eq!(r.code, 2, "refused:\n{}", r.all());
    assert!(r.err.contains("hexa do gate"), "{}", r.err);
    assert!(
        r.err.contains("remote runner"),
        "says the gate runs on the remote runner:\n{}",
        r.err
    );
}

/// S18: Without test.remote the refusal does not mention a remote runner
#[test]
fn s18_no_remote_no_remote_wording() {
    let (_t, root) = staged_project(false);
    let r = pre_bash(&root, "git commit -m x");
    assert_eq!(r.code, 2, "refused:\n{}", r.all());
    assert!(r.err.contains("hexa do gate"), "{}", r.err);
    assert!(
        !r.err.contains("remote runner"),
        "no remote wording without test.remote:\n{}",
        r.err
    );
    // The control: the same project and command with test.remote set does name it.
    let pj = serde_json::json!({ "name": "p", "id": "00000000-test",
                                 "test": { "remote": "bash ci/remote.sh --commit {commit} --extra {cmd}" } });
    write(&root, ".hexa/project.json", &pj.to_string());
    let r = pre_bash(&root, "git commit -m x");
    assert!(
        r.err.contains("remote runner"),
        "the wording follows the setting:\n{}",
        r.err
    );
}
