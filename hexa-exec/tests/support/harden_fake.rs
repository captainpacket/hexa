//! A fake frontier and a scratch repository for the harden tests of
//! ADR-2611160730. The fake answers the hunt with `bugs` findings, confirms
//! each, and "fixes" bug `n` by running `fix(cwd, n)` — whatever a scenario
//! needs a fix (or a person working beside it) to do.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hexa_exec::ports::{Frontier, FrontierError};

pub const TARGET: &str = "src/target.txt";

/// What a fix does to the tree for bug `n`.
pub type FixAction = Arc<dyn Fn(&Path, usize) + Send + Sync>;

pub struct Fake {
    bugs: usize,
    handed_out: AtomicBool,
    fix: FixAction,
    /// When set, the hunt answers come from here, one per lens call, in
    /// order (each a `findings` array body); lenses past the end find none.
    hunts: Option<std::sync::Mutex<std::collections::VecDeque<String>>>,
    /// Every verify prompt the pass sent.
    pub verified: std::sync::Mutex<Vec<String>>,
    /// Every call the pass made, of any kind.
    pub calls: AtomicUsize,
}

impl Fake {
    pub fn new(bugs: usize, fix: FixAction) -> Arc<Fake> {
        Arc::new(Fake {
            bugs,
            handed_out: AtomicBool::new(false),
            fix,
            hunts: None,
            verified: Default::default(),
            calls: AtomicUsize::new(0),
        })
    }

    /// A fake whose lenses answer with `hunts`, one per lens call. Bug `n` is
    /// fixed when the fix prompt names `title: bug-{n}`.
    pub fn with_hunts(bugs: usize, hunts: Vec<String>, fix: FixAction) -> Arc<Fake> {
        Arc::new(Fake {
            bugs,
            handed_out: AtomicBool::new(false),
            fix,
            hunts: Some(std::sync::Mutex::new(hunts.into())),
            verified: Default::default(),
            calls: AtomicUsize::new(0),
        })
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Frontier for Fake {
    async fn run(&self, prompt: &str, cwd: &Path, _t: Duration, _s: &str) -> Result<String, FrontierError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if prompt.contains("adversarial code reviewer") {
            if let Some(hunts) = &self.hunts {
                let next = hunts.lock().unwrap().pop_front().unwrap_or_default();
                return Ok(format!(r#"{{"findings":[{next}]}}"#));
            }
            if self.handed_out.swap(true, Ordering::SeqCst) {
                return Ok(r#"{"findings":[]}"#.to_string());
            }
            let findings: Vec<String> = (1..=self.bugs)
                .map(|n| format!(r#"{{"title":"bug-{n}","location":"{TARGET}:{n}","description":"defect {n}","lens":"correctness"}}"#))
                .collect();
            return Ok(format!(r#"{{"findings":[{}]}}"#, findings.join(",")));
        }
        if prompt.contains("SKEPTICALLY verify") {
            self.verified.lock().unwrap().push(prompt.to_string());
            return Ok(r#"{"is_real":true,"reasoning":"it is"}"#.to_string());
        }
        if prompt.contains("Fix this CONFIRMED bug") {
            let n = (1..=self.bugs)
                .find(|n| prompt.to_ascii_lowercase().contains(&format!("title: bug-{n}\n")))
                .expect("the fix prompt names its bug");
            (self.fix)(cwd, n);
            return Ok(format!("fixed bug-{n}"));
        }
        Ok(String::new())
    }
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Commit everything staged-or-not as `who`, the way a person beside the
/// pass would.
pub fn commit_as(dir: &Path, who: &str, msg: &str) -> String {
    git(dir, &["add", "-A", "--", "."]);
    let name = format!("user.name={who}");
    let email = format!("user.email={who}@example.com");
    let out = Command::new("git")
        .args(["-c", &name, "-c", &email, "commit", "-qm", msg])
        .env("GIT_COMMITTER_NAME", who)
        .env("GIT_COMMITTER_EMAIL", format!("{who}@example.com"))
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "commit as {who}: {}", String::from_utf8_lossy(&out.stderr));
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A repository on a branch with `src/target.txt` and `notes.txt`
/// committed.
pub fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("repo");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join(TARGET), "original\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "notes\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["-c", "user.name=t", "-c", "user.email=t@x", "commit", "-qm", "init"]);
    (tmp, dir)
}

/// A gate a recognised runner wrote: one test, passed — unless the target
/// holds a line marked BROKEN.
pub const GATE: &str = "if grep -q BROKEN src/target.txt; then echo 'test result: FAILED. 0 passed; 1 failed;'; exit 1; fi; echo 'test result: ok. 1 passed; 0 failed;'";

/// A good fix: a line in the target and a regression test beside it.
pub fn good_fix() -> FixAction {
    Arc::new(|cwd: &Path, n: usize| {
        let target = cwd.join(TARGET);
        let mut body = std::fs::read_to_string(&target).unwrap();
        body.push_str(&format!("fix {n}\n"));
        std::fs::write(&target, body).unwrap();
        std::fs::create_dir_all(cwd.join("tests")).unwrap();
        std::fs::write(cwd.join(format!("tests/bug_{n}.txt")), format!("regression test for bug {n}\n")).unwrap();
    })
}

pub fn subjects(dir: &Path) -> Vec<String> {
    git(dir, &["log", "--format=%s", "HEAD"]).lines().map(str::to_string).collect()
}

pub fn is_ancestor(dir: &Path, sha: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", sha, "HEAD"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success()
}

/// A fix that only writes a test file: the bug stays.
pub fn test_only_fix() -> FixAction {
    Arc::new(|cwd: &Path, n: usize| {
        std::fs::create_dir_all(cwd.join("tests")).unwrap();
        std::fs::write(cwd.join(format!("tests/bug_{n}.txt")), format!("regression test for bug {n}\n")).unwrap();
    })
}
