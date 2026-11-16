//! `hexa insight` — recursive insight routing (ADR-2026-04-14-2345).
//!
//! The agent emits `★ Insight` blocks during non-trivial turns. These blocks
//! carry architectural observations, actionable gaps, meta-patterns, and
//! failure-mode notes — the highest-signal self-observation the system
//! produces. This module provides the *extractor* — the first half of the
//! spinal cord that turns those blocks into durable, routable artifacts.
//!
//! Phase I1 (this module) covers:
//!
//! 1. The [`Insight`] struct + its supporting enums.
//! 2. The [`extract_insights`] function that parses `★ Insight` blocks from
//!    assistant text, tolerating both structured YAML bodies and legacy prose.
//! 3. PostToolUse hook wiring lives in `hexa-cli/src/commands/hook.rs` and
//!    calls into [`extract_insights`] directly.
//!
//! Later phases (I2–I5) add a classifier, router, and closure reconciler.
//! See `docs/workplans/wp-insight-routing.json` for the full plan.

pub mod extractor;

use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};

/// `hexa insight` subcommands. `punch-list --json` returns the gap and todo
/// items an agent acknowledged in its own output and routed nowhere. An
/// empty findings array, when no transcripts exist or none were found, is a
/// healthy result, not a failure.
#[derive(Subcommand)]
pub enum InsightAction {
    /// List gaps, todos and follow-ups enumerated in recent transcripts that carry no routing reference (`(out-of-scope)`, a task id, a draft path)
    PunchList {
        /// Emit findings as JSON
        /// (`{findings: [{line_no, raw, classification, severity}]}`).
        #[arg(long)]
        json: bool,
        /// Scan every project's transcripts, not only this repository's.
        #[arg(long)]
        all_projects: bool,
        /// Another Claude Code config directory to read transcripts from
        /// (its `projects/` folder), in addition to `$CLAUDE_CONFIG_DIR` and
        /// `~/.claude`. Repeatable.
        #[arg(long = "config-dir", value_name = "DIR")]
        config_dirs: Vec<std::path::PathBuf>,
    },
}

pub async fn run(action: InsightAction) -> anyhow::Result<()> {
    match action {
        InsightAction::PunchList { json, all_projects, config_dirs } => {
            punch_list_run(json, all_projects, &config_dirs).await
        }
    }
}

/// The transcript roots to scan, deduplicated, in order: each `--config-dir`,
/// then `$CLAUDE_CONFIG_DIR`, then `~/.claude`. Claude Code keeps a profile's
/// transcripts under its config dir, and a shell that runs hexa need not have
/// the variable a session set, so both the variable and the default are read.
fn transcript_roots(extra: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    use crate::commands::claude_paths::claude_projects_root;
    let home = dirs::home_dir();
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let mut roots: Vec<std::path::PathBuf> = extra.iter().map(|d| d.join("projects")).collect();
    roots.extend(claude_projects_root(config_dir.as_deref(), home.as_deref()));
    roots.extend(claude_projects_root(None, home.as_deref()));
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for r in roots {
        let key = r.canonicalize().unwrap_or_else(|_| r.clone());
        if r.is_dir() && !out.iter().any(|o| o.canonicalize().unwrap_or_else(|_| o.clone()) == key) {
            out.push(r);
        }
    }
    out
}

/// The repository root of the working directory: the nearest ancestor that
/// holds `.git` (a directory, or a worktree's file), canonicalized, because
/// Claude Code names a project folder after the path it was started in.
fn repo_root() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    cwd.ancestors().find(|d| d.join(".git").exists()).map(|d| d.to_path_buf()).or(Some(cwd))
}

