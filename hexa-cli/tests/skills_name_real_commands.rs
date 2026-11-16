//! Every `hexa …` command a shipped skill or the managed CLAUDE.md section
//! tells an agent to run must exist — verb, subcommand and `--flag` — and must
//! be one `hexa --help` shows (ADR-2611160730 §7).
//!
//! `shipped_docs_name_real_verbs.rs` already checks verb chains in inline code
//! spans. The audit behind ADR-2611160730 found what it could not see:
//!
//! - `npx hexa summarize` sat in a fenced block, and began with `npx`, so the
//!   older extractor never read it. `hexa summarize` does not exist and hexa is
//!   not an npm package.
//! - `--evidence "hexa analyze . --exit-code"` is a command nested in a quoted
//!   argument. It ran without `--uncounted` and failed every time; a flag check
//!   on `do run` catches the missing flag.
//! - `hexa analyze` resolves, but `--help` hides it, while every document
//!   promotes it. A hidden verb is reported here as if it were missing.
//! - Reference files beside a skill (`<skill>/*.md`) were never walked.
//!
//! The oracle is the binary itself: each level's `--help` lists its
//! subcommands and its options, so there is no list to keep in sync.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

// ── extraction ──────────────────────────────────────────────────────────────

/// One `hexa …` invocation found in a document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Invocation {
    /// Tokens after `hexa`, quotes removed, up to a shell separator or comment.
    args: Vec<String>,
    /// It was written `npx hexa …`.
    via_npx: bool,
    /// The text as written, for the failure message.
    text: String,
}

/// Split a command line the way a shell would, enough for documentation:
/// `"…"` and `'…'` group, and a quoted token remembers that it was quoted so a
/// nested `hexa …` inside it can be checked on its own.
fn shell_words(line: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_token = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                in_token = true;
                quoted = true;
                for d in chars.by_ref() {
                    if d == c {
                        break;
                    }
                    cur.push(d);
                }
            }
            c if c.is_whitespace() => {
                if in_token {
                    out.push((std::mem::take(&mut cur), quoted));
                    quoted = false;
                    in_token = false;
                }
            }
            c => {
                in_token = true;
                cur.push(c);
            }
        }
    }
    if in_token {
        out.push((cur, quoted));
    }
    out
}

/// The invocations on one line of command text (a code span, or a line of a
/// fenced block). A line counts only when it *starts* with `hexa`, `npx hexa`
/// or `$ hexa`, which keeps prose ("hexa is one binary") out. Quoted arguments
/// that are themselves `hexa …` commands are returned too.
fn invocations_in(line: &str, out: &mut Vec<Invocation>) {
    let words = shell_words(line.trim());
    let mut i = 0;
    if words.first().is_some_and(|(w, q)| w == "$" && !q) {
        i = 1;
    }
    let via_npx = words.get(i).is_some_and(|(w, q)| w == "npx" && !q);
    if via_npx {
        i += 1;
    }
    if words.get(i).map(|(w, q)| (w.as_str(), *q)) != Some(("hexa", false)) {
        return;
    }
    let mut args = Vec::new();
    for (w, quoted) in &words[i + 1..] {
        if !quoted && (matches!(w.as_str(), "|" | "||" | "&&" | ";" | ">" | ">>") || w.starts_with('#')) {
            break;
        }
        if *quoted && w.trim_start().starts_with("hexa ") {
            invocations_in(w, out);
        }
        args.push(w.clone());
    }
    out.push(Invocation { args, via_npx, text: line.trim().to_string() });
}

/// Every invocation in a Markdown document: inline code spans, and each line
/// of every fenced block.
fn extract(markdown: &str) -> Vec<Invocation> {
    let mut out = Vec::new();
    let mut prose = String::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            invocations_in(line, &mut out);
        } else {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    // Inline spans in the prose. A table cell may escape a pipe as `\|`.
    let mut rest = prose.as_str();
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        invocations_in(&after[..close].replace("\\|", "|"), &mut out);
        rest = &after[close + 1..];
    }
    out
}

// ── the oracle ──────────────────────────────────────────────────────────────

