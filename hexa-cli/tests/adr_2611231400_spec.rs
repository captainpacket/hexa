//! A remote gate commits red or green, tries itself first, and says what it
//! did in one object.
//!
//! Written from the accepted scenarios of `docs/specs/<this ADR>.json`, before
//! the code. Every case runs the built binary in a temp repository whose
//! origin is a bare repository beside it; the remote runner is a local script
//! that clones the pushed commit from that origin, as a build server would. No
//! network.
//!
//! Fixture ids are assembled at run time: the citation checker scans source,
//! and a literal id here would claim a decision exists.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ── harness ──────────────────────────────────────────────────────────────────

const SESSION: &str = "remote-gate-red-test";

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Run {
    fn all(&self) -> String {
        format!("{}{}", self.out, self.err)
    }

    /// The result object: the last non-empty line of stdout (§1).
    fn result(&self) -> serde_json::Value {
        let line = self
            .out
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_else(|| panic!("no stdout at all:\n{}", self.all()));
        serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("the last stdout line is not one JSON object ({e}): {line}\n{}", self.all()))
    }
}

fn hexa_in(dir: &Path, project: &Path, args: &[&str], stdin: &str) -> Run {
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
        .env_remove("HEXA_EVENTS")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("run hexa");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).to_string(),
        err: String::from_utf8_lossy(&o.stderr).to_string(),
    }
}

