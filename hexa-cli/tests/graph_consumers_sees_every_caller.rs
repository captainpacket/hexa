//! `hexa graph consumers` on a Rust crate: a module only its parent declares
//! is SAFE-TO-REMOVE, a fully qualified call blocks it, and a graph older than
//! the source says so.

use std::path::Path;
use std::process::{Command, Output};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn hexa(root: &Path, args: &[&str]) -> Output {
    let home = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(root)
        .env("HOME", home.path())
        .env_remove("HEXA_PROJECT_ROOT")
        .output()
        .unwrap()
}

fn consumers_json(root: &Path, target: &str) -> (serde_json::Value, String) {
    let out = hexa(root, &["graph", "consumers", target, "--json"]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let v = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}:\n{text}"));
    (v, String::from_utf8_lossy(&out.stderr).to_string())
}

fn fixture(caller: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    std::fs::create_dir(r.join(".git")).unwrap();
    write(r, "src/lib.rs", "pub mod domain;\npub mod app;\n");
    write(r, "src/domain/mod.rs", "pub mod util;\n");
    write(r, "src/domain/util.rs", "pub fn half(x: u32) -> u32 { x / 2 }\n");
    write(r, "src/app.rs", caller);
    let built = hexa(r, &["graph", "build"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    d
}

#[test]
fn a_module_only_its_parent_declares_is_safe_to_remove() {
    let d = fixture("pub fn run() -> u32 { 4 }\n");
    let (v, _) = consumers_json(d.path(), "src/domain/util.rs");
    assert_eq!(v["safe_to_remove"], true, "{v}");
    assert_eq!(v["declared_in"], serde_json::json!(["src/domain/mod.rs"]), "{v}");
    assert_eq!(v["stale"], false, "{v}");
}

#[test]
fn a_fully_qualified_call_blocks_removal() {
    let d = fixture("pub fn run() -> u32 { crate::domain::util::half(4) }\n");
    let (v, _) = consumers_json(d.path(), "src/domain/util.rs");
    assert_eq!(v["safe_to_remove"], false, "{v}");
}

#[test]
fn a_graph_older_than_the_source_is_reported_as_stale() {
    let d = fixture("pub fn run() -> u32 { 4 }\n");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write(d.path(), "src/app.rs", "pub fn run() -> u32 { crate::domain::util::half(4) }\n");
    let (v, stderr) = consumers_json(d.path(), "src/domain/util.rs");
    assert_eq!(v["stale"], true, "{v}");
    assert_eq!(v["newer_files"], serde_json::json!(["src/app.rs"]), "{v}");
    assert!(stderr.contains("hexa graph build"), "{stderr}");
}

#[test]
fn build_writes_under_hexa_and_not_to_the_repo_root() {
    let d = fixture("pub fn run() -> u32 { 4 }\n");
    assert!(d.path().join(".hexa/graph/graph.json").is_file());
    assert!(!d.path().join("graph-out").exists());
}

#[test]
fn a_legacy_graph_is_still_read() {
    let d = fixture("pub fn run() -> u32 { 4 }\n");
    let r = d.path();
    std::fs::create_dir_all(r.join("graph-out")).unwrap();
    std::fs::rename(r.join(".hexa/graph/graph.json"), r.join("graph-out/graph.json")).unwrap();
    let (v, _) = consumers_json(r, "src/domain/util.rs");
    assert_eq!(v["safe_to_remove"], true, "{v}");
}

#[test]
fn a_path_to_an_unknown_node_is_an_error() {
    let d = fixture("pub fn run() -> u32 { 4 }\n");
    let out = hexa(d.path(), &["graph", "path", "src/app.rs", "no_such_node"]);
    assert!(!out.status.success(), "exit 1 for an unknown node");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("node not found") && err.contains("no_such_node"), "{err}");

    let out = hexa(d.path(), &["graph", "path", "src/app.rs", "src/domain/util.rs"]);
    assert!(out.status.success(), "two known nodes with no path still exit 0");
}
