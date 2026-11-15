//! `hexa harden` checks its gate before it hunts, and keeps what the gate said
//! (ADR-2611151645). Written before the code.
//!
//! Observed 2026-11-15 on kairos: two passes spent 10m41s and 15m36s on 10
//! confirmed bugs; every fix "did not pass the gate", and the final gate on
//! the reverted tree failed too. The gate was red whatever the fixes did, the
//! pass never asked, and the gate's output (which `run_evidence` returns) was
//! thrown away, so nothing said why.
//!
//! The frontier is a fake that counts its calls. The gates are shell commands
//! that append a line to a counter file kept outside the tree, so a test can
//! say how many times the gate ran. The oracle is hand-written: literal counts
//! of gate runs and of frontier calls, and literal output lines.
//!
//! "0 tests run is a failure": the gate greps for at least 8 passes.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hexa_exec::adversarial::{run_review_gated, HardenGates, ReviewReport};
use hexa_exec::ports::{Frontier, FrontierError};

const TARGET: &str = "src/target.txt";

/// What the fake agent does when asked to fix bug `n`.
#[derive(Clone, Copy, PartialEq)]
enum Fix {
    /// Append a line to the target.
    Good,
    /// Append a line that fails the gate.
    Breaks,
    /// Append a line, and take away something outside the tree that the gate
    /// needs: what another process does to a shared checkout.
    Sabotage,
}

struct FakeFrontier {
    fixes: Vec<Fix>,
    flag: PathBuf,
    handed_out: AtomicBool,
    calls: AtomicUsize,
    fix_calls: AtomicUsize,
}

