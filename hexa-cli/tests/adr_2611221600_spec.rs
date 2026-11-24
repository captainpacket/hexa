//! ADR-2611221600: every hexa project gets agent orientation.
//!
//! One test per scenario of `docs/specs/ADR-2611221600.json`. The CLI ones run
//! the binary in a temporary directory with its own HOME; the property ones
//! call the rules in `hexa_core::domain::orientation` over many inputs.

use std::path::{Path, PathBuf};
use std::process::Command;

use hexa_core::domain::orientation::{
    graph_state, imports_agents_md, managed_body, module_readme, replace_managed, wrap, GraphState, Layer, MarkerError,
    AGENTS_MD, CLAUDE_MD,
};

struct Run {
    code: i32,
    out: String,
    stdout: String,
}

/// A temporary root holding `home/` (HOME) and `p/` (the project).
struct Fixture {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let dir = tmp.path().join("p");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        Fixture { _tmp: tmp, home, dir }
    }

    /// `hexa init [extra]` on the project, which must succeed.
    fn init(extra: &[&str]) -> Fixture {
        let f = Fixture::new();
        let mut args = vec!["init", "."];
        args.extend_from_slice(extra);
        let r = f.hexa(&args);
        assert_eq!(r.code, 0, "hexa init failed:\n{}", r.out);
        f
    }

    fn hexa(&self, args: &[&str]) -> Run {
        hexa_in(&self.dir, &self.home, args)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

fn hexa_in(dir: &Path, home: &Path, args: &[&str]) -> Run {
    let o = Command::new(env!("CARGO_BIN_EXE_hexa"))
        .args(args)
        .current_dir(dir)
        .env("HOME", home)
        .env("HEXA_HOME", home.join(".hexa"))
        .env("HEXA_NO_CLAUDE_MODS", "1")
        .env("HEXA_PROJECT_ROOT", dir)
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HEXA_SESSION_ID", "adr-2611221600-session")
        .env("NO_COLOR", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env_remove("HEXA_TASK")
        .env_remove("HEXA_INTERNAL")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run hexa");
    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
    Run { code: o.status.code().unwrap_or(-1), out: format!("{stdout}{}", String::from_utf8_lossy(&o.stderr)), stdout }
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn shipped(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The text of the `## <heading>` section of a Markdown document.
fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc.find(&format!("## {heading}")).unwrap_or_else(|| panic!("no `## {heading}` in:\n{doc}"));
    let rest = &doc[start..];
    let end = rest[3..].find("\n## ").map(|i| i + 3).unwrap_or(rest.len());
    &rest[..end]
}

// ── §1 ──────────────────────────────────────────────────

#[test]
fn s01_init_writes_agents_md_with_its_managed_section() {
    let f = Fixture::init(&[]);
    let md = f.read("AGENTS.md");
    assert!(md.contains(AGENTS_MD.start) && md.contains(AGENTS_MD.end), "no markers:\n{md}");
    let body = managed_body(&md, &AGENTS_MD).expect("well formed").expect("a managed section");
    for want in ["Start here", "Where builds run", "Find your place", "Several agents", "hexa do gate", "hexa graph context", "hexa graph consumers", "hexa memory"] {
        assert!(body.contains(want), "AGENTS.md's managed section lacks {want:?}:\n{body}");
    }
    let claude = f.read("CLAUDE.md");
    let cbody = managed_body(&claude, &CLAUDE_MD).expect("well formed").expect("CLAUDE.md has a managed section");
    assert!(cbody.lines().any(|l| l.trim() == "@AGENTS.md"), "CLAUDE.md does not import AGENTS.md:\n{claude}");
}

#[test]
fn s02_an_agents_md_without_markers_keeps_its_text_and_gets_the_section_appended() {
    let f = Fixture::init(&[]);
    let mine = "# Our agents\n\nHand-written. Keep every byte of this.\n";
    f.write("AGENTS.md", mine);
    let r = f.hexa(&["refresh", "."]);
    assert_eq!(r.code, 0, "{}", r.out);
    let md = f.read("AGENTS.md");
    assert!(md.starts_with(mine), "the hand-written text is not the unchanged prefix:\n{md}");
    assert_eq!(md.matches(AGENTS_MD.start).count(), 1, "the section is appended once:\n{md}");
    assert_eq!(f.hexa(&["refresh", "."]).code, 0);
    assert_eq!(f.read("AGENTS.md"), md, "a second refresh changed AGENTS.md");
}

// ── §2 ──────────────────────────────────────────────────

#[test]
fn s03_refresh_makes_claude_md_import_agents_md_and_keep_only_claude_parts() {
    let f = Fixture::init(&[]);
    f.write("CLAUDE.md", &format!("# p\n\nmine above\n\n{}\nold body\n{}\n\nmine below\n", CLAUDE_MD.start, CLAUDE_MD.end));
    let r = f.hexa(&["refresh", "."]);
    assert_eq!(r.code, 0, "{}", r.out);
    let md = f.read("CLAUDE.md");
    let body = managed_body(&md, &CLAUDE_MD).unwrap().unwrap();
    assert!(body.lines().any(|l| l.trim() == "@AGENTS.md"), "no import:\n{body}");
    assert!(!body.contains("old body"), "{body}");
    for want in ["hooks", ".claude/skills/", ".claude/agents/"] {
        assert!(body.contains(want), "the Claude-only part lacks {want:?}:\n{body}");
    }
    assert!(!body.contains("Don't do this by hand"), "CLAUDE.md repeats AGENTS.md's verb table:\n{body}");
}

#[test]
fn s04_a_claude_md_with_no_hexa_section_is_never_appended_to() {
    let f = Fixture::init(&[]);
    let mine = "# mine\n\nNo hexa section here, by choice.\n";
    f.write("CLAUDE.md", mine);
    let r = f.hexa(&["refresh", "."]);
    assert_eq!(f.read("CLAUDE.md"), mine, "refresh changed a CLAUDE.md with no hexa section");
    assert!(r.out.contains("no hexa-managed section"), "{}", r.out);
}

// ── §3 ──────────────────────────────────────────────────

#[test]
fn s05_refresh_replaces_only_the_managed_bytes() {
    let f = Fixture::init(&[]);
    for (file, m) in [("AGENTS.md", AGENTS_MD), ("CLAUDE.md", CLAUDE_MD)] {
        f.write(file, &format!("TOP of {file}\n\n{}\nstale\n{}\n\nBOTTOM of {file}\n", m.start, m.end));
    }
    let r = f.hexa(&["refresh", "."]);
    assert_eq!(r.code, 0, "{}", r.out);
    for (file, m) in [("AGENTS.md", AGENTS_MD), ("CLAUDE.md", CLAUDE_MD)] {
        let md = f.read(file);
        assert!(md.starts_with(&format!("TOP of {file}\n\n{}\n", m.start)), "{file} above the markers changed:\n{md}");
        assert!(md.ends_with(&format!("{}\n\nBOTTOM of {file}\n", m.end)), "{file} below the markers changed:\n{md}");
        assert_ne!(managed_body(&md, &m).unwrap(), Some("stale"), "{file}'s body was not refreshed");
    }
    let before = (f.read("AGENTS.md"), f.read("CLAUDE.md"));
    assert_eq!(f.hexa(&["refresh", "."]).code, 0);
    assert_eq!((f.read("AGENTS.md"), f.read("CLAUDE.md")), before, "a second refresh changed a file");
}

#[test]
fn s06_refresh_refuses_an_end_marker_before_its_start() {
    let f = Fixture::init(&[]);
    let bad = format!("{}\nx\n{}\n", AGENTS_MD.end, AGENTS_MD.start);
    f.write("AGENTS.md", &bad);
    let r = f.hexa(&["refresh", "."]);
    assert_ne!(r.code, 0, "refresh accepted a malformed AGENTS.md:\n{}", r.out);
    assert!(r.out.contains("AGENTS.md"), "the refusal names the file:\n{}", r.out);
    assert_eq!(f.read("AGENTS.md"), bad, "the malformed file was changed");
}

// ── §4 ──────────────────────────────────────────────────

const ROLE_FILES: [&str; 3] = [".claude/agents/hexa-lead.md", ".claude/agents/hexa-builder.md", ".claude/skills/hexa-parallel-agents/SKILL.md"];

#[test]
fn s07_init_installs_the_roles_and_the_parallel_agents_skill_with_their_rules() {
    let f = Fixture::init(&[]);
    let mut all = String::new();
    for rel in ROLE_FILES {
        assert!(f.path(rel).is_file(), "{rel} was not installed");
        all.push_str(&f.read(rel).to_lowercase());
    }
    for rule in [
        "cargo fmt",
        "never end the turn",
        "pushed head",
        "one writer per branch",
        "generated files",
        "never restart a ci runner",
        "split work by file",
        "integrate, never trust",
    ] {
        assert!(all.contains(rule), "the roles and skill do not carry the rule {rule:?}");
    }
}

#[test]
fn s08_skill_sync_restores_a_deleted_role_and_leaves_an_edited_one_alone() {
    let f = Fixture::init(&[]);
    std::fs::remove_file(f.path(".claude/agents/hexa-lead.md")).unwrap();
    let edited = "---\nname: hexa-builder\ndescription: ours\n---\n\nOur own builder.\n";
    f.write(".claude/agents/hexa-builder.md", edited);
    let r = f.hexa(&["skill", "sync"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(f.read(".claude/agents/hexa-lead.md"), shipped("agents/hexa-lead.md"), "the deleted role was not written back");
    assert_eq!(f.read(".claude/agents/hexa-builder.md"), edited, "sync overwrote the project's own copy");
    assert!(r.out.contains("hexa-builder.md"), "the differing copy is named:\n{}", r.out);
}

// ── §5 ──────────────────────────────────────────────────

#[test]
fn s09_init_scaffold_writes_a_readme_per_layer() {
    let f = Fixture::init(&["--scaffold"]);
    for (dir, rule) in [
        ("src/domain", "domain/ only"),
        ("src/ports", "domain/ only"),
        ("src/usecases", "domain/ and ports/"),
        ("src/adapters", "ports/"),
    ] {
        let md = f.read(&format!("{dir}/README.md"));
        for h in ["Owns", "May import", "Tests", "ADRs"] {
            section(&md, h);
        }
        assert!(section(&md, "May import").contains(rule), "{dir}'s May import does not state {rule:?}:\n{md}");
    }
}

fn missing_readmes(f: &Fixture) -> Vec<String> {
    let r = f.hexa(&["docs", "check", "--json", "--max-age-days", "0", "--root", "."]);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("docs check --json is JSON ({e}):\n{}", r.out));
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["kind"] == "missing_module_readme")
        .map(|x| x["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn s10_docs_check_flags_a_module_without_a_readme_and_docs_readmes_writes_it() {
    let f = Fixture::init(&["--scaffold"]);
    assert!(missing_readmes(&f).is_empty(), "a fresh scaffold has every README");
    std::fs::remove_file(f.path("src/ports/README.md")).unwrap();
    let missing = missing_readmes(&f);
    assert!(missing.iter().any(|p| p.contains("src/ports")), "src/ports is not flagged: {missing:?}");
    f.write("src/domain/README.md", "MINE\n");
    let r = f.hexa(&["docs", "readmes", "--root", "."]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(f.path("src/ports/README.md").is_file(), "docs readmes did not write src/ports/README.md");
    assert_eq!(f.read("src/domain/README.md"), "MINE\n", "docs readmes overwrote an existing README");
    assert!(missing_readmes(&f).is_empty(), "still flagged after docs readmes");
}

// ── §6 ──────────────────────────────────────────────────

#[test]
fn s11_doctor_names_each_missing_piece_with_its_fix() {
    let f = Fixture::init(&["--scaffold"]);
    std::fs::remove_file(f.path("AGENTS.md")).unwrap();
    std::fs::remove_file(f.path(".claude/skills/hexa-workflow/SKILL.md")).unwrap();
    std::fs::remove_file(f.path("src/ports/README.md")).unwrap();
    f.write(".claude/settings.json", "{\"hooks\": {}}\n");
    let r = f.hexa(&["doctor"]);
    assert_ne!(r.code, 0, "doctor passed with pieces missing:\n{}", r.out);
    let has = |needle: &str| r.out.lines().any(|l| l.contains(needle));
    assert!(r.out.lines().any(|l| l.contains("AGENTS.md") && l.contains("hexa refresh")), "AGENTS.md and its fix:\n{}", r.out);
    assert!(r.out.lines().any(|l| l.contains("hook") && l.contains("hexa refresh")), "the hooks and their fix:\n{}", r.out);
    assert!(r.out.lines().any(|l| l.contains("hexa-workflow") && l.contains("hexa skill sync")), "the skill and its fix:\n{}", r.out);
    assert!(r.out.lines().any(|l| l.contains("src/ports") && l.contains("hexa docs readmes")), "the README and its fix:\n{}", r.out);
    let graph = r.out.lines().find(|l| l.to_lowercase().contains("graph")).unwrap_or_else(|| panic!("no graph line:\n{}", r.out));
    assert!(["fresh", "stale", "missing", "unknown"].iter().any(|s| graph.contains(s)), "graph state: {graph}");
    if graph.contains("stale") || graph.contains("missing") {
        assert!(has("hexa graph build"), "a stale or missing graph names its fix:\n{}", r.out);
    }
}

fn orientation_items(r: &Run) -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("finish --json is JSON ({e}):\n{}", r.out));
    v["open"].as_array().unwrap().iter().filter(|i| i["kind"] == "orientation").cloned().collect()
}

#[test]
fn s12_finish_lists_failing_orientation_only_in_an_initialised_project() {
    let f = Fixture::init(&[]);
    std::fs::remove_file(f.path("AGENTS.md")).unwrap();
    let items = orientation_items(&f.hexa(&["finish", "--json"]));
    assert!(
        items.iter().any(|i| i["what"].as_str().unwrap().contains("AGENTS.md") && i["fix"].as_str().unwrap().contains("hexa refresh")),
        "no orientation item for AGENTS.md: {items:?}"
    );
    assert_eq!(f.hexa(&["finish", "--exit-code"]).code, 1);

    let g = Fixture::new();
    git(&g.dir, &["init", "-q"]);
    std::fs::create_dir_all(g.path(".hexa")).unwrap();
    g.write(".gitignore", ".hexa/\n");
    g.write("a.txt", "one\n");
    git(&g.dir, &["add", "-A"]);
    git(&g.dir, &["commit", "-q", "-m", "init"]);
    let items = orientation_items(&g.hexa(&["finish", "--json"]));
    assert!(items.is_empty(), "a repository with no .hexa/project.json gets orientation items: {items:?}");
}

// ── properties ──────────────────────────────────────────

fn bodies() -> Vec<String> {
    let mut v: Vec<String> = ["x", "  padded  ", "two\nlines", "## heading\n\n- item\n", "unicode \u{e9}\u{2b21}", "`code` and @AGENTS.md"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for n in 1..20 {
        v.push((0..n).map(|i| format!("line {i} {}", "w".repeat(i % 5))).collect::<Vec<_>>().join("\n"));
    }
    v
}

#[test]
fn s13_wrap_puts_the_body_between_the_markers_on_their_own_lines() {
    for m in [AGENTS_MD, CLAUDE_MD] {
        for b in bodies() {
            let w = wrap(&m, &b);
            assert_eq!(w, format!("{}\n{}\n{}", m.start, b.trim(), m.end));
            assert_eq!(managed_body(&w, &m).unwrap(), Some(b.trim()));
        }
    }
}

#[test]
fn s14_imports_agents_md_only_for_the_exact_line_outside_a_fence() {
    for (text, want) in [
        ("@AGENTS.md", true),
        ("# p\n\n@AGENTS.md\n\nmore", true),
        ("  @AGENTS.md  \n", true),
        ("see @AGENTS.md for more", false),
        ("@AGENTS.md.bak", false),
        ("@./AGENTS.md.old", false),
        ("```\n@AGENTS.md\n```\n", false),
        ("```md\n@AGENTS.md\n```\nafter", false),
        ("```\nx\n```\n@AGENTS.md\n", true),
        ("", false),
        ("AGENTS.md", false),
    ] {
        assert_eq!(imports_agents_md(text), want, "{text:?}");
    }
}

#[test]
fn s15_module_readme_always_has_the_four_headings() {
    let layers = [
        None,
        Some(Layer::Domain),
        Some(Layer::Ports),
        Some(Layer::Usecases),
        Some(Layer::PrimaryAdapters),
        Some(Layer::SecondaryAdapters),
        Some(Layer::Adapters),
    ];
    for name in ["src/domain", "core", "my-crate", "internal/ports", "a b"] {
        for layer in layers {
            let md = module_readme(name, layer, "cargo test");
            for h in ["## Owns", "## May import", "## Tests", "## ADRs"] {
                assert!(md.contains(h), "{name} {layer:?} lacks {h}:\n{md}");
            }
            assert!(md.contains(name), "{name} is not named:\n{md}");
            if let Some(l) = layer {
                assert!(section(&md, "May import").contains(l.may_import()), "{name} {layer:?}:\n{md}");
            }
        }
    }
    assert_eq!(Layer::of_dir("src/domain"), Some(Layer::Domain));
    assert_eq!(Layer::of_dir("internal/ports"), Some(Layer::Ports));
    assert_eq!(Layer::of_dir("src/core/usecases"), Some(Layer::Usecases));
    assert_eq!(Layer::of_dir("src/adapters/primary"), Some(Layer::PrimaryAdapters));
    assert_eq!(Layer::of_dir("adapters/secondary"), Some(Layer::SecondaryAdapters));
    assert_eq!(Layer::of_dir("src/adapters"), Some(Layer::Adapters));
    assert_eq!(Layer::of_dir("src/commands"), None);
}

#[test]
fn s16_graph_state_reads_the_state_field() {
    for (out, want) in [
        (Some(r#"{"state":"fresh","built":true,"fresh":true}"#), GraphState::Fresh),
        (Some(r#"{"state":"stale","built":true,"fresh":false}"#), GraphState::Stale),
        (Some(r#"{"state":"missing","built":false,"fresh":false}"#), GraphState::Missing),
        (Some(r#"{"state":"weird"}"#), GraphState::Unknown),
        (Some(r#"{"fresh":true}"#), GraphState::Unknown),
        (Some("error: unrecognized subcommand 'status'"), GraphState::Unknown),
        (Some(""), GraphState::Unknown),
        (None, GraphState::Unknown),
    ] {
        assert_eq!(graph_state(out), want, "{out:?}");
    }
}

#[test]
fn s17_replace_managed_changes_only_the_managed_bytes_and_is_idempotent() {
    let edges = ["", "top\n\n", "# title\nhand text\n\n", "no trailing newline"];
    for m in [AGENTS_MD, CLAUDE_MD] {
        for pre in edges {
            for post in edges {
                for (old, new) in bodies().iter().zip(bodies().iter().rev()) {
                    let text = format!("{pre}{}{post}", wrap(&m, old));
                    let once = replace_managed(&text, &m, new).unwrap().expect("markers present");
                    assert!(once.starts_with(pre) && once.ends_with(post), "outside bytes changed:\n{once}");
                    assert_eq!(managed_body(&once, &m).unwrap(), Some(new.trim()));
                    let twice = replace_managed(&once, &m, new).unwrap().unwrap();
                    assert_eq!(twice, once, "not idempotent");
                }
            }
            assert_eq!(replace_managed(pre, &m, "x").unwrap(), None, "no markers is None");
        }
        assert_eq!(replace_managed(&format!("{}\n{}", m.end, m.start), &m, "x"), Err(MarkerError::EndBeforeStart));
        assert_eq!(replace_managed(&format!("a\n{}\nb", m.start), &m, "x"), Err(MarkerError::Unpaired));
    }
}

/// Every verb `hexa --help` lists.
fn verbs() -> Vec<String> {
    let f = Fixture::new();
    let r = f.hexa(&["--help"]);
    let mut out = Vec::new();
    let mut on = false;
    for l in r.out.lines() {
        if l.starts_with("Commands:") {
            on = true;
            continue;
        }
        if on {
            if !l.starts_with("  ") {
                break;
            }
            if let Some(w) = l.split_whitespace().next() {
                out.push(w.to_string());
            }
        }
    }
    assert!(out.len() > 10, "could not read the verbs:\n{}", r.out);
    out
}

#[test]
fn s18_every_shipped_role_and_the_skill_are_project_neutral() {
    let verbs = verbs();
    let forbidden = ["datum", "bazzite", "tt", "quietbox", "xtask", "kairos", "strata", "jaco"];
    for rel in ["agents/hexa-lead.md", "agents/hexa-builder.md", "skills/hexa-parallel-agents/SKILL.md"] {
        let text = shipped(rel);
        let lower = text.to_lowercase();
        for word in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
            assert!(!forbidden.contains(&word), "{rel} names {word:?}, which belongs to one project");
        }
        assert!(!lower.contains("remote-gate.sh"), "{rel} names a project's own script");
        for (i, _) in text.match_indices("`hexa ") {
            let verb: String = text[i + 6..].chars().take_while(|c| c.is_ascii_lowercase() || *c == '-').collect();
            assert!(verbs.contains(&verb), "{rel} names `hexa {verb}`, which `hexa --help` does not list");
        }
    }
}
