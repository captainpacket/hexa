//! Can this project orient an agent? (ADR-2611221600 §6)
//!
//! The checks `hexa doctor` prints and `hexa finish` lists, each with the
//! command that fixes it: the hooks, AGENTS.md, CLAUDE.md's import, the code
//! graph, the shipped skills and agents, and the module READMEs. They run only
//! in an initialised project, one with `.hexa/project.json`.

use std::path::Path;

use hexa_core::ports::orientation::{
    graph_state, imports_agents_md, is_current, managed_body, CheckState, GraphState,
    OrientationCheck, AGENTS_MD, CLAUDE_MD,
};

use crate::assets::Assets;

/// True when `dir` is a project hexa has initialised.
pub fn initialised(dir: &Path) -> bool {
    dir.join(".hexa").join("project.json").is_file()
}

/// Every check, in a fixed order; empty outside an initialised project.
pub fn checks(dir: &Path) -> Vec<OrientationCheck> {
    if !initialised(dir) {
        return Vec::new();
    }
    vec![
        hooks(dir),
        agents_md(dir),
        claude_md(dir),
        graph(dir),
        shipped_files(dir),
        module_readmes(dir),
    ]
}

fn check(
    name: &'static str,
    state: CheckState,
    what: impl Into<String>,
    fix: impl Into<String>,
) -> OrientationCheck {
    OrientationCheck {
        name,
        state,
        what: what.into(),
        fix: fix.into(),
    }
}

fn ok(name: &'static str, what: impl Into<String>) -> OrientationCheck {
    check(name, CheckState::Ok, what, "")
}

fn hooks(dir: &Path) -> OrientationCheck {
    let path = dir.join(".claude").join("settings.json");
    let settings: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    let have = super::init::settings_hook_commands(&settings);
    let missing: Vec<(String, String)> = super::init::template_hook_commands()
        .into_iter()
        .filter(|w| !have.contains(w))
        .collect();
    if missing.is_empty() {
        return ok(
            "hooks",
            "every hexa hook is installed in .claude/settings.json",
        );
    }
    let mut events: Vec<&str> = missing.iter().map(|(e, _)| e.as_str()).collect();
    events.sort_unstable();
    events.dedup();
    check(
        "hooks",
        CheckState::Failing,
        format!(
            "{} hexa hook command{} missing from .claude/settings.json ({})",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" },
            events.join(", ")
        ),
        "hexa refresh",
    )
}

fn agents_md(dir: &Path) -> OrientationCheck {
    let Ok(text) = std::fs::read_to_string(dir.join("AGENTS.md")) else {
        return check(
            "AGENTS.md",
            CheckState::Failing,
            "AGENTS.md is missing: no agent gets the project's guide",
            "hexa refresh",
        );
    };
    match managed_body(&text, &AGENTS_MD) {
        Err(e) => check(
            "AGENTS.md",
            CheckState::Failing,
            format!("AGENTS.md's markers are malformed: {e}"),
            "fix the markers in AGENTS.md by hand, then hexa refresh",
        ),
        Ok(None) => check(
            "AGENTS.md",
            CheckState::Failing,
            "AGENTS.md has no hexa-managed section",
            "hexa refresh",
        ),
        Ok(Some(_)) if !is_current(&text, &AGENTS_MD, &super::refresh::agents_section(dir)) => {
            check(
                "AGENTS.md",
                CheckState::Failing,
                "AGENTS.md's managed section is out of date with this hexa and .hexa/project.json",
                "hexa refresh",
            )
        }
        Ok(Some(_)) => ok(
            "AGENTS.md",
            "AGENTS.md is present and its managed section is current",
        ),
    }
}