/// The assistant text of one transcript line, or `None` for any other line.
///
/// Claude Code writes `{"type":"assistant","message":{"role":"assistant",
/// "content":[{"type":"text","text":…}]}}`. The role sits under `message`; a
/// top-level `role` (the shape this read before) is accepted too.
fn assistant_text(turn: &serde_json::Value) -> Option<String> {
    let body = turn.get("message").filter(|m| m.is_object()).unwrap_or(turn);
    let role = body.get("role").or_else(|| turn.get("role")).and_then(|v| v.as_str());
    if role != Some("assistant") {
        return None;
    }
    let content = body.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let blocks = content.as_array()?;
    Some(
        blocks
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()).is_none_or(|t| t == "text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Scan recent Claude Code transcript JSONL files for assistant turns and
/// extract punch-list items via the existing `extract_punch_list` pure
/// function in `commands::hook::punch_list`. Read-only.
///
/// Transcripts live at `<config dir>/projects/<project>/*.jsonl`. By default
/// only this repository's project folder is read; `--all-projects` reads
/// every one. Only files modified in the last hour are read, so the list
/// reflects current-session gaps, not historical ones the user has already
/// routed elsewhere.
async fn punch_list_run(json: bool, all_projects: bool, config_dirs: &[std::path::PathBuf]) -> anyhow::Result<()> {
    use crate::commands::claude_paths::project_dir_name;
    use crate::commands::hook::punch_list::{extract_punch_list, Reference};
    use std::path::PathBuf;

    let mut findings: Vec<serde_json::Value> = Vec::new();
    let only = if all_projects { None } else { repo_root().map(|r| project_dir_name(&r)) };

    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(60 * 60))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut recent_files: Vec<PathBuf> = Vec::new();
    for root in transcript_roots(config_dirs) {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for project in entries.flatten() {
            let project_path = project.path();
            if !project_path.is_dir() {
                continue;
            }
            if let Some(name) = &only {
                if project.file_name().to_string_lossy() != name.as_str() {
                    continue;
                }
            }
            let Ok(jsonls) = std::fs::read_dir(&project_path) else { continue };
            for j in jsonls.flatten() {
                let p = j.path();
                if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(meta) = j.metadata() else { continue };
                if meta.modified().is_ok_and(|m| m >= cutoff) {
                    recent_files.push(p);
                }
            }
        }
    }
    recent_files.sort();

    for path in recent_files {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        for line in content.lines() {
            let Ok(turn): Result<serde_json::Value, _> = serde_json::from_str(line) else { continue };
            let Some(text) = assistant_text(&turn) else { continue };
            for item in extract_punch_list(&text) {
                let unrouted = item
                    .references
                    .iter()
                    .all(|r| matches!(r, Reference::None));
                if !unrouted {
                    continue;
                }
                findings.push(serde_json::json!({
                    "line_no": item.line_no,
                    "raw": item.raw,
                    "classification": format!("{:?}", item.classification),
                    "severity": "warning",
                    "remediation": "add a task id, draft path, or (out-of-scope) tag",
                }));
            }
        }
    }

    if json {
        println!("{}", serde_json::json!({"findings": findings}));
    } else {
        println!("Punch list: {} unrouted item(s)", findings.len());
        for f in &findings {
            println!(
                "  L{}: {}",
                f.get("line_no").and_then(|v| v.as_u64()).unwrap_or(0),
                f.get("raw").and_then(|v| v.as_str()).unwrap_or("").lines().next().unwrap_or(""),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod punch_list_tests {
    use super::assistant_text;
    use serde_json::json;

    #[test]
    fn the_role_is_read_from_the_message() {
        let line = json!({"type":"assistant","message":{"role":"assistant","content":[
            {"type":"text","text":"a"},{"type":"tool_use","name":"x"},{"type":"text","text":"b"}]}});
        assert_eq!(assistant_text(&line).as_deref(), Some("a\nb"));
    }

    #[test]
    fn a_top_level_role_still_reads_and_a_user_turn_does_not() {
        assert_eq!(assistant_text(&json!({"role":"assistant","content":"x"})).as_deref(), Some("x"));
        assert_eq!(assistant_text(&json!({"message":{"role":"user","content":"x"}})), None);
        assert_eq!(assistant_text(&json!({"type":"summary"})), None);
    }
}

/// A single extracted insight — either structured (parsed from YAML) or a
/// best-effort fallback around legacy prose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    /// Stable identifier. For structured insights this is author-supplied;
    /// for fallback extractions it is synthesized as
    /// `insight-<session>-<turn:03>`.
    pub id: String,
    pub kind: InsightKind,
    pub content: String,
    pub route_to: RouteTarget,
    pub estimated_tier: Tier,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub source_session: String,
    pub source_turn: usize,
    pub created_at: DateTime<Utc>,
}

/// Classification of what an insight *is*. Informs routing but is not the
/// same thing — an `ActionableGap` can still route to `Memory` if it's a
/// duplicate, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsightKind {
    ArchitecturalObservation,
    ActionableGap,
    MetaPattern,
    FailureMode,
    Duplicate,
}

/// Where a classified insight should be materialized. Decided by the
/// classifier in I2 — the extractor only records what the author said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteTarget {
    Adr,
    Workplan,
    Memory,
    DuplicateOf(String),
    Skip,
}

/// Estimated execution tier per ADR-2026-04-12-0202 tiered inference routing.
/// The author estimates; later phases can override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    T1,
    T2,
    T3,
}

pub use extractor::extract_insights;