fn hexa(root: &Path, args: &[&str]) -> Run {
    hexa_in(root, root, args, "")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
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

/// A small Rust crate in a git repo with one commit on `main`, and a hexa
/// project file whose whole content is `project_json`.
fn crate_repo(project_json: &str) -> (tempfile::TempDir, PathBuf) {
    let (t, root) = tmp();
    write(&root, "Cargo.toml", "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    write(&root, "src/lib.rs", "pub mod a;\npub mod b;\n");
    write(&root, "src/a.rs", "pub fn helper() -> u32 { 1 }\n");
    write(&root, "src/b.rs", "use crate::a::helper;\npub fn user() -> u32 { helper() }\n");
    write(&root, ".gitignore", ".test-home/\ntarget/\n.hexa/loop.json\n.hexa/loop.lock\n.hexa/graph/\n");
    write(&root, ".hexa/project.json", project_json);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    (t, root)
}

// ── the remote ───────────────────────────────────────────────────────────────

/// A bare origin, and a runner script beside it. `honest` runners clone the
/// commit they are given from origin, print the id they checked out, run the
/// command there, and append the commit to `log`. A dishonest one runs the
/// command where it stands and prints a commit id that is not the one sent.
struct Remote {
    _dir: tempfile::TempDir,
    origin: PathBuf,
    log: PathBuf,
    template: String,
}

impl Remote {
    fn invocations(&self) -> usize {
        std::fs::read_to_string(&self.log).map(|s| s.lines().count()).unwrap_or(0)
    }

    fn tested(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Install a pre-receive hook on origin that refuses the refs `refuse`
    /// matches (a `case` pattern).
    fn refuse(&self, pattern: &str) {
        let hook = self.origin.join("hooks/pre-receive");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/bash\nwhile read old new ref; do\n  case \"$ref\" in {pattern}) echo \"refused by policy: $ref\" >&2; exit 1;; esac\ndone\nexit 0\n"
            ),
        )
        .unwrap();
        executable(&hook);
    }
}

fn remote_for(root: &Path, honest: bool) -> Remote {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    git(dir.path(), &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(root, &["remote", "add", "origin", origin.to_str().unwrap()]);
    let log = dir.path().join("runner.log");
    let runner = dir.path().join("runner.sh");
    let body = if honest {
        format!(
            "#!/bin/bash\necho \"$1\" >> '{log}'\nd=$(mktemp -d)\n\
             git clone -q '{origin}' \"$d/c\" && cd \"$d/c\" && git checkout -q \"$1\" || exit 2\n\
             echo \"runner: testing $(git rev-parse HEAD)\"\nbash -c \"$2\"; rc=$?\ncd /; rm -rf \"$d\"; exit $rc\n",
            log = log.display(),
            origin = origin.display()
        )
    } else {
        format!(
            "#!/bin/bash\necho \"$1\" >> '{log}'\necho \"runner: testing 0123456789abcdef0123456789abcdef01234567\"\nbash -c \"$2\"\n",
            log = log.display()
        )
    };
    std::fs::write(&runner, body).unwrap();
    executable(&runner);
    Remote { template: format!("bash {} {{commit}} {{cmd}}", runner.display()), origin, log, _dir: dir }
}

const GREEN: &str = "#!/bin/bash\ntouch ran-here\nprintf 'running 2 tests\\ntest a ... ok\\ntest b ... ok\\n\\ntest result: ok. 2 passed; 0 failed; 0 ignored\\n'\nexit 0\n";
const RED: &str = "#!/bin/bash\ntouch ran-here\nprintf 'running 2 tests\\ntest a ... ok\\ntest b ... FAILED\\n\\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\\n'\nexit 101\n";
const COMPILE: &str = "#!/bin/bash\ntouch ran-here\nprintf 'error[E0425]: cannot find function `helper2` in this scope\\nerror: could not compile `p` (test \"t\") due to 1 previous error\\n'\nexit 101\n";
const ZERO: &str = "#!/bin/bash\ntouch ran-here\nprintf 'running 0 tests\\n\\ntest result: ok. 0 passed; 0 failed; 0 ignored\\n'\nexit 0\n";

/// A project with `test.remote` and the gate scripts, on branch `feat`
/// tracking origin, with an uncommitted change to `src/a.rs`.
fn remote_project_with(honest: bool) -> (tempfile::TempDir, PathBuf, Remote) {
    let (t, root) = crate_repo(r#"{"name":"p","id":"00000000-test","lifecycle_enforcement":"advisory"}"#);
    let remote = remote_for(&root, honest);
    let pj = serde_json::json!({ "name": "p", "id": "00000000-test", "lifecycle_enforcement": "advisory",
                                 "test": { "remote": remote.template } });
    write(&root, ".hexa/project.json", &serde_json::to_string_pretty(&pj).unwrap());
    write(&root, "ci/green.sh", GREEN);
    write(&root, "ci/red.sh", RED);
    write(&root, "ci/compile.sh", COMPILE);
    write(&root, "ci/zero.sh", ZERO);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "remote"]);
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["checkout", "-q", "-b", "feat"]);
    git(&root, &["push", "-q", "-u", "origin", "feat"]);
    write(&root, "src/a.rs", "pub fn helper() -> u32 { 42 }\n");
    (t, root, remote)
}

fn remote_project() -> (tempfile::TempDir, PathBuf, Remote) {
    remote_project_with(true)
}

fn head(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"])
}

fn origin_head(root: &Path, branch: &str) -> Option<String> {
    git(root, &["ls-remote", "origin", &format!("refs/heads/{branch}")])
        .split_whitespace()
        .next()
        .map(str::to_string)
}

fn gate_refs(root: &Path) -> String {
    git(root, &["ls-remote", "origin", "refs/heads/hexa-gate/*"])
}

fn gate(root: &Path, files: &[&str], evidence: &str, extra: &[&str]) -> Run {
    let mut args = vec!["do", "gate", "--files"];
    args.extend_from_slice(files);
    args.extend_from_slice(&["-e", evidence, "-m", "change"]);
    args.extend_from_slice(extra);
    hexa(root, &args)
}

fn with_new_test(root: &Path) {
    write(root, "tests/t.rs", "#[test]\nfn b() { assert_eq!(p::a::helper(), 42); }\n");
}

fn tests_of(v: &serde_json::Value) -> (u64, u64) {
    (
        v["tests"]["passed"].as_u64().unwrap_or(u64::MAX),
        v["tests"]["failed"].as_u64().unwrap_or(u64::MAX),
    )
}

