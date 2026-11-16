//! Which writes are project source, and whether the loop's gate covers them
//! (ADR-2611160730 §4).
//!
//! The hooks hold an edit to project source, whether it comes through
//! Edit/Write or through a write-shaped Bash command, to one check: is a gate
//! recorded for the *current* task. Before this the check ran only when the
//! router had sized the prompt T2/T3, and any gate string in `loop.json`
//! satisfied it for the rest of the session, even for unrelated work.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Directories whose contents are never project source: build output,
/// dependencies, scratch, hexa's own state, git's.
const NEVER_SOURCE: &[&str] = &[
    "target", "node_modules", ".git", ".hexa", "tmp", "temp", "scratch", "scratchpad", ".cache",
];

/// Prose stays exempt: `docs/`, ADRs, and markdown anywhere. Exempt before
/// this change too, in effect: the old check only ran for sized prompts.
fn is_prose(rel: &Path) -> bool {
    if rel.components().next().is_some_and(|c| c.as_os_str() == "docs") {
        return true;
    }
    matches!(
        rel.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("md" | "mdx" | "markdown")
    )
}

/// True when `rel` (relative to the repo root) can never be project source,
/// whatever git says about it.
pub fn is_exempt(rel: &Path) -> bool {
    rel.components().any(|c| NEVER_SOURCE.iter().any(|n| c.as_os_str() == *n)) || is_prose(rel)
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

/// The top of the git work tree that holds `dir`, canonical.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let top = git(dir, &["rev-parse", "--show-toplevel"])?;
    std::fs::canonicalize(top.trim()).ok()
}

/// `p` with its longest existing ancestor canonicalized and the rest kept:
/// a file a command is about to create does not exist yet, but `/tmp` and
/// `/private/tmp` must still compare equal.
pub fn canonical_lenient(p: &Path) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
    let mut out = std::fs::canonicalize(&existing).unwrap_or(existing);
    for name in rest.into_iter().rev() {
        out.push(name);
    }
    out
}

/// The candidates that are project source in the repo at `root`, as paths
/// relative to it. A candidate counts when it is inside the repo, not
/// exempt, and either tracked by git (a directory counts when it holds a
/// tracked file) or a new file that is not ignored, in a directory below the
/// root that holds tracked files. A new file at the repo root does not
/// count: `cargo test > out.log` is not an edit to source.
pub fn project_source(root: &Path, candidates: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for c in candidates {
        let abs = canonical_lenient(c);
        let Ok(rel) = abs.strip_prefix(root) else { continue };
        if rel.as_os_str().is_empty() || is_exempt(rel) {
            continue;
        }
        let rel_s = rel.display().to_string();
        let tracked = git(root, &["ls-files", "--", &rel_s]).is_some_and(|s| !s.trim().is_empty());
        let counts = tracked || {
            let parent = rel.parent().filter(|p| !p.as_os_str().is_empty());
            parent.is_some_and(|p| {
                let p = p.display().to_string();
                git(root, &["ls-files", "--", &p]).is_some_and(|s| !s.trim().is_empty())
                    && git(root, &["check-ignore", "-q", "--", &rel_s]).is_none()
            })
        };
        if counts && !out.contains(&rel_s) {
            out.push(rel_s);
        }
    }
    out
}

/// Read a script named on a command line, at most
/// [`super::write_shape::SCRIPT_READ_CAP`] bytes.
pub fn read_script_bounded(p: &Path) -> Option<String> {
    use std::io::Read;
    let f = std::fs::File::open(p).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::new();
    f.take(super::write_shape::SCRIPT_READ_CAP as u64).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn ts(v: Option<&serde_json::Value>) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    v.and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
}

