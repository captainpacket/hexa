//! `hexa refresh` — update the hexa-managed sections of AGENTS.md and CLAUDE.md
//! in place (ADR-2611221600 §3).
//!
//! Unlike `hexa init`, refresh does NOT re-run the interview, regenerate
//! `.hexa/`, or touch any project structure. It rewrites only the bytes
//! between each file's markers, so operators get the guide shipped in
//! `hexa-cli/assets/templates/` without losing a word they wrote:
//!
//! ```text
//! <!-- hexa:agents-md:start -->   AGENTS.md: the guide every agent reads
//! <!-- hexa:agents-md:end -->
//! <!-- hexa:claude-md:start -->   CLAUDE.md: `@AGENTS.md` and the Claude-only parts
//! <!-- hexa:claude-md:end -->
//! ```
//!
//! AGENTS.md is created when missing, and an AGENTS.md with no markers gets
//! the section appended after its own text. CLAUDE.md is never appended to:
//! one written by an older `hexa init` lacks the markers, and on first
//! refresh its hexa section is found by its canonical opening heading and the
//! file upgraded to marker-wrapped form. A file whose end marker precedes its
//! start is refused and left as it is.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use hexa_core::ports::orientation::{self as orient, AGENTS_MD, CLAUDE_MD};

pub const START_MARKER: &str = CLAUDE_MD.start;
pub const END_MARKER: &str = CLAUDE_MD.end;
const LEGACY_HEADING: &str = "## hexa Autonomous Behavior";

/// CLAUDE.md's managed section, wrapped in the markers that make a later
/// `hexa refresh` a pure replacement: the `@AGENTS.md` import and the
/// Claude-only parts (ADR-2611221600 §2).
///
/// `hexa init` uses this too. It used to write the section bare, which meant a
/// freshly initialised project had neither a marker nor the legacy heading, so
/// `hexa refresh` declined to touch it — the projects most likely to want a
/// newer rule set were the only ones that could never receive one.
pub fn wrapped_hex_section() -> String {
    orient::wrap(&CLAUDE_MD, &claude_section())
}

/// CLAUDE.md's managed body.
pub fn claude_section() -> String {
    crate::assets::Assets::get_str("templates/claude-md-claude-section.md")
        .expect("claude-md-claude-section.md must be embedded in assets/templates/")
}

