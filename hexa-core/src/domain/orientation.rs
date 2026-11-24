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
pub const AGENTS_MD: Markers = Markers {
    start: "<!-- hexa:agents-md:start -->",
    end: "<!-- hexa:agents-md:end -->",
};

/// CLAUDE.md's managed section (§2). The same strings `hexa refresh` has always used.
pub const CLAUDE_MD: Markers = Markers {
    start: "<!-- hexa:claude-md:start -->",
    end: "<!-- hexa:claude-md:end -->",
};

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
            MarkerError::EndBeforeStart => {
                write!(f, "the end marker comes before the start marker")
            }
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
    Ok(span(text, markers)?.map(|(s, e)| text[s + markers.start.len()..e].trim()))
}

/// Byte offsets of the start marker and of the end marker, when both are there.
fn span(text: &str, markers: &Markers) -> Result<Option<(usize, usize)>, MarkerError> {
    match (text.find(markers.start), text.find(markers.end)) {
        (None, None) => Ok(None),
        (Some(s), Some(e)) if e >= s + markers.start.len() => Ok(Some((s, e))),
        (Some(_), Some(_)) => Err(MarkerError::EndBeforeStart),
        _ => Err(MarkerError::Unpaired),
    }
}

/// `text` with its managed section replaced by `body`, every byte outside the
/// markers unchanged. `Ok(None)` when `text` has no markers.
pub fn replace_managed(
    text: &str,
    markers: &Markers,
    body: &str,
) -> Result<Option<String>, MarkerError> {
    Ok(span(text, markers)?.map(|(s, e)| {
        let after = e + markers.end.len();
        format!("{}{}{}", &text[..s], wrap(markers, body), &text[after..])
    }))
}

/// `text` with the wrapped section appended after it, `text` kept byte for byte.
pub fn append_managed(text: &str, markers: &Markers, body: &str) -> String {
    let sep = if text.is_empty() || text.ends_with("\n\n") {
        ""
    } else if text.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{text}{sep}{}\n", wrap(markers, body))
}

/// True when `text`'s managed section holds exactly `body` (both trimmed).
pub fn is_current(text: &str, markers: &Markers, body: &str) -> bool {
    matches!(managed_body(text, markers), Ok(Some(b)) if b == body.trim())
}

/// True when a line outside a code fence is exactly `@AGENTS.md` (§2).
pub fn imports_agents_md(claude_md: &str) -> bool {
    let mut fenced = false;
    for line in claude_md.lines() {
        let l = line.trim();
        if l.starts_with("```") || l.starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced && l == AGENTS_IMPORT {
            return true;
        }
    }
    false
}

/// "Where builds run", as Markdown, from `.hexa/project.json` (§1): the full
/// gate (`gate.full`), the remote runner (`test.remote`) or "this machine",
/// and the extensions of `test.runners`.
pub fn where_builds_run(project: &serde_json::Value) -> String {
    let text = |v: &serde_json::Value| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut out = Vec::new();
    match text(&project["gate"]["full"]) {
        Some(g) => out.push(format!("- **The full gate:** `{g}`. It decides `hexa loop stage done` and is the evidence `hexa do gate` is given.")),
        None => out.push(
            "- **The full gate:** not declared. Declare the command that proves the whole project as `gate.full` in `.hexa/project.json`.".to_string(),
        ),
    }
    match text(&project["test"]["remote"]) {
        Some(r) => out.push(format!(
            "- **Tests and gates run on a remote runner**, not on this machine: `{r}`. `hexa spec verify`, `hexa spec red` and \
             `hexa loop stage done` send their tests there for the commit concerned. Commit and push first: hexa refuses an \
             unpushed commit or a dirty tree. `--local` runs one of them here, on purpose. Report test counts only from the \
             remote run on the pushed head."
        )),
        None => out.push(
            "- **Tests run on this machine.** A project that builds elsewhere names its runner as `test.remote` in \
             `.hexa/project.json` (a command template with `{commit}` and `{cmd}`)."
                .to_string(),
        ),
    }
    if let Some(runners) = project["test"]["runners"]
        .as_object()
        .filter(|m| !m.is_empty())
    {
        let exts: Vec<String> = runners.keys().map(|k| format!("`.{k}`")).collect();
        out.push(format!(
            "- **Test runners configured here** (`test.runners`): {}.",
            exts.join(", ")
        ));
    }
    out.join("\n")
}