/// Whether this session's loop entry has a gate that covers the current task.
///
/// The rule (ADR-2611160730 §4: "tied to the current loop task, not to 'some
/// gate exists'"): a gate covers the work in front of you when
///
/// 1. this session's entry in `.hexa/loop.json` records a non-empty `gate`
///    (entries are per session, so another session's gate never counts);
/// 2. it carries `gate_at`, the time it was recorded or re-confirmed, which
///    `hexa loop gate` and a run that records its evidence as the gate write.
///    A gate from before `gate_at` existed must be recorded once more;
/// 3. `gate_at` is not older than the last step checked off with
///    `hexa loop task done`: the gate written for that step proved that step,
///    and the next one is new work until the gate is re-confirmed;
/// 4. `gate_at` is not older than the start of this Claude session, when the
///    hooks know it. Hosts with no session id share one loop entry, and a
///    gate left there by yesterday's session is not today's.
///
/// `Err` says which condition failed, in words for the person.
pub fn gate_is_current(
    entry: Option<&serde_json::Value>,
    session_started: Option<chrono::DateTime<chrono::FixedOffset>>,
) -> Result<(), String> {
    let Some(entry) = entry else {
        return Err("no loop is recorded for this session".to_string());
    };
    if entry.get("gate").and_then(|g| g.as_str()).is_none_or(|g| g.trim().is_empty()) {
        return Err("no gate is recorded for this session".to_string());
    }
    let Some(gate_at) = ts(entry.get("gate_at")) else {
        return Err("the gate was recorded before gates carried a time; re-confirm it".to_string());
    };
    let last_done = entry
        .get("tasks")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .filter(|t| t.get("status").and_then(|s| s.as_str()) == Some("done"))
        .filter_map(|t| ts(t.get("done_at")))
        .max();
    if let Some(done) = last_done {
        if gate_at < done {
            return Err("the gate was recorded before the last step was checked off, so it proved that step, not this one".to_string());
        }
    }
    if let Some(start) = session_started {
        if gate_at < start {
            return Err("the gate was recorded before this session started".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        chrono::DateTime::parse_from_rfc3339(s).ok()
    }

    #[test]
    fn prose_and_build_output_are_exempt_and_source_is_not() {
        for p in ["docs/adrs/ADR-1.md", "README.md", "crate/notes.MD", "target/foo", "a/node_modules/x.js", ".hexa/loop.json", "tmp/x.rs"] {
            assert!(is_exempt(Path::new(p)), "{p} is exempt");
        }
        for p in ["src/x.rs", "hexa-cli/src/main.rs", "Cargo.toml", "docsy/x.rs"] {
            assert!(!is_exempt(Path::new(p)), "{p} is source");
        }
    }

    #[test]
    fn the_gate_must_be_recorded_for_the_current_task() {
        let t0 = "2026-11-16T10:00:00+00:00";
        let t1 = "2026-11-16T11:00:00+00:00";
        let t2 = "2026-11-16T12:00:00+00:00";
        assert!(gate_is_current(None, None).is_err());
        assert!(gate_is_current(Some(&json!({})), None).is_err());
        assert!(gate_is_current(Some(&json!({"gate": ""})), None).is_err());
        // A gate with no time: from before the rule, must be re-confirmed.
        assert!(gate_is_current(Some(&json!({"gate": "cargo test"})), None).is_err());
        assert!(gate_is_current(Some(&json!({"gate": "cargo test", "gate_at": t1})), None).is_ok());
        // A step checked off after the gate: stale.
        let stale = json!({"gate": "cargo test", "gate_at": t1, "tasks": [{"title": "a", "status": "done", "done_at": t2}]});
        assert!(gate_is_current(Some(&stale), None).is_err());
        // A step checked off before it, or one not done: current.
        let fresh = json!({"gate": "cargo test", "gate_at": t2, "tasks": [
            {"title": "a", "status": "done", "done_at": t1},
            {"title": "b", "status": "doing"}
        ]});
        assert!(gate_is_current(Some(&fresh), None).is_ok());
        // A gate from before this session began does not count.
        assert!(gate_is_current(Some(&json!({"gate": "g", "gate_at": t0})), at(t1)).is_err());
        assert!(gate_is_current(Some(&json!({"gate": "g", "gate_at": t2})), at(t1)).is_ok());
    }
}
