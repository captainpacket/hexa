//! `hexa harden` under `test.remote` gates every fix on the remote runner
//! (ADR-2611240900 §1, gaberger/hexa#74).
//!
//! Written from the scenarios S01 to S05 of `docs/specs/<this ADR>.json`,
//! before the code. The repository's origin is a bare repository beside it.
//! The runner is a local script that clones the commit it is sent from that
//! origin, as a build server would, and sets `ON_FAKE_RUNNER=1` for the
//! command. The gate passes only there: run here, it is red, and it leaves a
//! mark outside the repository. No network, no model: the frontier is a fake
//! that finds the bugs it is told to and "fixes" each by appending to the
//! target and writing a regression-test file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hexa_exec::adversarial::{run_review_gated_at, GateAt, HardenGates, ReviewReport};
use hexa_exec::ports::{Frontier, FrontierError};

const TARGET: &str = "src/target.txt";

#[derive(Clone, Copy, PartialEq)]
enum Fix {
    Good,
    Breaks,
}

struct FakeFrontier {
    fixes: Vec<Fix>,
    handed_out: AtomicBool,
}

impl FakeFrontier {
    fn new(fixes: Vec<Fix>) -> Arc<Self> {
        Arc::new(Self { fixes, handed_out: AtomicBool::new(false) })
    }
}