// ── §1 one result object ─────────────────────────────────────────────────────

/// S01: A remote pass ends with one result object naming the commit, the remote head and the counts
#[test]
fn s01_a_remote_pass_ends_with_one_result_object() {
    let (_t, root, _remote) = remote_project();
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &["--no-land"]);
    assert_eq!(r.code, 0, "the remote gate passed:\n{}", r.all());
    let v = r.result();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["mode"], "remote", "{v}");
    assert_eq!(v["expect"], "green", "{v}");
    let now = head(&root);
    assert_eq!(v["commit"].as_str(), Some(now.as_str()), "the commit is the new HEAD, in full: {v}");
    assert_eq!(v["remote_head"].as_str(), Some(now.as_str()), "{v}");
    assert_eq!(tests_of(&v), (2, 0), "counted by the evidence reader: {v}");
    assert_eq!(v["files"], serde_json::json!(["src/a.rs"]), "{v}");
    assert_eq!(v["branch"], "feat", "{v}");
}

/// S02: A failing remote gate's result object carries code, hint, field, retryable and the first failure
#[test]
fn s02_a_failing_remote_gate_says_why_in_its_object() {
    let (_t, root, _remote) = remote_project();
    let before = head(&root);
    let r = gate(&root, &["src/a.rs"], "bash ci/red.sh", &[]);
    assert_ne!(r.code, 0, "a failing gate fails:\n{}", r.all());
    let v = r.result();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["code"], "remote-gate-failed", "{v}");
    assert!(v["hint"].as_str().is_some_and(|h| !h.trim().is_empty()), "a hint: {v}");
    assert_eq!(v["field"], "--evidence", "{v}");
    assert_eq!(v["retryable"], false, "{v}");
    assert_eq!(tests_of(&v), (1, 1), "{v}");
    assert!(
        v["first_failure"].as_str().is_some_and(|f| f.contains("test b ... FAILED")),
        "the first failure: {v}"
    );
    assert_eq!(head(&root), before, "HEAD has not moved");
    assert!(!git(&root, &["status", "--porcelain", "--", "src/a.rs"]).is_empty(), "still uncommitted");
}

/// S03: An uncounted remote pass says uncounted in its object
#[test]
fn s03_an_uncounted_pass_says_so() {
    let (_t, root, _remote) = remote_project();
    let r = gate(&root, &["src/a.rs"], "test -f src/a.rs", &["--uncounted", "--no-land"]);
    assert_eq!(r.code, 0, "{}", r.all());
    let v = r.result();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["tests"], serde_json::json!({ "uncounted": true }), "{v}");
}

/// S04: A remote gate that ran zero tests is vacuous, and commits nothing
#[test]
fn s04_zero_tests_is_vacuous() {
    let (_t, root, _remote) = remote_project();
    let before = head(&root);
    let r = gate(&root, &["src/a.rs"], "bash ci/zero.sh", &[]);
    assert_ne!(r.code, 0, "{}", r.all());
    let v = r.result();
    assert_eq!(v["code"], "vacuous-gate", "{v}");
    assert_eq!(v["field"], "--evidence", "{v}");
    assert_eq!(head(&root), before, "HEAD has not moved");
}

// ── §2 the scratch ref ───────────────────────────────────────────────────────

/// S05: The runner is sent the commit on the scratch ref, and the scratch ref is gone after a pass
#[test]
fn s05_the_scratch_ref_carries_the_candidate_and_is_deleted() {
    let (_t, root, remote) = remote_project();
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &["--no-land"]);
    assert_eq!(r.code, 0, "{}", r.all());
    let commit = r.result()["commit"].as_str().unwrap_or_default().to_string();
    assert_eq!(commit.len(), 40, "a full commit id: {}", r.out);
    assert!(remote.tested().lines().any(|l| l.trim() == commit), "the runner was sent {commit}: {}", remote.tested());
    assert_eq!(gate_refs(&root), "", "no scratch ref is left");
}