/// What `hexa <chain> --help` says: whether it resolved, the subcommands it
/// lists, and every `--flag` it names.
#[derive(Clone, Default)]
struct Help {
    ok: bool,
    subcommands: BTreeSet<String>,
    flags: BTreeSet<String>,
    /// Flags written `--name <VALUE>`: the next word is theirs.
    value_flags: BTreeSet<String>,
    /// Flags written `--name <VALUE>...`: every word up to the next flag is theirs.
    multi_flags: BTreeSet<String>,
    /// `-e, --evidence <EVIDENCE>`: the short spelling of a long flag.
    shorts: BTreeMap<String, String>,
    /// How many positional arguments `Usage:` allows; `None` is unbounded.
    max_positional: Option<usize>,
}

fn parse_help(text: &str) -> Help {
    let mut h = Help { ok: true, max_positional: Some(0), ..Default::default() };
    let mut in_commands = false;
    let mut command_slot = false;
    for line in text.lines() {
        if let Some(usage) = line.strip_prefix("Usage: ") {
            // `hexa do run [OPTIONS] --file <FILE> <INSTRUCTION>`: a placeholder
            // that does not follow a flag is a positional.
            let words: Vec<&str> = usage.split_whitespace().collect();
            let mut n = 0usize;
            for (i, w) in words.iter().enumerate() {
                // `<COMMAND>` is the subcommand slot, unless this level has no
                // subcommands (`hexa loop gate <COMMAND>`); settled below.
                if matches!(*w, "[COMMAND]" | "<COMMAND>") {
                    command_slot = true;
                    continue;
                }
                let placeholder = (w.starts_with('<') || w.starts_with('[')) && *w != "[OPTIONS]";
                let after_flag = i > 0 && words[i - 1].starts_with('-');
                if placeholder && !after_flag {
                    if w.ends_with("...") {
                        h.max_positional = None;
                    }
                    n += 1;
                }
            }
            if h.max_positional.is_some() {
                h.max_positional = Some(n);
            }
        }
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if !line.starts_with(' ') && !line.is_empty() {
            in_commands = false;
        }
        if in_commands {
            // "  name   description" — two-space indent, the name first.
            if let Some(name) = line.strip_prefix("  ").and_then(|l| l.split_whitespace().next()) {
                if !line.starts_with("   ") {
                    h.subcommands.insert(name.trim_end_matches(',').to_string());
                }
            }
        }
        for tok in line.split(|c: char| c.is_whitespace() || c == ',' || c == '[' || c == ']' || c == '=') {
            if let Some(name) = tok.strip_prefix("--") {
                let name: String = name.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
                if !name.is_empty() {
                    let takes_value = line.contains(&format!("--{name} <"));
                    if takes_value {
                        h.value_flags.insert(name.clone());
                        let after = line.split(&format!("--{name} <")).nth(1).unwrap_or("");
                        if after.split_whitespace().next().is_some_and(|v| v.ends_with(">...")) {
                            h.multi_flags.insert(name.clone());
                        }
                    }
                    // "  -e, --evidence <EVIDENCE>": `-e` is `--evidence`.
                    if let Some(short) = line.trim_start().strip_prefix('-').and_then(|l| l.split(',').next()) {
                        if short.len() == 1 && line.trim_start().contains(&format!(", --{name}")) {
                            h.shorts.insert(short.to_string(), name.clone());
                        }
                    }
                    h.flags.insert(name);
                }
            }
        }
    }
    if command_slot && h.subcommands.is_empty() {
        h.max_positional = h.max_positional.map(|n| n + 1);
    }
    h
}

