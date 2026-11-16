//! "Where am I / what next" is one verb: `hexa status`. `hexa go` is its
//! next-step line and nothing else, and `hexa hey` routes to them.
//!
//! What this replaced, each a reproduced fault:
//!
//! - `hexa status` listed a harden from another project as one of this
//!   repo's outcomes: run records live in one user-level directory, and the
//!   report read all of them.
//! - `hexa go` was a self-check of the hexa repository. In any project it
//!   said "no release binary — build with: cargo build -p hexa-cli
//!   --release"; in the hexa repo it started a detached release build no one
//!   asked for, then reported "a rebuild is in flight" for it; and it ran
//!   `cargo test --no-run`. It never read the loop.
//! - `hexa hey` exited 0 on a request it could not route, so a script could
//!   not tell "done" from "did not understand".

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HEXA: &str = env!("CARGO_BIN_EXE_hexa");
const SESSION: &str = "where-am-i-test-session";

/// One scratch world: a user-level state directory and the repos in it.
struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("hexa-home");
        std::fs::create_dir_all(home.join("runs")).unwrap();
        World { _tmp: tmp, home }
    }

    /// A git repo with a `.hexa/` directory and one commit.
    fn repo(&self, name: &str) -> PathBuf {
        let dir = self._tmp.path().join(name);
        std::fs::create_dir_all(dir.join(".hexa")).unwrap();
        std::fs::write(dir.join("README.md"), "x\n").unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["add", "-A"],
            &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"],
        ] {
            let o = Command::new("git").args(args).current_dir(&dir).output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        }
        dir.canonicalize().unwrap()
    }

    fn cmd(&self, dir: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(HEXA);
        c.args(args)
            .current_dir(dir)
            .env("HEXA_HOME", &self.home)
            .env("HEXA_SESSION_ID", SESSION)
            .env("HEXA_SESSION_PID", std::process::id().to_string())
            .env("NO_COLOR", "1");
        c
    }

    fn hexa(&self, dir: &Path, args: &[&str]) -> Output {
        self.cmd(dir, args).output().expect("run hexa")
    }

    fn status_json(&self, dir: &Path, extra: &[&str]) -> serde_json::Value {
        let mut args = vec!["status", "--json"];
        args.extend_from_slice(extra);
        let o = self.hexa(dir, &args);
        assert!(o.status.success(), "{}", text(&o));
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
    }

    /// A finished run record, as the verbs write it.
    fn record(&self, id: &str, worktree: &Path, target: &str, gate: &str, outcome: &str, ended: &str) {
        let rec = serde_json::json!({
            "id": id, "verb": "harden", "target": target, "pid": 1, "pid_start": 1,
            "pid_start_unknown": false, "started": "2026-11-16T08:00:00Z", "ended": ended,
            "outcome": outcome, "commit": null, "worktree": worktree.display().to_string(),
            "gate": gate, "log": null, "task": null, "gate_result": null,
        });
        std::fs::write(self.home.join("runs").join(format!("{id}.json")), rec.to_string()).unwrap();
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn targets(report: &serde_json::Value) -> Vec<String> {
    report["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["target"].as_str().unwrap_or("").to_string())
        .collect()
}

// ── status ──────────────────────────────────────────────────────────────────

#[test]
fn status_in_a_repo_shows_only_that_repos_runs_and_all_shows_every_one() {
    let w = World::new();
    let mine = w.repo("mine");
    let theirs = w.repo("theirs");
    w.record("r-mine", &mine.join("sub"), "src/mine.rs", "g", "passed", "2026-11-16T09:00:00Z");
    w.record("r-theirs", &theirs, "src/theirs.rs", "g", "failed", "2026-11-16T09:30:00Z");

    let r = w.status_json(&mine, &[]);
    assert_eq!(targets(&r), vec!["src/mine.rs"], "another repo's run is not this repo's outcome: {r}");
    assert_eq!(r["scope"], "repo", "{r}");
    assert_eq!(r["count"]["other_repos"], 1, "what was left out is counted, not hidden: {r}");

    let all = w.status_json(&mine, &["--all"]);
    let mut t = targets(&all);
    t.sort();
    assert_eq!(t, vec!["src/mine.rs", "src/theirs.rs"], "{all}");
    assert_eq!(all["scope"], "all", "{all}");
}

#[test]
fn a_run_in_a_linked_worktree_of_this_repo_is_this_repos() {
    let w = World::new();
    let mine = w.repo("mine");
    let wt = mine.parent().unwrap().join("mine-wt");
    let o = Command::new("git")
        .args(["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()])
        .current_dir(&mine)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    w.record("r-wt", &wt.canonicalize().unwrap(), "src/wt.rs", "g", "passed", "2026-11-16T09:00:00Z");
    let r = w.status_json(&mine, &[]);
    assert_eq!(targets(&r), vec!["src/wt.rs"], "{r}");
}

#[test]
fn the_next_step_follows_the_loop_state() {
    let w = World::new();
    let repo = w.repo("loop");
    let gate = "cargo test --test parser";

    // No gate recorded: record one.
    let r = w.status_json(&repo, &[]);
    assert_eq!(r["next"]["kind"], "record-gate", "{r}");
    assert!(r["next"]["command"].as_str().unwrap_or("").starts_with("hexa loop gate"), "{r}");

    let o = w.hexa(&repo, &["loop", "gate", gate]);
    assert!(o.status.success(), "{}", text(&o));

    // A task pending: name it.
    let o = w.hexa(&repo, &["loop", "task", "add", "write the parser"]);
    assert!(o.status.success(), "{}", text(&o));
    let r = w.status_json(&repo, &[]);
    assert_eq!(r["next"]["kind"], "task", "{r}");
    assert!(r["next"]["text"].as_str().unwrap().contains("write the parser"), "{r}");
    assert_eq!(r["next"]["command"], "hexa loop task done 1", "{r}");

    // The gate's last run in this repo failed: fix it, before any task.
    w.record("g-red", &repo, "src/parser.rs", gate, "failed", "2026-11-16T09:00:00Z");
    let r = w.status_json(&repo, &[]);
    assert_eq!(r["next"]["kind"], "fix-gate", "{r}");
    assert_eq!(r["next"]["command"], gate, "{r}");

    // A red run of the same gate in another repo says nothing about this one.
    let other = w.repo("other");
    w.record("g-other-red", &other, "src/parser.rs", gate, "failed", "2026-11-16T11:00:00Z");
    // A later green run here clears it.
    w.record("g-green", &repo, "src/parser.rs", gate, "passed", "2026-11-16T10:00:00Z");
    let r = w.status_json(&repo, &[]);
    assert_eq!(r["next"]["kind"], "task", "{r}");

    // Text form: the loop, then what is in flight, then the next step.
    let o = w.hexa(&repo, &["go"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("write the parser"), "{}", text(&o));
}

#[test]
fn status_text_answers_loop_then_in_flight_then_next() {
    let w = World::new();
    let repo = w.repo("order");
    let o = w.hexa(&repo, &["loop", "gate", "cargo test --test order"]);
    assert!(o.status.success(), "{}", text(&o));
    // `go` prints the next step alone; `status` prints the whole report. Ask
    // for the human form through the library's renderer by forcing a tty-less
    // text: `hexa` with no verb prints status text.
    let o = w.hexa(&repo, &[]);
    let t = text(&o);
    let at = |needle: &str| t.find(needle).unwrap_or_else(|| panic!("no {needle:?} in:\n{t}"));
    let (l, f, n) = (at("Loop:"), at("In flight"), at("Next:"));
    assert!(l < f && f < n, "the order is loop, in flight, next:\n{t}");
}

// ── go ──────────────────────────────────────────────────────────────────────

/// `go` starts nothing. A `cargo` first on PATH records any call to it; the
/// project looks like the hexa repo with no release binary, which is the
/// state that made the old `go` build.
#[test]
fn go_spawns_no_process() {
    let w = World::new();
    let repo = w.repo("hexa-like");
    std::fs::write(repo.join("Cargo.toml"), "[workspace]\nmembers = [\"hexa-cli\"]\n").unwrap();
    let bin = repo.parent().unwrap().join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let marker = repo.parent().unwrap().join("cargo-was-called");
    let fake = bin.join("cargo");
    std::fs::write(&fake, format!("#!/bin/sh\necho \"$@\" >> '{}'\n", marker.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let o = w
        .cmd(&repo, &["go"])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    // Give a detached child a moment to show itself.
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(o.status.success(), "{}", text(&o));
    assert!(!marker.exists(), "go called cargo: {}", std::fs::read_to_string(&marker).unwrap_or_default());
    let t = text(&o);
    for s in ["release binary", "rebuild", "cargo build"] {
        assert!(!t.contains(s), "go is not a hexa-repo self-check ({s:?}): {t}");
    }
    assert!(t.contains("hexa loop gate"), "go names the loop's next step: {t}");
}

/// The source holds the line too: no process is built on `go`'s path.
#[test]
fn go_source_builds_no_command() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/go.rs")).unwrap();
    for s in ["Command::new", ".spawn(", "process::Command"] {
        assert!(!src.contains(s), "go.rs must not start a process: found {s:?}");
    }
}

// ── hey ─────────────────────────────────────────────────────────────────────

#[test]
fn hey_with_no_match_exits_1_and_points_at_status() {
    let w = World::new();
    let repo = w.repo("hey");
    let o = w.cmd(&repo, &["hey", "make me a sandwich"]).stdin(std::process::Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("hexa status") && t.contains("--help"), "{t}");
}

#[test]
fn hey_routes_who_uses_a_path_to_graph_consumers() {
    use hexa_core::ports::intent::{route, Routed};
    for (ask, want) in [
        ("who uses src/a.rs", "graph consumers src/a.rs"),
        ("what depends on src/a.rs?", "graph consumers src/a.rs"),
        ("is it safe to delete src/a.rs", "graph consumers src/a.rs"),
        ("where am I", "status"),
        ("what next", "go"),
    ] {
        match route(ask) {
            Routed::Hexa(r) => {
                assert_eq!(r.command, want, "route({ask:?})");
                assert!(!r.destructive, "route({ask:?}) only reads");
            }
            other => panic!("route({ask:?}) = {other:?}"),
        }
    }
    // A path is needed; "who uses it" invents none.
    assert_eq!(route("who uses it"), Routed::None);
}

#[test]
fn hey_queue_is_refused_with_a_reason() {
    let w = World::new();
    let repo = w.repo("queue");
    let o = w.cmd(&repo, &["hey", "--queue", "where am I"]).stdin(std::process::Stdio::null()).output().unwrap();
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("no queue"), "{}", text(&o));
}

// ── the template ────────────────────────────────────────────────────────────

#[test]
fn the_template_names_status_and_do_gate_and_not_go_as_the_next_step() {
    let t = include_str!("../assets/templates/claude-md-hexa-section.md");
    assert!(t.contains("`hexa status`"), "the template names status");
    assert!(t.contains("hexa do gate --files"), "the template names do gate");
    assert!(!t.contains("`hexa go` suggests"), "go is not the next-action verb");
    // The hard rules are numbered once each.
    let rules: Vec<&str> = t
        .split("### Autonomous operation (HARD RULES)")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once(". **").map(|(n, _)| n))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for n in &rules {
        assert!(seen.insert(*n), "rule {n} is numbered twice: {rules:?}");
    }
    // The headings `hexa refresh` finds the managed section by.
    assert!(t.starts_with("## hexa — how to work in this project"));
    assert!(t.contains("\n## Development pipeline (gate-first)"));
}