fn claude_md(dir: &Path) -> OrientationCheck {
    let Ok(text) = std::fs::read_to_string(dir.join("CLAUDE.md")) else {
        return check(
            "CLAUDE.md",
            CheckState::Failing,
            "CLAUDE.md is missing: Claude Code does not read AGENTS.md",
            "echo '@AGENTS.md' > CLAUDE.md",
        );
    };
    let managed = managed_body(&text, &CLAUDE_MD);
    if !imports_agents_md(&text) {
        let fix = if matches!(managed, Ok(Some(_))) {
            "hexa refresh"
        } else {
            "add the line `@AGENTS.md` to CLAUDE.md"
        };
        return check(
            "CLAUDE.md",
            CheckState::Failing,
            "CLAUDE.md does not import AGENTS.md",
            fix,
        );
    }
    match managed {
        Err(e) => check(
            "CLAUDE.md",
            CheckState::Failing,
            format!("CLAUDE.md's markers are malformed: {e}"),
            "fix the markers in CLAUDE.md by hand, then hexa refresh",
        ),
        Ok(Some(_)) if !is_current(&text, &CLAUDE_MD, &super::refresh::claude_section()) => check(
            "CLAUDE.md",
            CheckState::Failing,
            "CLAUDE.md's managed section is out of date with this hexa",
            "hexa refresh",
        ),
        _ => ok("CLAUDE.md", "CLAUDE.md imports AGENTS.md"),
    }
}

/// `hexa graph status --json` from this binary, in `dir`; `None` when it does
/// not run or exits non-zero.
fn graph_status_json(dir: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let out = std::process::Command::new(exe)
        .args(["graph", "status", "--json"])
        .current_dir(dir)
        .env("HEXA_INTERNAL", "1")
        .env_remove("HEXA_EVENTS")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn graph(dir: &Path) -> OrientationCheck {
    match graph_state(graph_status_json(dir).as_deref()) {
        GraphState::Fresh => ok("graph", "the code graph is fresh"),
        GraphState::Stale => check(
            "graph",
            CheckState::Failing,
            "the code graph is stale: graph context and consumers answer about old code",
            "hexa graph build",
        ),
        GraphState::Missing => check(
            "graph",
            CheckState::Failing,
            "the code graph is missing: graph context and consumers cannot answer",
            "hexa graph build",
        ),
        GraphState::Unknown => check(
            "graph",
            CheckState::Unknown,
            "the code graph's state is unknown: `hexa graph status --json` did not answer",
            "hexa graph build",
        ),
    }
}

fn shipped_files(dir: &Path) -> OrientationCheck {
    let claude = dir.join(".claude");
    let (mut missing, mut differ) = (Vec::new(), Vec::new());
    for path in Assets::iter() {
        let Some((prefix, sub)) = [("skills/", "skills"), ("agents/", "agents")]
            .into_iter()
            .find(|(p, _)| path.starts_with(p))
        else {
            continue;
        };
        let rel = path[prefix.len()..]
            .strip_suffix(".tmpl")
            .unwrap_or(&path[prefix.len()..])
            .to_string();
        let Some(file) = Assets::get(&path) else {
            continue;
        };
        match std::fs::read(claude.join(sub).join(&rel)) {
            Err(_) => missing.push(rel),
            Ok(now) if now != file.data.as_ref() => differ.push(rel),
            Ok(_) => {}
        }
    }
    missing.sort();
    differ.sort();
    if !missing.is_empty() {
        return check(
            "skills",
            CheckState::Failing,
            format!(
                "{} shipped skill or agent file{} missing from .claude/: {}",
                missing.len(),
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", ")
            ),
            "hexa skill sync",
        );
    }
    if differ.is_empty() {
        ok(
            "skills",
            "every shipped skill and agent is installed and current",
        )
    } else {
        ok(
            "skills",
            format!(
                "every shipped skill and agent is installed; {} differ from this hexa's and are the project's own: {} (`hexa skill sync --update` replaces them)",
                differ.len(),
                differ.join(", ")
            ),
        )
    }
}

fn module_readmes(dir: &Path) -> OrientationCheck {
    let missing = super::docs::missing_module_readmes(dir);
    if missing.is_empty() {
        return ok("module READMEs", "every module has a README.md");
    }
    check(
        "module READMEs",
        CheckState::Failing,
        format!("module README missing: {}", missing.join(", ")),
        "hexa docs readmes",
    )
}