fn help(chain: &[String]) -> Help {
    static CACHE: Mutex<BTreeMap<Vec<String>, Help>> = Mutex::new(BTreeMap::new());
    if let Some(h) = CACHE.lock().unwrap().get(chain) {
        return h.clone();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(chain)
        .arg("--help")
        .env("NO_COLOR", "1")
        .output()
        .expect("run hexa --help");
    let h = if out.status.success() {
        parse_help(&String::from_utf8_lossy(&out.stdout))
    } else {
        Help::default()
    };
    CACHE.lock().unwrap().insert(chain.to_vec(), h.clone());
    h
}

/// Why an invocation is wrong, or `None` when hexa would accept it.
fn defect(inv: &Invocation) -> Option<String> {
    if inv.via_npx {
        return Some("`npx hexa`: hexa is one binary on PATH, not an npm package".into());
    }
    // Resolve the verb chain greedily. While the current level lists
    // subcommands, a bare word must be one of them.
    let mut chain: Vec<String> = Vec::new();
    let mut level = help(&chain);
    let mut rest = inv.args.iter().peekable();
    while let Some(tok) = rest.peek() {
        let bare = !tok.is_empty() && tok.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !tok.starts_with('-');
        if !bare || level.subcommands.is_empty() {
            break;
        }
        let mut next = chain.clone();
        next.push((*tok).clone());
        let h = help(&next);
        if !h.ok {
            return Some(format!("`hexa {}` does not exist", next.join(" ")));
        }
        if !level.subcommands.contains(*tok) {
            return Some(format!("`hexa {}` is hidden from `--help`", next.join(" ")));
        }
        chain = next;
        level = h;
        rest.next();
    }
    // Every long flag must be one this level names, and the words left over
    // must fit the positionals `Usage:` allows.
    let rest: Vec<&String> = rest.collect();
    let mut positional = 0usize;
    let mut evidence: Option<&str> = None;
    let mut uncounted = false;
    let mut i = 0;
    while i < rest.len() {
        let raw = rest[i].as_str();
        i += 1;
        // A short flag the help spells out (`-e`) is read as its long name.
        let tok = raw;
        let short = raw.strip_prefix('-').filter(|t| !t.starts_with('-')).and_then(|t| level.shorts.get(t)).map(String::as_str);
        let Some(flag) = short.or_else(|| tok.strip_prefix("--")) else {
            if !tok.starts_with('-') && !tok.is_empty() {
                // `hexa loop gate <COMMAND>`: the gate is the first positional.
                if positional == 0 && chain == ["loop", "gate"] {
                    evidence = Some(tok);
                }
                positional += 1;
            }
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (flag, None),
        };
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            continue;
        }
        if !level.flags.contains(name) {
            return Some(format!("`hexa {}` takes no `--{name}`", chain.join(" ")));
        }
        let value = if level.multi_flags.contains(name) && inline.is_none() {
            let first = rest.get(i).map(|s| s.as_str());
            while i < rest.len() && !rest[i].starts_with('-') {
                i += 1;
            }
            first
        } else if level.value_flags.contains(name) && inline.is_none() {
            i += 1;
            rest.get(i - 1).map(|s| s.as_str())
        } else {
            inline
        };
        match name {
            "evidence" => evidence = value,
            "uncounted" => uncounted = true,
            _ => {}
        }
    }
    if let Some(max) = level.max_positional {
        if positional > max {
            return Some(format!(
                "`hexa {}` takes {max} positional argument(s), the document gives {positional}",
                chain.join(" ")
            ));
        }
    }
    // A `do` evidence or `loop gate` command that no known runner writes passes
    // only when it is declared `--uncounted` (ADR-2611081151 §6). Without the flag the
    // run fails every time, which is what the analyze-arch recipe did.
    if let Some(cmd) = evidence {
        if !uncounted && !cmd.trim_start().starts_with('<') && !counted(cmd) {
            return Some(format!(
                "`--evidence \"{cmd}\"` is not a test runner ({}); the run needs `--uncounted`",
                hexa_core::domain::evidence::KNOWN_RUNNERS
            ));
        }
    }
    None
}

/// Whether a command is one the evidence reader counts tests from, by the
/// reader's own list of runners.
fn counted(cmd: &str) -> bool {
    let cmd = cmd.trim_start();
    hexa_core::domain::evidence::KNOWN_RUNNERS
        .split(", ")
        .map(|r| r.split(" (").next().unwrap_or(r).trim())
        .any(|runner| cmd == runner || cmd.starts_with(&format!("{runner} ")))
}

// ── the documents ───────────────────────────────────────────────────────────

