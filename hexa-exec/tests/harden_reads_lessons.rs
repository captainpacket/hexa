//! Harden and build read what the project has learned (ADR-2611152035 §4).
//!
//! Written before the code. `do run` already put stored lessons in its prompt;
//! harden and build put none, so harden fixed seven instances of one stored
//! lesson (async staleness in a renderer) one at a time. Now the hunt, the fix
//! and the build prompts carry the lessons that bear on the target, and the fix
//! is told to fix the whole class when a bug is an instance of one.

#[path = "support/harden_fake.rs"]
mod fake;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fake::{good_fix, repo, Fake, GATE, TARGET};
use hexa_exec::adversarial::{lessons_block, run_build_with, run_review_gated, HardenGates};
use hexa_exec::ports::{Frontier, FrontierError};

/// The fake, with every prompt it was sent kept.
struct Recording {
    inner: Arc<Fake>,
    prompts: Mutex<Vec<String>>,
}

#[async_trait]
impl Frontier for Recording {
    async fn run(&self, prompt: &str, cwd: &Path, t: Duration, s: &str) -> Result<String, FrontierError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        self.inner.run(prompt, cwd, t, s).await
    }
}

fn lessons() -> Vec<(String, String)> {
    vec![
        ("lesson:async-staleness".into(), format!("in {TARGET} every await must check that its answer is still the newest")),
        ("lesson:unrelated".into(), "the docs site builds with mdbook".into()),
    ]
}

fn gates(lessons: Vec<(String, String)>) -> HardenGates {
    HardenGates { gate: GATE.to_string(), fast_gate: None, batch: None, baseline: false, lessons }
}

#[tokio::test]
async fn the_hunt_and_the_fix_carry_the_lessons_about_the_target() {
    let (_tmp, dir) = repo();
    let rec = Arc::new(Recording { inner: Fake::new(1, good_fix()), prompts: Mutex::new(Vec::new()) });
    let _ = run_review_gated(rec.clone(), hexa_exec::default_git_history(), TARGET, &gates(lessons()), &dir).await;

    let prompts = rec.prompts.lock().unwrap().clone();
    let hunt: Vec<&String> = prompts.iter().filter(|p| p.contains("adversarial code reviewer")).collect();
    let fix: Vec<&String> = prompts.iter().filter(|p| p.contains("Fix this CONFIRMED bug")).collect();
    assert!(!hunt.is_empty() && !fix.is_empty(), "the pass hunted and fixed");
    for p in hunt.iter().chain(fix.iter()) {
        assert!(p.contains("lesson:async-staleness"), "the lesson about the target is in the prompt:\n{p}");
    }
    assert!(fix[0].contains("whole class"), "the fix is told to fix the class, not the instance:\n{}", fix[0]);
}

#[tokio::test]
async fn with_nothing_learned_the_prompts_have_no_lessons_section() {
    let (_tmp, dir) = repo();
    let rec = Arc::new(Recording { inner: Fake::new(1, good_fix()), prompts: Mutex::new(Vec::new()) });
    let _ = run_review_gated(rec.clone(), hexa_exec::default_git_history(), TARGET, &gates(Vec::new()), &dir).await;
    assert!(rec.prompts.lock().unwrap().iter().all(|p| !p.contains("## Lessons")));
}

#[tokio::test]
async fn build_prompts_carry_the_lessons_too() {
    let (_tmp, dir) = repo();
    let rec = Arc::new(Recording { inner: Fake::new(0, good_fix()), prompts: Mutex::new(Vec::new()) });
    let _ = run_build_with(rec.clone(), "make it", "src", GATE, 2, &dir, 5, 0, Arc::new(|_| {}), &lessons()).await;
    let prompts = rec.prompts.lock().unwrap().clone();
    assert!(!prompts.is_empty(), "build asked the frontier something");
    assert!(prompts[0].contains("lesson:async-staleness"), "the first build prompt carries the lessons:\n{}", prompts[0]);
}

#[test]
fn lessons_about_the_target_come_first_and_the_block_is_bounded() {
    let mut many: Vec<(String, String)> = (0..40).map(|i| (format!("lesson:other-{i}"), format!("about something else {i}"))).collect();
    many.push(("lesson:here".into(), format!("bears on {TARGET}")));
    let block = lessons_block(&many, TARGET);
    let first = block.lines().find(|l| l.starts_with("- ")).unwrap();
    assert!(first.contains("lesson:here"), "the lesson naming the target leads: {block}");
    assert!(block.lines().filter(|l| l.starts_with("- ")).count() <= 8, "bounded: {block}");
    assert_eq!(lessons_block(&[], TARGET), "", "nothing learned, nothing said");
}