impl FakeFrontier {
    fn new(fixes: Vec<Fix>, flag: &Path) -> Arc<Self> {
        Arc::new(Self {
            fixes,
            flag: flag.to_path_buf(),
            handed_out: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            fix_calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl Frontier for FakeFrontier {
    async fn run(
        &self,
        prompt: &str,
        cwd: &Path,
        _t: Duration,
        _s: &str,
    ) -> Result<String, FrontierError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if prompt.contains("adversarial code reviewer") {
            // One lens reports every finding; the others report none.
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
            self.fix_calls.fetch_add(1, Ordering::SeqCst);
            let n = (1..=self.fixes.len())
                .find(|n| prompt.contains(&format!("title: bug-{n}\n")))
                .expect("the fix prompt names its bug");
            let target = cwd.join(TARGET);
            let mut body = std::fs::read_to_string(&target).unwrap();
            match self.fixes[n - 1] {
                Fix::Good => body.push_str(&format!("fix {n}\n")),
                Fix::Breaks => body.push_str(&format!("fix {n} BROKEN\n")),
                Fix::Sabotage => {
                    body.push_str(&format!("fix {n}\n"));
                    let _ = std::fs::remove_file(&self.flag);
                }
            }
            std::fs::write(&target, body).unwrap();
            return Ok(format!("fixed bug-{n}"));
        }
        Ok(String::new())
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repo with one tracked target, and beside it (outside the tree) a flag the
/// gates may need and a counter file the gates append to.
struct World {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    flag: PathBuf,
    count: PathBuf,
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let dir = root.join("repo");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join(TARGET), "original\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@x",
            "commit",
            "-qm",
            "init",
        ],
    );
    let flag = root.join("flag");
    std::fs::write(&flag, "up\n").unwrap();
    World {
        _tmp: tmp,
        dir,
        flag,
        count: root.join("count"),
    }
}

impl World {
    /// How many times a gate ran.
    fn gate_runs(&self) -> usize {
        std::fs::read_to_string(&self.count)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    /// Green while the flag is there and the target holds no BROKEN line.
    fn gate(&self) -> String {
        format!(
            "echo x >> {c}; test -f {f} || {{ echo 'the flag is gone'; exit 1; }}; if grep -q BROKEN src/target.txt; then echo 'found BROKEN in target'; exit 1; fi",
            c = self.count.display(),
            f = self.flag.display()
        )
    }
}

async fn pass(w: &World, frontier: Arc<FakeFrontier>, gate: &str, baseline: bool) -> ReviewReport {
    let gates = HardenGates {
        gate: gate.to_string(),
        fast_gate: None,
        batch: None,
        baseline,
    };
    run_review_gated(
        frontier,
        hexa_exec::default_git_history(),
        TARGET,
        &gates,
        &w.dir,
    )
    .await
}

#[tokio::test]
async fn a_red_gate_stops_the_pass_before_any_inference() {
    let w = world();
    let f = FakeFrontier::new(vec![Fix::Good, Fix::Good], &w.flag);
    let gate = format!(
        "echo x >> {}; echo line-A; echo the-gate-is-red; exit 3",
        w.count.display()
    );
    let report = pass(&w, f.clone(), &gate, true).await;

    assert_eq!(
        f.calls.load(Ordering::SeqCst),
        0,
        "no lens was asked, nothing was verified, no fix was tried"
    );
    assert_eq!(
        w.gate_runs(),
        2,
        "the gate ran, and ran once more to tell a flake from a red tree"
    );
    let base = report
        .baseline
        .as_ref()
        .expect("the baseline is in the report");
    assert!(!base.passed);
    assert_eq!(base.label, "baseline");
    assert!(
        base.tail.contains("the-gate-is-red"),
        "its output is kept: {:?}",
        base.tail
    );
    assert!(report.confirmed.is_empty() && report.fixed.is_empty());
    assert_eq!(report.gate_line(), "FAIL");
    assert!(
        report.verdict_line().contains("red before any fix"),
        "{}",
        report.verdict_line()
    );
    assert_eq!(
        git(&w.dir, &["status", "--porcelain"]),
        "",
        "the tree was not touched"
    );
}

#[tokio::test]
async fn a_gate_that_fails_once_and_then_passes_is_a_flake_and_the_pass_goes_on() {
    let w = world();
    let f = FakeFrontier::new(vec![Fix::Good], &w.flag);
    // red the first time it runs, green after
    let gate = format!(
        "n=$(wc -l < {c} 2>/dev/null || echo 0); echo x >> {c}; [ \"$n\" -ge 1 ]",
        c = w.count.display()
    );
    let report = pass(&w, f.clone(), &gate, true).await;

    assert!(
        f.calls.load(Ordering::SeqCst) > 0,
        "the pass went on to hunt"
    );
    assert!(
        report.baseline.is_none(),
        "a baseline that passed on its second run is not a stop"
    );
    assert!(
        report.flakes.iter().any(|x| x.contains("baseline")),
        "the flake is said: {:?}",
        report.flakes
    );
    assert_eq!(report.fixed, vec!["bug-1"], "notes: {:?}", report.notes);
}

#[tokio::test]
async fn a_failing_gate_keeps_the_last_sixty_lines_and_how_long_it_took() {
    let w = world();
    let f = FakeFrontier::new(vec![Fix::Good], &w.flag);
    let gate = "for i in $(seq 1 100); do echo out-$i; done; exit 1";
    let report = pass(&w, f, gate, true).await;

    let base = report.baseline.expect("red twice stops the pass");
    let lines: Vec<&str> = base.tail.lines().collect();
    assert_eq!(lines.len(), 60, "the last 60 lines, not the first");
    assert_eq!(lines.first(), Some(&"out-41"));
    assert_eq!(lines.last(), Some(&"out-100"));
    assert!(base.secs < 60, "the duration is recorded: {}", base.secs);
    assert_eq!(
        report.gate_runs.len(),
        2,
        "both baseline runs are listed: {:?}",
        report.gate_runs
    );
}

#[tokio::test]
async fn a_fix_that_fails_the_gate_says_what_the_gate_said() {
    let w = world();
    let f = FakeFrontier::new(vec![Fix::Breaks], &w.flag);
    let report = pass(&w, f, &w.gate(), true).await;

    assert!(report.fixed.is_empty());
    let note = report
        .notes
        .iter()
        .find(|n| n.contains("bug-1"))
        .expect("the failure is said");
    assert!(
        note.contains("found BROKEN in target"),
        "the note carries the gate's output: {note}"
    );
    let run = report
        .gate_runs
        .iter()
        .find(|r| r.label == "bug-1")
        .expect("the fix's gate run is kept");
    assert!(!run.passed && run.tail.contains("found BROKEN in target"));
    assert_eq!(
        git(&w.dir, &["status", "--porcelain"]),
        "",
        "the failed fix is reverted"
    );
}

#[tokio::test]
async fn two_failed_fixes_in_a_row_on_a_tree_that_is_red_without_them_stop_the_loop() {
    let w = world();
    // the first fix takes the flag away: nothing a revert can put back
    let f = FakeFrontier::new(vec![Fix::Sabotage, Fix::Breaks, Fix::Good], &w.flag);
    let report = pass(&w, f.clone(), &w.gate(), true).await;

    assert_eq!(
        f.fix_calls.load(Ordering::SeqCst),
        2,
        "the third fix was never tried"
    );
    let red = report
        .red_without_fix
        .as_ref()
        .expect("the red tree is reported");
    assert_eq!(red.label, "without any fix");
    assert!(
        !red.passed && red.tail.contains("the flag is gone"),
        "{:?}",
        red.tail
    );
    assert!(report.fixed.is_empty());
    assert_eq!(
        report.confirmed.len(),
        3,
        "the three confirmed findings stay listed"
    );
    assert!(
        report.verdict_line().contains("red without any fix"),
        "{}",
        report.verdict_line()
    );
    assert!(
        report.notes.iter().any(|n| n.contains("not judged")),
        "{:?}",
        report.notes
    );
    // baseline + fix 1 + fix 2 + the reverted tree
    assert_eq!(w.gate_runs(), 4);
}

#[tokio::test]
async fn two_failed_fixes_in_a_row_on_a_tree_that_is_green_without_them_do_not_stop_it() {
    let w = world();
    let f = FakeFrontier::new(vec![Fix::Breaks, Fix::Breaks, Fix::Good], &w.flag);
    let report = pass(&w, f.clone(), &w.gate(), true).await;

    assert_eq!(f.fix_calls.load(Ordering::SeqCst), 3, "the loop went on");
    assert!(report.red_without_fix.is_none());
    assert_eq!(report.fixed, vec!["bug-3"], "notes: {:?}", report.notes);
    // baseline + fix 1 + fix 2 + the reverted tree + fix 3 + final
    assert_eq!(w.gate_runs(), 6);
    assert_eq!(report.gate_line(), "PASS");
}

#[tokio::test]
async fn no_baseline_means_the_gate_is_not_run_before_the_hunt() {
    let w = world();
    let f = FakeFrontier::new(vec![], &w.flag);
    let gate = format!("echo x >> {}; exit 1", w.count.display());
    let report = pass(&w, f.clone(), &gate, false).await;

    assert!(
        f.calls.load(Ordering::SeqCst) > 0,
        "the pass hunted, as before"
    );
    assert!(report.baseline.is_none());
    assert!(report.baseline_skipped, "and says it did not check");
    assert_eq!(w.gate_runs(), 1, "only the final gate ran");
}

#[tokio::test]
async fn a_green_pass_costs_exactly_one_more_gate_run() {
    let w = world();
    let f = FakeFrontier::new(vec![Fix::Good, Fix::Good, Fix::Good], &w.flag);
    let report = pass(&w, f, &w.gate(), true).await;

    assert_eq!(
        report.fixed,
        vec!["bug-1", "bug-2", "bug-3"],
        "notes: {:?}",
        report.notes
    );
    // baseline + three fixes + final
    assert_eq!(w.gate_runs(), 5);
    assert!(report.baseline.is_none(), "a green baseline is not a stop");
    assert!(report
        .gate_runs
        .iter()
        .any(|r| r.label == "baseline" && r.passed));
    assert_eq!(report.gate_line(), "PASS");
    assert_eq!(
        std::fs::read_to_string(w.dir.join(TARGET)).unwrap(),
        "original\nfix 1\nfix 2\nfix 3\n"
    );
}