/// S06: A runner whose output does not name the commit is refused, and the scratch ref is gone
#[test]
fn s06_a_runner_that_does_not_name_the_commit_is_refused() {
    let (_t, root, remote) = remote_project_with(false);
    let before = head(&root);
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &[]);
    assert_ne!(r.code, 0, "never a pass:\n{}", r.all());
    assert!(remote.invocations() >= 1, "it went to the runner");
    let v = r.result();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["code"], "remote-runner-wrong-commit", "{v}");
    assert_eq!(v["retryable"], true, "{v}");
    assert_eq!(head(&root), before, "HEAD has not moved");
    assert_eq!(gate_refs(&root), "", "no scratch ref is left");
}

// ── §3 the remote says so ────────────────────────────────────────────────────

/// S07: A remote pass reads the branch's head back from the remote
#[test]
fn s07_a_pass_reads_the_remote_head_back() {
    let (_t, root, _remote) = remote_project();
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &["--no-land"]);
    assert_eq!(r.code, 0, "{}", r.all());
    let v = r.result();
    let on_origin = origin_head(&root, "feat");
    assert_eq!(v["commit"].as_str(), on_origin.as_deref(), "origin's feat is the commit: {v}");
    assert_eq!(v["remote_head"].as_str(), on_origin.as_deref(), "and the object says what origin said: {v}");
}

/// S08: A refused branch push fails the verb with push-refused
#[test]
fn s08_a_refused_branch_push_is_named() {
    let (_t, root, remote) = remote_project();
    let before_origin = origin_head(&root, "feat");
    remote.refuse("refs/heads/feat");
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &["--no-land"]);
    assert_ne!(r.code, 0, "a pass the remote did not take is not a pass:\n{}", r.all());
    let v = r.result();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["code"], "push-refused", "{v}");
    assert_eq!(v["retryable"], true, "{v}");
    assert_eq!(origin_head(&root, "feat"), before_origin, "origin's feat is unchanged");
    assert_eq!(gate_refs(&root), "", "no scratch ref is left");
}

/// S09: A refused scratch push commits nothing
#[test]
fn s09_a_refused_scratch_push_commits_nothing() {
    let (_t, root, remote) = remote_project();
    let before = head(&root);
    remote.refuse("*");
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &[]);
    assert_ne!(r.code, 0, "{}", r.all());
    let v = r.result();
    assert_eq!(v["code"], "push-refused", "{v}");
    assert_eq!(head(&root), before, "HEAD has not moved");
    assert_eq!(remote.invocations(), 0, "the runner was never invoked");
}

// ── §4 --expect-red ──────────────────────────────────────────────────────────

/// S10: --expect-red commits tests that fail on assertions
#[test]
fn s10_expect_red_commits_assertion_failures() {
    let (_t, root, _remote) = remote_project();
    git(&root, &["checkout", "-q", "--", "src/a.rs"]);
    with_new_test(&root);
    let before = head(&root);
    let main_before = git(&root, &["rev-parse", "main"]);
    let r = gate(&root, &["tests/t.rs"], "bash ci/red.sh", &["--expect-red"]);
    assert_eq!(r.code, 0, "a red of assertion failures is committed:\n{}", r.all());
    let now = head(&root);
    assert_eq!(git(&root, &["rev-parse", "HEAD^"]), before, "one commit on top");
    assert!(git(&root, &["show", "--name-only", "--format=", "HEAD"]).lines().any(|l| l == "tests/t.rs"), "it holds the test");
    assert_eq!(origin_head(&root, "feat").as_deref(), Some(now.as_str()), "pushed");
    let v = r.result();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["expect"], "red", "{v}");
    assert_eq!(tests_of(&v), (1, 1), "{v}");
    assert_eq!(git(&root, &["rev-parse", "main"]), main_before, "a red commit never lands");
}