/// AGENTS.md's managed body (§1): start here, where builds run, find your
/// place, several agents, then the shipped guide.
pub fn agents_md_section(where_builds_run: &str, guide: &str) -> String {
    format!(
        "{AGENTS_PREAMBLE}\n\n### Where builds run\n\n{}\n\n{FIND_YOUR_PLACE}\n\n{SEVERAL_AGENTS}\n\n{}",
        where_builds_run.trim(),
        guide.trim()
    )
}

const AGENTS_PREAMBLE: &str = "\
This section is written by hexa and kept current by `hexa refresh`. Every agent reads it: Codex, Cursor and \
Gemini read AGENTS.md, and Claude Code imports it from CLAUDE.md. Write project notes outside the markers; \
hexa never changes them.

### Start here

1. `hexa status`: the loop (ADR, gate, stage, checklist), what is in flight, and the next step.
2. `hexa finish`: every open item, each with the command that closes it.
3. `hexa go`: the next step alone.
4. `hexa doctor`: whether this project can orient you (hooks, this file, the graph, skills, module READMEs).";

const FIND_YOUR_PLACE: &str = "\
### Find your place

- Before editing a file: `hexa graph context <path>`, what it defines, what it uses, who consumes it.
- Before deleting or moving code: `hexa graph consumers <path>` across the whole workspace.
- Both read `.hexa/graph/graph.json`. `hexa graph build` makes it; a stale graph answers about old code.
- Each module's `README.md` says what it owns, what it may import, where its tests are, and which ADRs decided it.
- Lessons: `hexa memory search <topic>` before you start, `hexa memory store lesson:<topic> \"<text>\"` when you learn one.";

const SEVERAL_AGENTS: &str = "\
### Several agents

- A lead splits the work and integrates it; builders each take files of their own, in their own worktree.
  Claude Code's definitions are `.claude/agents/hexa-lead.md` and `.claude/agents/hexa-builder.md`; the rules
  are the same for any agent.
- Split work by file. One writer per branch. Generated files change only through the tool that generates them.
- Run the formatter (`cargo fmt --all` in Rust) before every push. Wait on your own remote run; never end a turn
  while it is running. Report counts only from the remote run on the pushed head.
- Never restart a CI runner that has a job in flight.
- Integrate, never trust: the lead reruns every gate itself. The `hexa-parallel-agents` skill has the whole procedure.";

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
        let parts: Vec<&str> = rel
            .trim_end_matches('/')
            .split(['/', '\\'])
            .filter(|p| !p.is_empty())
            .collect();
        match parts.as_slice() {
            [.., "adapters", "primary"] => Some(Layer::PrimaryAdapters),
            [.., "adapters", "secondary"] => Some(Layer::SecondaryAdapters),
            [.., "adapters"] => Some(Layer::Adapters),
            [.., "domain"] => Some(Layer::Domain),
            [.., "ports"] => Some(Layer::Ports),
            [.., "usecases"] => Some(Layer::Usecases),
            _ => None,
        }
    }

    /// What this layer may import (the enforced rule).
    pub fn may_import(&self) -> &'static str {
        match self {
            Layer::Domain => "domain/ only: no other layer and no I/O",
            Layer::Ports => "domain/ only, for value types",
            Layer::Usecases => "domain/ and ports/ only",
            Layer::PrimaryAdapters => "ports/ and usecases/ only: never domain/ directly, never another adapter",
            Layer::SecondaryAdapters => "ports/ only: never usecases/, never another adapter",
            Layer::Adapters => {
                "ports/ (a driving adapter also usecases/): never domain/ directly, never another adapter"
            }
        }
    }

    /// What a module of this layer owns, in one line.
    fn owns(&self) -> &'static str {
        match self {
            Layer::Domain => "The business rules and value types: pure code, no I/O.",
            Layer::Ports => "The typed interfaces between layers: the contracts adapters implement and use cases call.",
            Layer::Usecases => "The application logic, composed from ports.",
            Layer::PrimaryAdapters => "The driving adapters: the ways in (CLI, HTTP, UI) that call use cases.",
            Layer::SecondaryAdapters => "The driven adapters: implementations of ports (storage, network, files).",
            Layer::Adapters => "The adapters: driving ones that call use cases, driven ones that implement ports.",
        }
    }
}

