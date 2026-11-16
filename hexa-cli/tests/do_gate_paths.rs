//! `hexa do gate --files` takes paths as written from where you stand (ADR-2611160730 §3).
//!
//! Written before the fix: from `apps/lintel`, `--files ../../docs/a.md` was turned into
//! `apps/lintel/../../docs/a.md` and refused as a path that may leave the repository. A path
//! is normalised first; only one that really ends up outside the repository is refused.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main"]);
    git(d, &["config", "user.email", "t@t"]);
    git(d, &["config", "user.name", "t"]);
    std::fs::create_dir_all(d.join("apps/sub")).unwrap();
    std::fs::create_dir_all(d.join("docs")).unwrap();
    std::fs::write(d.join("apps/sub/x.rs"), "// x\n").unwrap();
    std::fs::write(d.join("docs/a.md"), "a\n").unwrap();
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "init"]);
    std::fs::write(d.join("apps/sub/x.rs"), "// x v2\n").unwrap();
    std::fs::write(d.join("docs/a.md"), "a v2\n").unwrap();
    tmp
}

fn gate(cwd: &Path, files: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(["do", "gate", "--files"])
        .args(files)
        .args(["-e", "true", "--uncounted", "-m", "change"])
        .current_dir(cwd)
        .env("HEXA_HOME", cwd.join(".hexa-home-test"))
        .output()
        .expect("hexa runs")
}

fn committed(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = git(dir, &["show", "--name-only", "--format=", "HEAD"]).lines().map(str::to_string).collect();
    v.sort();
    v
}

#[test]
fn a_path_with_dot_dot_from_a_subdirectory_is_normalised_and_committed() {
    let tmp = repo();
    let d = tmp.path();
    let out = gate(&d.join("apps/sub"), &["x.rs", "../../docs/a.md"]);
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_eq!(committed(d), vec!["apps/sub/x.rs", "docs/a.md"]);
}

#[test]
fn an_absolute_path_inside_the_repository_is_accepted() {
    let tmp = repo();
    let d = tmp.path();
    let abs = d.join("docs/a.md");
    let out = gate(&d.join("apps"), &[abs.to_str().unwrap()]);
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_eq!(committed(d), vec!["docs/a.md"]);
}

#[test]
fn a_path_that_really_leaves_the_repository_is_refused_by_name() {
    let tmp = repo();
    let d = tmp.path();
    let out = gate(&d.join("apps/sub"), &["../../../outside.rs"]);
    assert!(!out.status.success());
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("outside.rs") && text.contains("outside the repository"), "{text}");
    assert_eq!(git(d, &["rev-list", "--count", "HEAD"]).trim(), "1", "nothing committed");
}
