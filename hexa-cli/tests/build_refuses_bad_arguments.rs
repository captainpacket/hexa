//! `hexa build` refuses an out-of-range `--designs` and a `--target` outside
//! the repository at the command line, before a run record, a loop gate or a
//! detached child exists.

use std::process::Command;

fn build(dir: &std::path::Path, extra: &[&str]) -> (bool, String) {
    let home = tempfile::tempdir().unwrap();
    let mut args = vec!["build", "a challenge", "--gate", "true"];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(&args)
        .current_dir(dir)
        .env("HOME", home.path())
        .env("HEXA_HOME", home.path().join(".hexa"))
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

#[test]
fn out_of_range_designs_and_an_escaping_target_are_refused() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join(".git")).unwrap();
    let (ok, t) = build(d.path(), &["--target", "src", "--designs", "9"]);
    assert!(!ok && t.contains("--designs 9 is out of range"), "{t}");
    let (ok, t) = build(d.path(), &["--target", "../escape", "--detach"]);
    assert!(!ok && t.contains("--target ../escape is outside the repository"), "{t}");
    assert!(!d.path().join(".hexa/loop.json").exists(), "no loop gate was recorded");
}