/// Every Markdown file under `assets/skills/`, at any depth, and the managed
/// CLAUDE.md section, as (name, body).
fn shipped_guidance() -> Vec<(String, String)> {
    let base = root().join("hexa-cli/assets");
    let mut out = Vec::new();
    let mut stack = vec![base.join("skills")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("read skills dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                let body = std::fs::read_to_string(&p).expect("read skill");
                out.push((p.strip_prefix(&base).unwrap().display().to_string(), body));
            }
        }
    }
    let t = base.join("templates/claude-md-hexa-section.md");
    out.push(("templates/claude-md-hexa-section.md".into(), std::fs::read_to_string(t).expect("read template")));
    out.sort();
    out
}

// ── tests ───────────────────────────────────────────────────────────────────

/// The case that started it, written first: a fenced `npx hexa summarize`.
#[test]
fn a_fenced_npx_hexa_summarize_is_caught() {
    let md = "Run it:\n\n```bash\nnpx hexa summarize <file> --level L2\n```\n";
    let found = extract(md);
    assert_eq!(found.len(), 1, "the extractor missed the fenced line: {found:?}");
    assert!(defect(&found[0]).is_some(), "npx hexa summarize passed the check");

    // Without `npx`, the verb itself is still dead.
    let bare = extract("`hexa summarize src/`");
    assert_eq!(defect(&bare[0]).as_deref(), Some("`hexa summarize` does not exist"));
}

/// The analyze-arch recipe that failed every run: the nested evidence command
/// is checked, and so are the outer command's flags.
#[test]
fn nested_commands_and_flags_are_checked() {
    let found = extract("```\nhexa do run \"x\" --file a.rs --evidence \"hexa analyze . --exit-code\" --nope\n```");
    assert_eq!(found.len(), 2, "the nested evidence command was not extracted: {found:?}");
    let outer = found.iter().find(|i| i.args.first().is_some_and(|a| a == "do")).unwrap();
    assert_eq!(defect(outer).as_deref(), Some("`hexa do run` takes no `--nope`"));

    let ok = extract("`hexa do run \"x\" --file a.rs --evidence \"cargo test\"`");
    assert_eq!(defect(&ok[0]), None, "a correct do run was rejected");

    // The analyze-arch recipe: the analyzer is not a test runner.
    let recipe = extract("`hexa do run \"x\" --file a.rs --evidence \"hexa analyze . --exit-code\"`");
    let outer = recipe.iter().find(|i| i.args.first().is_some_and(|a| a == "do")).unwrap();
    assert!(defect(outer).is_some_and(|d| d.contains("--uncounted")), "a do run gated on the analyzer passed without --uncounted");
    let fixed = extract("`hexa do run \"x\" --file a.rs --evidence \"hexa analyze . --exit-code\" --uncounted`");
    let outer = fixed.iter().find(|i| i.args.first().is_some_and(|a| a == "do")).unwrap();
    assert_eq!(defect(outer), None);

    // A positional the command does not take: `hexa adr status <id>`.
    assert!(defect(&extract("`hexa adr status <id>`")[0]).is_some_and(|d| d.contains("positional")));
    assert_eq!(defect(&extract("`hexa adr status`")[0]), None);
    assert!(defect(&extract("`hexa adr nosuch`")[0]).is_some());
    assert_eq!(defect(&extract("`hexa graph consumers <path>`")[0]), None);
}

/// Prose and other tools are not invocations.
#[test]
fn prose_is_not_a_command() {
    assert!(extract("hexa is one binary. `cargo test` and `hexa-workflow`.").is_empty());
}

#[test]
fn every_shipped_skill_and_the_claude_md_section_name_real_commands() {
    let docs = shipped_guidance();
    assert!(docs.len() >= 12, "found only {} documents; the walker is broken", docs.len());

    let mut total = 0usize;
    let mut dead: Vec<String> = Vec::new();
    for (name, body) in &docs {
        for inv in extract(body) {
            total += 1;
            if let Some(why) = defect(&inv) {
                dead.push(format!("{name}: {}\n      {why}", inv.text));
            }
        }
    }
    // Vacuity guard: the skills and the section hold well over a hundred.
    assert!(total >= 100, "found only {total} hexa commands; the extractor is broken");
    assert!(dead.is_empty(), "{} command(s) hexa would not accept:\n  {}", dead.len(), dead.join("\n  "));
}