/// S11: --expect-red refuses a compile error
#[test]
fn s11_expect_red_refuses_a_compile_error() {
    let (_t, root, _remote) = remote_project();
    git(&root, &["checkout", "-q", "--", "src/a.rs"]);
    with_new_test(&root);
    let before = head(&root);
    let r = gate(&root, &["tests/t.rs"], "bash ci/compile.sh", &["--expect-red"]);
    assert_ne!(r.code, 0, "a compile error is not a red test:\n{}", r.all());
    let v = r.result();
    assert_eq!(v["code"], "red-not-a-test-failure", "{v}");
    assert_eq!(head(&root), before, "HEAD has not moved");
    assert!(git(&root, &["status", "--porcelain", "--", "tests/t.rs"]).starts_with("??"), "still untracked");
}

/// S12: --expect-red refuses a gate that passes, and one that ran zero tests
#[test]
fn s12_expect_red_refuses_green_and_zero() {
    let (_t, root, _remote) = remote_project();
    git(&root, &["checkout", "-q", "--", "src/a.rs"]);
    with_new_test(&root);
    let before = head(&root);
    let r = gate(&root, &["tests/t.rs"], "bash ci/green.sh", &["--expect-red"]);
    assert_ne!(r.code, 0, "a passing gate is not red:\n{}", r.all());
    assert_eq!(r.result()["code"], "not-red", "{}", r.out);
    let r = gate(&root, &["tests/t.rs"], "bash ci/zero.sh", &["--expect-red"]);
    assert_ne!(r.code, 0, "zero tests is not red:\n{}", r.all());
    assert_eq!(r.result()["code"], "vacuous-gate", "{}", r.out);
    assert_eq!(head(&root), before, "HEAD has not moved");
}

/// S13: --expect-red cannot be combined with --uncounted
#[test]
fn s13_expect_red_and_uncounted_conflict() {
    let (_t, root, remote) = remote_project();
    with_new_test(&root);
    let r = gate(&root, &["tests/t.rs"], "bash ci/red.sh", &["--expect-red", "--uncounted"]);
    assert_ne!(r.code, 0, "{}", r.all());
    assert!(
        r.err.contains("--expect-red") && r.err.contains("--uncounted") && r.err.contains("cannot be used with"),
        "the two flags are refused together, by name:\n{}",
        r.err
    );
    assert_eq!(remote.invocations(), 0, "the runner was never invoked");
}

// ── §5 the red commit's next step ────────────────────────────────────────────

/// S14: A red commit names `hexa spec red` as its next step
#[test]
fn s14_a_red_commit_names_spec_red_next() {
    let (_t, root, _remote) = remote_project();
    git(&root, &["checkout", "-q", "--", "src/a.rs"]);
    with_new_test(&root);
    let r = gate(&root, &["tests/t.rs"], "bash ci/red.sh", &["--expect-red"]);
    assert_eq!(r.code, 0, "{}", r.all());
    let v = r.result();
    let commit = head(&root);
    let next = v["next"].as_str().unwrap_or_default();
    assert!(
        next.starts_with("hexa spec red ") && next.ends_with(&commit),
        "next names spec red and the full commit: {v}"
    );
}

/// S15: A red commit writes no spec record
#[test]
fn s15_a_red_commit_writes_no_spec() {
    let (_t, root, _remote) = remote_project();
    git(&root, &["checkout", "-q", "--", "src/a.rs"]);
    let id = format!("ADR-{}", "2701230000");
    let spec = format!("docs/specs/{id}.json");
    write(&root, &spec, &format!("{{\"adr\":\"{id}\",\"specs\":[]}}\n"));
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "spec"]);
    git(&root, &["push", "-q"]);
    let bytes = read(&root, &spec);
    with_new_test(&root);
    let r = gate(&root, &["tests/t.rs"], "bash ci/red.sh", &["--expect-red"]);
    assert_eq!(r.code, 0, "{}", r.all());
    assert_eq!(r.result()["expect"], "red", "{}", r.out);
    assert_eq!(read(&root, &spec), bytes, "the spec record is untouched");
    assert_eq!(git(&root, &["status", "--porcelain", "--untracked-files=all"]), "", "no other change in the tree");
}

