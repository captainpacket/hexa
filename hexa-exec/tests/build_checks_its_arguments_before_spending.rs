//! `hexa build` refuses an out-of-range `--designs` and a `--target` outside
//! the repository before any inference. `--designs 9` was cut to 4 without a
//! word, and `--target ../escape` started spending.

#[path = "support/harden_fake.rs"]
mod fake;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fake::repo;
use hexa_exec::adversarial::{check_build_args, run_build};
use hexa_exec::ports::{Frontier, FrontierError};

struct Counting(AtomicUsize);

#[async_trait]
impl Frontier for Counting {
    async fn run(&self, _p: &str, _c: &Path, _t: Duration, _s: &str) -> Result<String, FrontierError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("a design".to_string())
    }
}

const GATE: &str = "echo 'test result: ok. 1 passed; 0 failed;'";

#[tokio::test]
async fn out_of_range_designs_are_refused_before_any_call() {
    let (_tmp, dir) = repo();
    for n in [0, 1, 5, 9] {
        let f = Arc::new(Counting(AtomicUsize::new(0)));
        let r = run_build(f.clone(), "c", "src/lib", GATE, n, &dir, 30, 0).await;
        let why = r.stopped.unwrap_or_else(|| panic!("--designs {n} accepted"));
        assert!(why.contains("--designs") && why.contains("2") && why.contains("4"), "{why}");
        assert_eq!(f.0.load(Ordering::SeqCst), 0, "--designs {n}: no inference");
    }
}

#[tokio::test]
async fn a_target_outside_the_repository_is_refused_before_any_call() {
    let (tmp, dir) = repo();
    let outside = tmp.path().join("elsewhere");
    for target in ["../escape", "src/../../escape", outside.to_str().unwrap(), "/"] {
        let f = Arc::new(Counting(AtomicUsize::new(0)));
        let r = run_build(f.clone(), "c", target, GATE, 2, &dir, 30, 0).await;
        let why = r.stopped.unwrap_or_else(|| panic!("--target {target} accepted"));
        assert!(why.contains("--target"), "{why}");
        assert_eq!(f.0.load(Ordering::SeqCst), 0, "--target {target}: no inference");
    }
}

#[test]
fn a_target_inside_the_repository_is_accepted() {
    let (_tmp, dir) = repo();
    for target in ["src/lib", "./src/new", "src/a/../b", dir.join("src/x").to_str().unwrap(), "."] {
        assert!(check_build_args(target, 3, &dir).is_ok(), "{target}");
    }
    assert!(check_build_args("src/lib", 2, &dir).is_ok());
    assert!(check_build_args("src/lib", 4, &dir).is_ok());
}
