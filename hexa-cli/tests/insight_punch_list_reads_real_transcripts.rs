//! `hexa insight punch-list` reads Claude Code's transcript shape
//! (`message.role`, `message.content`), under `$CLAUDE_CONFIG_DIR`, and only the
//! current repository's project folder unless `--all-projects` is given.
//!
//! The audit found it reporting 0 items on every run: it checked a top-level
//! `role` that a real transcript line never has.

use std::path::Path;
use std::process::Command;

fn assistant_line(text: &str) -> String {
    serde_json::json!({
        "type": "assistant",
        "sessionId": "s1",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
        },
    })
    .to_string()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn project_dir_name(p: &Path) -> String {
    p.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn punch_list(repo: &Path, cfg: &Path, extra: &[&str]) -> Vec<serde_json::Value> {
    let home = tempfile::tempdir().unwrap();
    let mut args = vec!["insight", "punch-list", "--json"];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(&args)
        .current_dir(repo)
        .env("HOME", home.path())
        .env("CLAUDE_CONFIG_DIR", cfg)
        .env_remove("HEXA_PROJECT_ROOT")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{e}:\n{text}\n{}", String::from_utf8_lossy(&out.stderr)));
    v["findings"].as_array().unwrap().clone()
}

const GAPS: &str = "Left over:\n\n1. Fix the login bug\n2. Review auth flow\n";

fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let canon = repo.path().canonicalize().unwrap();
    let mine = cfg.path().join("projects").join(project_dir_name(&canon)).join("t.jsonl");
    write(&mine, &format!("{}\n", assistant_line(GAPS)));
    let other = cfg.path().join("projects").join("-somewhere-else").join("t.jsonl");
    write(&other, &format!("{}\n", assistant_line("1. Fix the other bug\n2. Fix the third bug\n")));
    (repo, cfg)
}

#[test]
fn items_in_a_real_transcript_line_are_found() {
    let (repo, cfg) = setup();
    let found = punch_list(repo.path(), cfg.path(), &[]);
    let raws: Vec<&str> = found.iter().filter_map(|f| f["raw"].as_str()).collect();
    assert!(raws.iter().any(|r| r.contains("Fix the login bug")), "{found:?}");
}

#[test]
fn only_this_repository_is_scanned_by_default() {
    let (repo, cfg) = setup();
    let found = punch_list(repo.path(), cfg.path(), &[]);
    assert!(found.iter().all(|f| !f["raw"].as_str().unwrap_or("").contains("other bug")), "{found:?}");

    let all = punch_list(repo.path(), cfg.path(), &["--all-projects"]);
    assert!(all.iter().any(|f| f["raw"].as_str().unwrap_or("").contains("other bug")), "{all:?}");
}

#[test]
fn a_user_turn_is_not_read() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let canon = repo.path().canonicalize().unwrap();
    let line = serde_json::json!({"type": "user", "message": {"role": "user", "content": GAPS}});
    write(&cfg.path().join("projects").join(project_dir_name(&canon)).join("t.jsonl"), &format!("{line}\n"));
    assert!(punch_list(repo.path(), cfg.path(), &[]).is_empty());
}
