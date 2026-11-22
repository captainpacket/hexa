//! Agent orientation: what a hexa project gives every agent that works in it
//! (ADR-2611221600).
//!
//! Pure rules, no I/O:
//! - the hexa-managed sections of AGENTS.md and CLAUDE.md, and how one is
//!   replaced in place without touching hand-written text (§1–§3);
//! - "where builds run", read from `.hexa/project.json` (§1);
//! - a module README's skeleton (§5);
//! - reading `hexa graph status --json` (§6).

/// The pair of HTML comments that bound a hexa-managed section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Markers {
    pub start: &'static str,
    pub end: &'static str,
}

/// AGENTS.md's managed section (§1).
pub const AGENTS_MD: Markers = Markers { start: "<!-- hexa:agents-md:start -->", end: "<!-- hexa:agents-md:end -->" };

/// CLAUDE.md's managed section (§2). The same strings `hexa refresh` has always used.
pub const CLAUDE_MD: Markers = Markers { start: "<!-- hexa:claude-md:start -->", end: "<!-- hexa:claude-md:end -->" };

/// The line by which CLAUDE.md imports AGENTS.md (Claude Code's `@path` import).
pub const AGENTS_IMPORT: &str = "@AGENTS.md";

/// Why a managed section could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerError {
    /// The end marker comes before the start marker.
    EndBeforeStart,
    /// Only one of the two markers is present.
    Unpaired,
}

impl std::fmt::Display for MarkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkerError::EndBeforeStart => write!(f, "the end marker comes before the start marker"),
            MarkerError::Unpaired => write!(f, "only one of the two markers is present"),
        }
    }
}

/// `body` wrapped in its markers, each on its own line.
pub fn wrap(markers: &Markers, body: &str) -> String {
    format!("{}\n{}\n{}", markers.start, body.trim(), markers.end)
}

/// The text between the markers, trimmed; `Ok(None)` when neither is present.
pub fn managed_body<'a>(text: &'a str, markers: &Markers) -> Result<Option<&'a str>, MarkerError> {
    let _ = (text, markers);
    unimplemented!("ADR-2611221600 §3")
}

/// `text` with its managed section replaced by `body`, every byte outside the
/// markers unchanged. `Ok(None)` when `text` has no markers.
pub fn replace_managed(text: &str, markers: &Markers, body: &str) -> Result<Option<String>, MarkerError> {
    let _ = (text, markers, body);
    unimplemented!("ADR-2611221600 §3")
}

/// `text` with the wrapped section appended after it, `text` kept byte for byte.
pub fn append_managed(text: &str, markers: &Markers, body: &str) -> String {
    let _ = (text, markers, body);
    unimplemented!("ADR-2611221600 §1")
}

/// True when `text`'s managed section holds exactly `body` (both trimmed).
pub fn is_current(text: &str, markers: &Markers, body: &str) -> bool {
    matches!(managed_body(text, markers), Ok(Some(b)) if b == body.trim())
}

/// True when a line outside a code fence is exactly `@AGENTS.md` (§2).
pub fn imports_agents_md(claude_md: &str) -> bool {
    let _ = claude_md;
    unimplemented!("ADR-2611221600 §2")
}

/// "Where builds run", as Markdown, from `.hexa/project.json` (§1): the full
/// gate (`gate.full`), the remote runner (`test.remote`) or "this machine",
/// and the extensions of `test.runners`.
pub fn where_builds_run(project: &serde_json::Value) -> String {
    let _ = project;
    unimplemented!("ADR-2611221600 §1")
}

/// AGENTS.md's managed body (§1): start here, where builds run, find your
/// place, several agents, then the shipped guide.
pub fn agents_md_section(where_builds_run: &str, guide: &str) -> String {
    let _ = (where_builds_run, guide);
    unimplemented!("ADR-2611221600 §1")
}

/// A hexagonal layer, as the folder convention names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Domain,
    Ports,
    Usecases,
    PrimaryAdapters,
    SecondaryAdapters,
    /// `adapters/` itself, when it holds both kinds.
    Adapters,
}

impl Layer {
    /// The layer a directory is, from its trailing path components
    /// (`src/domain`, `internal/ports`, `src/adapters/secondary` …).
    pub fn of_dir(rel: &str) -> Option<Layer> {
        let _ = rel;
        unimplemented!("ADR-2611221600 §5")
    }

    /// What this layer may import (the enforced rule).
    pub fn may_import(&self) -> &'static str {
        unimplemented!("ADR-2611221600 §5")
    }
}

/// A module README's skeleton (§5): `## Owns`, `## May import`, `## Tests`,
/// `## ADRs`. `layer` fills "may import"; `None` is a module that is not one
/// layer (a crate), whose rule is `.hexa/project.json → analyze.layers`.
pub fn module_readme(name: &str, layer: Option<Layer>, gate: &str) -> String {
    let _ = (name, layer, gate);
    unimplemented!("ADR-2611221600 §5")
}

/// The graph's state, as `hexa graph status --json` reports it (§6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphState {
    Fresh,
    Stale,
    Missing,
    /// The verb is missing, failed, or printed something else.
    Unknown,
}

/// Read `hexa graph status --json`'s output; `None` (no output) is `Unknown`.
pub fn graph_state(status_json: Option<&str>) -> GraphState {
    let _ = status_json;
    unimplemented!("ADR-2611221600 §6")
}

/// How an orientation check came out (§6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Ok,
    Failing,
    /// Could not be decided: listed, never a pass, never a failure.
    Unknown,
}

/// One orientation check: its name, how it came out, what was found, and the
/// command that fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrientationCheck {
    pub name: &'static str,
    pub state: CheckState,
    pub what: String,
    pub fix: String,
}