// ── §6 --dry-run ─────────────────────────────────────────────────────────────

/// S16: --dry-run says what it would do and does none of it
#[test]
fn s16_dry_run_does_nothing() {
    let (_t, root, remote) = remote_project();
    let before = head(&root);
    let refs = git(&root, &["ls-remote", "origin"]);
    let r = gate(&root, &["src/a.rs"], "bash ci/green.sh", &["--dry-run"]);
    assert_eq!(r.code, 0, "{}", r.all());
    let v = r.result();
    assert_eq!(v["mode"], "dry-run", "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["would"]["parent"].as_str(), Some(before.as_str()), "{v}");
    assert!(v["would"]["run"].as_str().is_some_and(|c| c.contains("runner.sh")), "the test.remote command: {v}");
    assert_eq!(head(&root), before, "HEAD has not moved");
    assert_eq!(git(&root, &["ls-remote", "origin"]), refs, "origin is unchanged");
    assert_eq!(remote.invocations(), 0, "the runner was never invoked");
    assert!(!git(&root, &["status", "--porcelain", "--", "src/a.rs"]).is_empty(), "still uncommitted");
}

/// S17: A dry run still refuses what the gate would refuse
#[test]
fn s17_a_dry_run_refuses_what_the_gate_would() {
    let (_t, root, _remote) = remote_project();
    let r = gate(&root, &["src/b.rs"], "bash ci/green.sh", &["--dry-run"]);
    assert_ne!(r.code, 0, "{}", r.all());
    let v = r.result();
    assert_eq!(v["code"], "nothing-to-commit", "{v}");
    assert_eq!(v["field"], "--files", "{v}");
}

// ── §7 the hook ──────────────────────────────────────────────────────────────

fn pre_bash(root: &Path, command: &str) -> Run {
    let payload = serde_json::json!({
        "session_id": SESSION, "hook_event_name": "PreToolUse", "tool_name": "Bash",
        "cwd": root.to_str().unwrap(), "tool_input": { "command": command },
    });
    hexa_in(root, root, &["hook", "pre-bash"], &payload.to_string())
}

fn staged_project(test_remote: bool, rel: &str) -> (tempfile::TempDir, PathBuf) {
    let mut pj = serde_json::json!({ "name": "p", "id": "00000000-test" });
    if test_remote {
        pj["test"] = serde_json::json!({ "remote": "bash ci/remote.sh --commit {commit} --extra {cmd}" });
    }
    let (t, root) = crate_repo(&pj.to_string());
    write(&root, rel, "pub fn helper() -> u32 { 7 }\n");
    git(&root, &["add", rel]);
    (t, root)
}

/// S18: The commit hook names --expect-red
#[test]
fn s18_the_hook_names_expect_red() {
    for remote in [true, false] {
        let (_t, root) = staged_project(remote, "src/a.rs");
        let r = pre_bash(&root, "git commit -m x");
        assert_eq!(r.code, 2, "refused (test.remote: {remote}):\n{}", r.all());
        assert!(r.err.contains("hexa do gate"), "{}", r.err);
        assert!(r.err.contains("--expect-red"), "names the tests-first path (test.remote: {remote}):\n{}", r.err);
    }
}

/// S19: A prose-only commit is not refused and names nothing
#[test]
fn s19_a_prose_commit_names_nothing() {
    let (_t, root) = staged_project(true, "docs/x.md");
    let r = pre_bash(&root, "git commit -m x");
    assert_eq!(r.code, 0, "prose is not refused:\n{}", r.all());
    assert!(!r.all().contains("--expect-red"), "{}", r.all());
    // The control: a source commit in the same project does name it.
    write(&root, "src/a.rs", "pub fn helper() -> u32 { 8 }\n");
    git(&root, &["add", "src/a.rs"]);
    let r = pre_bash(&root, "git commit -m x");
    assert!(r.err.contains("--expect-red"), "the source commit names it:\n{}", r.all());
}