#[async_trait]
impl Frontier for FakeFrontier {
    async fn run(&self, prompt: &str, cwd: &Path, _t: Duration, _s: &str) -> Result<String, FrontierError> {
        if prompt.contains("adversarial code reviewer") {
            if self.handed_out.swap(true, Ordering::SeqCst) {
                return Ok(r#"{"findings":[]}"#.to_string());
            }
            let findings: Vec<String> = (1..=self.fixes.len())
                .map(|n| format!(r#"{{"title":"bug-{n}","location":"{TARGET}:{n}","description":"defect {n}","lens":"correctness"}}"#))
                .collect();
            return Ok(format!(r#"{{"findings":[{}]}}"#, findings.join(",")));
        }
        if prompt.contains("SKEPTICALLY verify") {
            return Ok(r#"{"is_real":true,"reasoning":"it is"}"#.to_string());
        }
        if prompt.contains("Fix this CONFIRMED bug") {
            let n = (1..=self.fixes.len()).find(|n| prompt.contains(&format!("title: bug-{n}\n"))).expect("the fix prompt names its bug");
            let target = cwd.join(TARGET);
            let mut body = std::fs::read_to_string(&target).unwrap();
            match self.fixes[n - 1] {
                Fix::Good => body.push_str(&format!("fix {n}\n")),
                Fix::Breaks => body.push_str(&format!("fix {n} BROKEN\n")),
            }
            std::fs::write(&target, body).unwrap();
            std::fs::create_dir_all(cwd.join("tests")).unwrap();
            std::fs::write(cwd.join(format!("tests/bug_{n}.txt")), format!("regression test for bug {n}\n")).unwrap();
            return Ok(format!("fixed bug-{n}"));
        }
        Ok(String::new())
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@x", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A repository on `main`, pushed to a bare origin it tracks, and a runner
/// beside it. The honest runner logs each commit it is sent, clones it, logs
/// the files the commit holds, prints the id, and runs the command there with
/// `ON_FAKE_RUNNER=1`. The dishonest one prints another id and runs nothing.
struct Fixture {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    origin: PathBuf,
    log: PathBuf,
    files: PathBuf,
    here: PathBuf,
    template: String,
}

impl Fixture {
    fn new(honest: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let dir = base.join("repo");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join(TARGET), "original\n").unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", "init"]);
        let origin = base.join("origin.git");
        git(&base, &["init", "-q", "--bare", origin.to_str().unwrap()]);
        git(&dir, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&dir, &["push", "-q", "-u", "origin", "main"]);
        let log = base.join("runner.log");
        let files = base.join("runner-files.log");
        let runner = base.join("runner.sh");
        let body = if honest {
            format!(
                "#!/bin/bash\necho \"$1\" >> '{log}'\nd=$(mktemp -d)\n\
                 git clone -q '{origin}' \"$d/c\" && cd \"$d/c\" && git checkout -q \"$1\" || exit 2\n\
                 git ls-tree -r --name-only HEAD | sed \"s|^|$1 |\" >> '{files}'\n\
                 echo \"runner: testing $(git rev-parse HEAD)\"\nON_FAKE_RUNNER=1 bash -c \"$2\"; rc=$?\ncd /; rm -rf \"$d\"; exit $rc\n",
                log = log.display(),
                files = files.display(),
                origin = origin.display()
            )
        } else {
            format!(
                "#!/bin/bash\necho \"$1\" >> '{log}'\necho \"runner: testing 0123456789abcdef0123456789abcdef01234567\"\nexit 0\n",
                log = log.display()
            )
        };
        std::fs::write(&runner, body).unwrap();
        executable(&runner);
        let here = base.join("ran-here");
        Fixture { template: format!("bash {} {{commit}} {{cmd}}", runner.display()), _tmp: tmp, dir, origin, log, files, here }
    }

    /// The gate: it passes only on the runner, and the target holds no BROKEN
    /// line. Run here, it marks `here` and fails.
    fn gate(&self) -> String {
        format!(
            "{{ test \"$ON_FAKE_RUNNER\" = 1 || touch '{}'; }} && test \"$ON_FAKE_RUNNER\" = 1 && ! grep -q BROKEN {TARGET}",
            self.here.display()
        )
    }

    fn fast_gate(&self) -> String {
        format!("{{ test \"$ON_FAKE_RUNNER\" = 1 || touch '{}'; }} && test \"$ON_FAKE_RUNNER\" = 1 && ! grep -q BROKEN {{touched}}", self.here.display())
    }

    fn tested(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn scratch_refs(&self) -> String {
        git(&self.origin, &["for-each-ref", "--format=%(refname)", "refs/heads/hexa-gate"])
    }

    fn commits(&self) -> Vec<String> {
        git(&self.dir, &["log", "--format=%s", "HEAD"]).lines().filter(|l| *l != "init").map(str::to_string).collect()
    }

    async fn harden(&self, fixes: Vec<Fix>, gates: HardenGates, at: GateAt) -> ReviewReport {
        run_review_gated_at(FakeFrontier::new(fixes), hexa_exec::default_git_history(), TARGET, &gates, at, &self.dir, Arc::new(|_| {})).await
    }
}

fn gates(gate: String, fast: Option<String>, batch: Option<usize>, baseline: bool) -> HardenGates {
    HardenGates { gate, fast_gate: fast, batch, baseline, lessons: Vec::new() }
}

/// S01: harden under test.remote gates each fix on the remote runner and keeps only the fixes that passed there
#[tokio::test]
async fn s01_each_fix_is_gated_on_the_runner_and_only_a_pass_is_kept() {
    let f = Fixture::new(true);
    let report = f.harden(vec![Fix::Good, Fix::Breaks], gates(f.gate(), None, None, true), GateAt::Remote(f.template.clone())).await;
    assert!(report.baseline.is_none(), "the baseline passed on the runner: {:?}", report.notes);
    assert_eq!(report.fixed, vec!["bug-1"], "only the fix that passed on the runner is kept; notes: {:?}", report.notes);
    assert_eq!(f.commits(), vec!["fix(harden): bug-1"], "one commit");
    assert_eq!(git(&f.dir, &["status", "--porcelain"]), "", "the failing fix was reverted");
    assert!(!std::fs::read_to_string(f.dir.join(TARGET)).unwrap().contains("BROKEN"));
    assert!(report.gate_run && report.gate_passed, "the final gate passed; notes: {:?}", report.notes);
    assert!(!f.here.exists(), "no gate ran here");
    let tested = f.tested();
    assert!(tested.len() >= 4, "baseline, two fixes and the final gate went to the runner: {tested:?}");
    assert_eq!(f.scratch_refs(), "", "every scratch ref is deleted");
    let head_tree = git(&f.dir, &["rev-parse", "HEAD^{tree}"]);
    let trees: BTreeSet<String> = tested.iter().map(|c| git(&f.origin, &["rev-parse", &format!("{c}^{{tree}}")])).collect();
    assert!(trees.contains(&head_tree), "the committed tree is one the runner tested: {head_tree} not in {trees:?}");
    let commits: Vec<String> = tested.iter().map(|c| git(&f.origin, &["log", "-1", "--format=%s", c])).collect();
    assert!(commits.iter().all(|s| !s.is_empty()), "each candidate reached origin: {commits:?}");
}

/// S02: With a fast gate, each fix's fast gate and each batch's full gate run on the remote runner
#[tokio::test]
async fn s02_fast_and_batch_gates_run_on_the_runner() {
    let f = Fixture::new(true);
    let report =
        f.harden(vec![Fix::Good, Fix::Good], gates(f.gate(), Some(f.fast_gate()), Some(2), false), GateAt::Remote(f.template.clone())).await;
    assert_eq!(report.fixed, vec!["bug-1", "bug-2"], "notes: {:?}", report.notes);
    assert!(report.full_gates >= 1 && report.gate_passed, "the batch's full gate passed; notes: {:?}", report.notes);
    assert!(!f.here.exists(), "no gate ran here");
    assert!(f.tested().len() >= 3, "two fast gates and a full gate: {:?}", f.tested());
    assert_eq!(f.scratch_refs(), "");
}

/// S03: Without a remote, the same gate runs here, where it is red, and the pass stops at the baseline
#[tokio::test]
async fn s03_without_a_remote_the_gate_runs_here() {
    let f = Fixture::new(true);
    let report = f.harden(vec![Fix::Good], gates(f.gate(), None, None, true), GateAt::Here).await;
    assert!(report.baseline.is_some(), "red here: {:?}", report.notes);
    assert!(report.fixed.is_empty() && f.commits().is_empty());
    assert!(f.here.exists(), "it ran here");
    assert!(f.tested().is_empty(), "the runner was never called");
}

/// S04: A runner that does not run the candidate keeps no fix and leaves no scratch ref
#[tokio::test]
async fn s04_a_runner_failure_keeps_no_fix() {
    let f = Fixture::new(false);
    let report = f.harden(vec![Fix::Good], gates(f.gate(), None, None, false), GateAt::Remote(f.template.clone())).await;
    assert!(report.fixed.is_empty(), "notes: {:?}", report.notes);
    assert!(f.commits().is_empty(), "nothing committed");
    assert!(!f.tested().is_empty(), "the runner was asked");
    assert!(report.notes.iter().any(|n| n.contains("runner failure")), "a note names the runner failure: {:?}", report.notes);
    assert!(!report.notes.iter().any(|n| n.contains("red first") && n.contains("bug-1") && !n.contains("not")), "{:?}", report.notes);
    assert_eq!(f.scratch_refs(), "");
    assert!(!f.here.exists());
}

/// S05: An untracked file the operator had before the pass is not pushed to the runner
#[tokio::test]
async fn s05_the_operators_untracked_file_is_not_pushed() {
    let f = Fixture::new(true);
    std::fs::write(f.dir.join("secret.txt"), "the operator's\n").unwrap();
    let report = f.harden(vec![Fix::Good], gates(f.gate(), None, None, false), GateAt::Remote(f.template.clone())).await;
    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
    let files = std::fs::read_to_string(&f.files).unwrap_or_default();
    assert!(!files.is_empty(), "the runner listed what it tested");
    assert!(!files.lines().any(|l| l.ends_with(" secret.txt")), "secret.txt reached the runner:\n{files}");
    assert!(files.lines().any(|l| l.ends_with(" tests/bug_1.txt")), "the fix's own new file did:\n{files}");
    assert!(f.dir.join("secret.txt").exists(), "and it is left where it was");
}