/// AGENTS.md's managed body for the project at `target` (ADR-2611221600 §1):
/// "where builds run" is read from its `.hexa/project.json`.
pub fn agents_section(target: &Path) -> String {
    let project = fs::read_to_string(target.join(".hexa/project.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    orient::agents_md_section(
        &orient::where_builds_run(&project),
        &hexa_section_template(),
    )
}

/// What writing AGENTS.md did.
#[derive(Debug, PartialEq, Eq)]
pub enum AgentsMdOutcome {
    Created,
    Replaced,
    Appended,
    Current,
}

/// The AGENTS.md text for `existing` (None: the file is missing), and what
/// writing it does. Text outside the markers is kept byte for byte.
fn agents_md_text(
    existing: Option<&str>,
    project_name: &str,
    body: &str,
) -> Result<(String, AgentsMdOutcome)> {
    let Some(text) = existing else {
        return Ok((
            fresh_agents_md(project_name, body),
            AgentsMdOutcome::Created,
        ));
    };
    match orient::replace_managed(text, &AGENTS_MD, body) {
        Ok(Some(new)) if new == text => Ok((new, AgentsMdOutcome::Current)),
        Ok(Some(new)) => Ok((new, AgentsMdOutcome::Replaced)),
        Ok(None) => Ok((
            orient::append_managed(text, &AGENTS_MD, body),
            AgentsMdOutcome::Appended,
        )),
        Err(e) => anyhow::bail!(
            "AGENTS.md: {e}; fix the markers by hand, hexa will not guess where the section ends"
        ),
    }
}

/// A new project's AGENTS.md: the managed section, then the project's own notes.
fn fresh_agents_md(project_name: &str, body: &str) -> String {
    format!(
        "# {project_name}\n\n{}\n\n\
         ## Project notes\n\n\
         Hand-written: hexa never changes anything outside the markers above.\n\n\
         - ALWAYS read a file before editing it.\n\
         - NEVER commit secrets, credentials, or `.env` files; use `.env.example`.\n\
         - ALWAYS run the tests after changing code.\n\
         - Primary adapters MUST NOT use `innerHTML`/`outerHTML`/`insertAdjacentHTML` with any data that originates \
         outside the domain layer. Use `textContent` or DOM APIs (`createElement`) instead.\n",
        orient::wrap(&AGENTS_MD, body)
    )
}

/// Write AGENTS.md at `target`: create it, replace its managed section, or
/// append one. `hexa init` and `hexa refresh` both call this.
pub fn write_agents_md(target: &Path, project_name: &str) -> Result<AgentsMdOutcome> {
    let path = target.join("AGENTS.md");
    let existing = fs::read_to_string(&path).ok();
    let (text, how) = agents_md_text(existing.as_deref(), project_name, &agents_section(target))?;
    if how != AgentsMdOutcome::Current {
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(how)
}

fn project_name(target: &Path) -> String {
    fs::read_to_string(target.join(".hexa/project.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["name"].as_str().map(str::to_string))
        .or_else(|| target.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "project".to_string())
}

#[derive(Args, Debug)]
pub struct RefreshArgs {
    /// Target directory (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: String,

    /// Print the would-be file content without writing
    #[arg(long)]
    pub dry_run: bool,
}

/// A labelled install step: what it writes, and the call that writes it.
type InstallStep<'a> = (&'a str, Box<dyn Fn() -> Result<()> + 'a>);

pub async fn run(args: RefreshArgs) -> Result<()> {
    let target = PathBuf::from(&args.path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&args.path));

    // Refresh the hexa-managed config files. This is the install-integrity
    // sweep — every asset hexa init writes, refresh re-syncs (idempotent
    // merges, never destructive).
    //
    // The .mcp.json and hexa-statusline.cjs entries went with the MCP server
    // and the daemon statusline they configured (ADR-2608241500). Re-syncing a
    // config that points at a deleted verb is worse than not syncing at all.
    if !args.dry_run {
        let installs: &[InstallStep] = &[(
            ".claude/settings.json (hooks + permissions)",
            Box::new(|| super::init::create_claude_settings(&target)),
        )];
        for (label, run) in installs {
            match run() {
                Ok(()) => println!("{} {}", "\u{2713}".green(), label),
                Err(e) => eprintln!("  {} {}: {}", "\u{2717}".red(), label, e),
            }
        }
    }

    let mut errors: Vec<String> = Vec::new();

    // AGENTS.md: the guide every agent reads (ADR-2611221600 §1, §3).
    let agents_md = target.join("AGENTS.md");
    let existing = fs::read_to_string(&agents_md).ok();
    match agents_md_text(
        existing.as_deref(),
        &project_name(&target),
        &agents_section(&target),
    ) {
        Err(e) => {
            eprintln!("{} {}: {e}", "\u{2717}".red(), agents_md.display());
            errors.push(format!("{}: {e}", agents_md.display()));
        }
        Ok((text, how)) if args.dry_run => {
            println!("{} {} ({how:?})", "[dry-run]".yellow(), agents_md.display());
            println!("{text}");
        }
        Ok((_, AgentsMdOutcome::Current)) => println!(
            "{} {} already up-to-date",
            "\u{2713}".green(),
            agents_md.display()
        ),
        Ok((text, how)) => {
            fs::write(&agents_md, text)
                .with_context(|| format!("writing {}", agents_md.display()))?;
            let what = match how {
                AgentsMdOutcome::Created => "created",
                AgentsMdOutcome::Appended => "managed section appended after your text",
                _ => "replaced marker-wrapped section",
            };
            println!("{} {} — {what}", "\u{2713}".green(), agents_md.display());
        }
    }

    if let Err(e) = refresh_claude_md(&target, args.dry_run) {
        eprintln!("{} {e}", "\u{2717}".red());
        errors.push(e.to_string());
    }

    if !errors.is_empty() {
        anyhow::bail!(
            "refresh left {} file(s) as they were: {}",
            errors.len(),
            errors.join("; ")
        );
    }
    Ok(())
}

/// CLAUDE.md's managed section: replaced between its markers, or a legacy
/// hexa section upgraded to markers. Never appended to.
fn refresh_claude_md(target: &Path, dry_run: bool) -> Result<()> {
    let claude_md = target.join("CLAUDE.md");

    if !claude_md.exists() {
        anyhow::bail!(
            "No CLAUDE.md at {} — run `hexa init` first.",
            claude_md.display()
        );
    }

    let existing = fs::read_to_string(&claude_md)
        .with_context(|| format!("reading {}", claude_md.display()))?;
    let body = claude_section();
    let wrapped = wrapped_hex_section();

    let replaced = orient::replace_managed(&existing, &CLAUDE_MD, &body)
        .map_err(|e| anyhow::anyhow!("{}: {e}; fix the markers by hand", claude_md.display()))?;
    let (updated, how) = if let Some(out) = replaced {
        (out, "replaced marker-wrapped section")
    } else if let Some(start) = existing.find(LEGACY_HEADING) {
        // Legacy file: find end of the hexa-managed block. The shipped section
        // ends with a "## File Organization" block whose fenced code block
        // closes the section. We look for the next top-level heading after
        // the legacy start that is NOT a hexa-managed heading, OR we take EOF.
        let after_start = &existing[start..];
        let end_offset = find_legacy_section_end(after_start);
        let absolute_end = start + end_offset;
        let mut out = String::with_capacity(existing.len() + wrapped.len());
        out.push_str(existing[..start].trim_end());
        out.push_str("\n\n");
        out.push_str(&wrapped);
        if absolute_end < existing.len() {
            out.push_str("\n\n");
            out.push_str(existing[absolute_end..].trim_start());
        }
        out.push('\n');
        (out, "upgraded legacy section (markers inserted)")
    } else {
        // Neither marker nor legacy heading found — this project isn't a
        // shipped-template consumer (or hand-rolled its own hexa section).
        // Refuse to append; appending would stomp user-maintained content.
        println!(
            "{} {} has no hexa-managed section — skipping CLAUDE.md (add the line `@AGENTS.md` to it, or use `hexa init` for fresh installs)",
            "\u{25CB}".yellow(),
            claude_md.display()
        );
        return Ok(());
    };

    if dry_run {
        println!("{} {} ({})", "[dry-run]".yellow(), claude_md.display(), how);
        println!("{}", updated);
        return Ok(());
    }

    if updated == existing {
        println!(
            "{} {} already up-to-date",
            "\u{2713}".green(),
            claude_md.display()
        );
        return Ok(());
    }

    fs::write(&claude_md, updated).with_context(|| format!("writing {}", claude_md.display()))?;
    println!("{} {} — {}", "\u{2713}".green(), claude_md.display(), how);
    Ok(())
}

/// The shipped cross-agent guide. Its file keeps the name sealed tests read
/// (ADR-2611221600 §1); it is the tail of AGENTS.md's managed section.
pub(crate) fn hexa_section_template() -> String {
    crate::assets::Assets::get_str("templates/claude-md-hexa-section.md")
        .expect("claude-md-hexa-section.md must be embedded in assets/templates/")
}

/// Given the slice starting at `## hexa Autonomous Behavior`, return the byte
/// offset where the hexa-managed section ends. We consider the section to
/// include the known shipped headings; the first `## ` that isn't one of
/// those terminates it. If none found, returns `slice.len()` (EOF).
fn find_legacy_section_end(slice: &str) -> usize {
    const HEXA_HEADINGS: &[&str] = &[
        // Pre-collapse headings. Kept so a CLAUDE.md written by an older hexa
        // is still recognised as one span and replaced whole, rather than
        // leaving half a daemon-era rule set stranded below the new section.
        "## hexa Autonomous Behavior",
        "## hexa Tool Precedence",
        // Current headings.
        "## hexa — how to work in this project",
        "## Development pipeline",
        "## Hexagonal Architecture Rules",
        "## File Organization",
        "## Lessons that a rule cannot catch",
        "## hexa — what only Claude Code uses",
    ];
    let mut cursor = 0;
    // Skip past the opening heading line itself so we don't match it.
    if let Some(nl) = slice.find('\n') {
        cursor = nl + 1;
    }
    while cursor < slice.len() {
        let rest = &slice[cursor..];
        let Some(rel) = rest.find("\n## ") else {
            return slice.len();
        };
        let heading_start = cursor + rel + 1; // position of '#'
        let heading_line_end = slice[heading_start..]
            .find('\n')
            .map(|n| heading_start + n)
            .unwrap_or(slice.len());
        let heading = &slice[heading_start..heading_line_end];
        let is_hex_managed = HEXA_HEADINGS.iter().any(|h| heading.starts_with(h));
        if !is_hex_managed {
            return heading_start;
        }
        cursor = heading_line_end + 1;
    }
    slice.len()
}
