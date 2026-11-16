//! `graph consumers` must see every real caller, and must be able to say a
//! Rust module is safe to remove.
//!
//! Found by the functional audit: a fully qualified call
//! (`crate::domain::util::half()`) was not a use, and the `mod.rs` that
//! declares a module always counted as its consumer, so the verdict for a Rust
//! file could never be SAFE-TO-REMOVE.

use std::fs;
use std::path::Path;

use hexa_graph::context::{context_for, ContextOpts};
use hexa_graph::model::{EdgeKind, KnowledgeGraph};
use hexa_graph::semantic::NoopSemanticExtractor;
use hexa_graph::{build, BuildOpts};

fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

async fn graph_of(dir: &Path) -> KnowledgeGraph {
    let opts = BuildOpts { project_id: "t".into(), ..Default::default() };
    build(dir, opts, &NoopSemanticExtractor).await.unwrap()
}

fn crate_fixture(dir: &Path, caller_body: &str) {
    write(dir, "src/lib.rs", "pub mod domain;\npub mod app;\n");
    write(dir, "src/domain/mod.rs", "pub mod util;\n");
    write(dir, "src/domain/util.rs", "pub fn half(x: u32) -> u32 { x / 2 }\n");
    write(dir, "src/app.rs", caller_body);
}

#[tokio::test]
async fn a_fully_qualified_call_is_a_use() {
    let tmp = tempfile::tempdir().unwrap();
    crate_fixture(
        tmp.path(),
        "pub fn run() -> u32 { crate::domain::util::half(4) }\n",
    );
    let g = graph_of(tmp.path()).await;
    let ctx = context_for(&g, "src/domain/util.rs", ContextOpts::default()).unwrap();
    assert!(
        ctx.used_by.iter().any(|u| u.file == "src/app.rs" && u.entity == "half"),
        "app.rs calls crate::domain::util::half: {:?}",
        ctx.used_by
    );
    assert!(ctx.imported_by.iter().any(|f| f == "src/app.rs"), "{:?}", ctx.imported_by);
}

#[tokio::test]
async fn a_path_call_into_another_module_by_super_is_a_use() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "src/lib.rs", "pub mod a;\npub mod b;\n");
    write(tmp.path(), "src/a.rs", "pub fn alpha() {}\n");
    write(tmp.path(), "src/b.rs", "pub fn beta() { super::a::alpha(); }\n");
    let g = graph_of(tmp.path()).await;
    let ctx = context_for(&g, "src/a.rs", ContextOpts::default()).unwrap();
    assert!(ctx.used_by.iter().any(|u| u.file == "src/b.rs" && u.entity == "alpha"));
}

#[tokio::test]
async fn a_path_into_a_foreign_crate_links_nothing() {
    // `std::mem::take` must not link to a local `take` by name.
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "src/lib.rs", "pub mod a;\npub mod b;\n");
    write(tmp.path(), "src/a.rs", "pub fn take() {}\n");
    write(
        tmp.path(),
        "src/b.rs",
        "pub fn beta(v: &mut Vec<u8>) -> Vec<u8> { std::mem::take(v) }\n",
    );
    let g = graph_of(tmp.path()).await;
    let ctx = context_for(&g, "src/a.rs", ContextOpts::default()).unwrap();
    assert!(ctx.used_by.is_empty(), "{:?}", ctx.used_by);
}

#[tokio::test]
async fn a_mod_declaration_is_a_declaration_not_a_consumer() {
    let tmp = tempfile::tempdir().unwrap();
    crate_fixture(tmp.path(), "pub fn run() -> u32 { 4 }\n");
    let g = graph_of(tmp.path()).await;

    assert!(
        g.edges.iter().any(|e| e.kind == EdgeKind::Declares
            && e.src == "file:src/domain/mod.rs"
            && e.dst == "file:src/domain/util.rs"),
        "mod util; is a Declares edge"
    );
    let ctx = context_for(&g, "src/domain/util.rs", ContextOpts::default()).unwrap();
    assert!(ctx.imported_by.is_empty(), "{:?}", ctx.imported_by);
    assert!(ctx.used_by.is_empty(), "{:?}", ctx.used_by);
    assert_eq!(ctx.declared_in, vec!["src/domain/mod.rs".to_string()]);
}

#[tokio::test]
async fn a_source_newer_than_the_graph_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    crate_fixture(tmp.path(), "pub fn run() -> u32 { 4 }\n");
    write(tmp.path(), "target/x.rs", "pub fn skill() {}\n");
    let built = std::time::SystemTime::now();
    assert!(hexa_graph::newer_sources(tmp.path(), built, 10).is_empty());

    std::thread::sleep(std::time::Duration::from_millis(20));
    write(tmp.path(), "src/app.rs", "pub fn run() -> u32 { 5 }\n");
    write(tmp.path(), "target/x.rs", "pub fn skill2() {}\n");
    let newer = hexa_graph::newer_sources(tmp.path(), built, 10);
    assert_eq!(newer, vec!["src/app.rs".to_string()]);
}