/// A module README's skeleton (§5): `## Owns`, `## May import`, `## Tests`,
/// `## ADRs`. `layer` fills "may import"; `None` is a module that is not one
/// layer (a crate), whose rule is `.hexa/project.json → analyze.layers`.
pub fn module_readme(name: &str, layer: Option<Layer>, gate: &str) -> String {
    let owns = layer
        .map(|l| l.owns())
        .unwrap_or("Say in a sentence what this module is responsible for, and what it is not.");
    let may = match layer {
        Some(l) => format!("{}. `hexa analyze .` enforces it.", l.may_import()),
        None => "The layer of each file is set by `.hexa/project.json` (`analyze.layers`) or its folder; \
                 `hexa analyze .` enforces the rules. List here the modules this one may use."
            .to_string(),
    };
    let gate = if gate.trim().is_empty() {
        "the project's full gate (`gate.full` in `.hexa/project.json`)".to_string()
    } else {
        format!("`{}`", gate.trim())
    };
    format!(
        "# {name}\n\n\
         ## Owns\n\n{owns}\n\n\
         ## May import\n\n{may}\n\n\
         ## Tests\n\nRun with {gate}. Before changing a file here, `hexa graph context <path>`; before deleting one, \
         `hexa graph consumers <path>`.\n\n\
         ## ADRs\n\nThe decisions that shaped this module, by id (`hexa adr search {search}` finds them).\n"
        ,
        search = name.rsplit('/').next().unwrap_or(name)
    )
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
    let Some(v) =
        status_json.and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim()).ok())
    else {
        return GraphState::Unknown;
    };
    match v.get("state").and_then(|s| s.as_str()) {
        Some("fresh") => GraphState::Fresh,
        Some("stale") => GraphState::Stale,
        Some("missing") => GraphState::Missing,
        _ => GraphState::Unknown,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_builds_run_names_the_remote_runner_and_says_to_push_first() {
        let v = serde_json::json!({
            "gate": {"full": "cargo test --workspace"},
            "test": {"remote": "bash ci/remote.sh --commit {commit} --extra {cmd}", "runners": {"py": "pytest {file}"}}
        });
        let w = where_builds_run(&v);
        assert!(w.contains("`cargo test --workspace`"), "{w}");
        assert!(
            w.contains("remote runner") && w.contains("bash ci/remote.sh"),
            "{w}"
        );
        assert!(
            w.contains("push"),
            "a remote runner sees only pushed commits: {w}"
        );
        assert!(w.contains("`.py`"), "{w}");
    }

    #[test]
    fn where_builds_run_without_config_says_this_machine_and_how_to_name_a_runner() {
        let w = where_builds_run(&serde_json::Value::Null);
        assert!(w.contains("this machine"), "{w}");
        assert!(w.contains("test.remote") && w.contains("gate.full"), "{w}");
        let blank =
            where_builds_run(&serde_json::json!({"gate": {"full": "  "}, "test": {"remote": ""}}));
        assert_eq!(blank, w, "a blank value is no value");
    }

    #[test]
    fn the_agents_section_runs_start_to_guide_in_order() {
        let s = agents_md_section("- BUILDS", "## GUIDE\n\nbody");
        let at = |n: &str| s.find(n).unwrap_or_else(|| panic!("{n} missing:\n{s}"));
        assert!(at("### Start here") < at("### Where builds run"));
        assert!(at("- BUILDS") < at("### Find your place"));
        assert!(at("### Find your place") < at("### Several agents"));
        assert!(at("### Several agents") < at("## GUIDE"));
        assert!(
            !s.contains(AGENTS_MD.start),
            "the body carries no markers; wrap adds them"
        );
    }

    #[test]
    fn append_managed_keeps_the_text_and_separates_it_by_one_blank_line() {
        for text in ["mine", "mine\n", "mine\n\n"] {
            let out = append_managed(text, &AGENTS_MD, "b");
            assert!(out.starts_with(text));
            assert!(
                out.contains(&format!("mine\n\n{}", AGENTS_MD.start)),
                "{out:?}"
            );
            assert!(out.ends_with(&format!("{}\n", AGENTS_MD.end)));
        }
        assert_eq!(
            append_managed("", &AGENTS_MD, "b"),
            format!("{}\nb\n{}\n", AGENTS_MD.start, AGENTS_MD.end)
        );
    }

    #[test]
    fn a_body_that_is_already_current_is_reported_so() {
        let text = format!("x\n{}\n", wrap(&CLAUDE_MD, "  the body \n"));
        assert!(is_current(&text, &CLAUDE_MD, "the body"));
        assert!(!is_current(&text, &CLAUDE_MD, "another"));
        assert!(!is_current("no markers", &CLAUDE_MD, "the body"));
    }
}
