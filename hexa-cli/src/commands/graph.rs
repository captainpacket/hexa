//! `hexa graph` — build and query a knowledge graph of the project.
//!
//! In-process over the `hexa-graph` engine (ADR-2608241500 P6.2). Every verb
//! here used to POST to the daemon's `/api/graph/*` routes, which then called
//! the same library functions this file now calls directly. `consumers` was
//! already standalone — and that asymmetry was the tell: the excision oracle
//! read the graph file off disk, while `build` could not run at all
//! without a daemon, so the file the oracle depends on could go stale with no
//! way to refresh it.
//!
//! Core verbs: build / query / path / explain / context / consumers.
//!
//! # What was dropped
//!
//! `--persist`, which mirrored the graph into the `knowledge-graph`
//! SpacetimeDB module. The graph file (now `.hexa/graph/graph.json`, formerly
//! `graph-out/graph.json`) was always the query source of truth; the mirror served the dashboard, which is going with the daemon.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde_json::json;

use hexa_graph::ports::KnowledgeGraph;
use hexa_graph::query as gquery;
use hexa_graph::semantic::{
    NoopSemanticExtractor, SemanticContext, SemanticExtractor, SemanticTriple,
};
use hexa_graph::{BuildOpts, Mode};


#[derive(Debug, Subcommand)]
pub enum GraphAction {
    /// Build (or rebuild) the knowledge graph for a project directory.
    Build(BuildArgs),
    /// Search the graph with a natural-language question.
    Query(QueryArgs),
    /// Find the shortest path between two nodes (id or exact label).
    Path(PathArgs),
    /// Explain a node — its kind, community, and relationships.
    Explain(ExplainArgs),
    /// Emit a file's graph neighbourhood as agent-ready context (defines, uses,
    /// consumers, community) — trace consumers before you edit.
    Context(ContextArgs),
    /// Who depends on a module/file — the excision-safety oracle (ADR-2606071340).
    ///
    /// Reports inbound importers + entity consumers, with a SAFE-TO-REMOVE /
    /// BLOCKED verdict. This is the graph-driven dead-code check that drives
    /// safe excision — `hexa` doing "trace ALL consumers before deleting"
    /// itself, deterministically.
    Consumers(ConsumersArgs),
    /// Whether the graph can be trusted: missing, stale or fresh, when it was
    /// built and how many nodes it holds (ADR-2611221610 §3). Exits 0 in every state.
    Status(StatusArgs),
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Project directory (default: detected root).
    #[arg(long, default_value = ".")]
    pub path: String,
    /// Emit `{state, built, fresh, built_at, nodes, changed}`.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Project directory to analyze (default: detected project root).
    #[arg(default_value = ".")]
    pub path: String,
    /// "ast" (default, no LLM) or "deep" (LLM-inferred edges from docs).
    #[arg(long, default_value = "ast")]
    pub mode: String,
    /// Skip documentation (Markdown) nodes.
    #[arg(long)]
    pub no_docs: bool,
    /// Model for deep-mode semantic inference.
    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    /// The question to search the graph with.
    pub question: String,
    #[arg(long, default_value = ".")]
    pub path: String,
    #[arg(long, default_value_t = 15)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct PathArgs {
    /// Source node (id or exact label).
    pub from: String,
    /// Target node (id or exact label).
    pub to: String,
    #[arg(long, default_value = ".")]
    pub path: String,
}

#[derive(Debug, Args)]
pub struct ExplainArgs {
    /// Node to explain (id or exact label).
    pub node: String,
    #[arg(long, default_value = ".")]
    pub path: String,
}

#[derive(Debug, Args)]
pub struct ContextArgs {
    /// File (or any node) to build neighbourhood context for.
    pub target: String,
    #[arg(long, default_value = ".")]
    pub path: String,
    /// Max items per list.
    #[arg(long, default_value_t = 25)]
    pub max_each: usize,
    /// Emit the raw JSON bundle instead of rendered Markdown.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ConsumersArgs {
    /// Module or file to check (a repo-relative path like
    /// `hexa-exec/src/tools/delegate.rs`, or a node id/label).
    pub target: String,
    /// Project directory holding `.hexa/graph/graph.json` (default: detected root).
    #[arg(long, default_value = ".")]
    pub path: String,
    /// Max consumers listed per category.
    #[arg(long, default_value_t = 50)]
    pub max_each: usize,
    /// Emit JSON `{ target, safe_to_remove, imported_by, used_by, declared_in,
    /// stale, newer_files }`.
    #[arg(long)]
    pub json: bool,
}

pub async fn run(action: GraphAction) -> anyhow::Result<()> {
    match action {
        GraphAction::Build(a) => build(a).await,
        GraphAction::Query(a) => query(a),
        GraphAction::Path(a) => path(a),
        GraphAction::Explain(a) => explain(a),
        GraphAction::Context(a) => context(a).await,
        GraphAction::Consumers(a) => consumers(a),
        GraphAction::Status(a) => status(a),
    }
}

// ── root and graph io ────────────────────────────────────────────────────────

/// Resolve an argument `path` (absolute or cwd-relative), or detect the root.
fn resolve_root(path: &str) -> anyhow::Result<PathBuf> {
    if !path.is_empty() && path != "." {
        let pb = PathBuf::from(path);
        if pb.is_dir() {
            return Ok(pb);
        }
        let joined = std::env::current_dir()?.join(path);
        if joined.is_dir() {
            return Ok(joined);
        }
        anyhow::bail!("not a directory: {path}");
    }
    if let Ok(root) = std::env::var("HEXA_PROJECT_ROOT") {
        let p = PathBuf::from(&root);
        if p.is_dir() {
            return Ok(p);
        }
    }
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join("CLAUDE.md").exists() || dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return Ok(cwd),
        }
    }
}

/// The graph to read: `.hexa/graph/graph.json`, or the legacy
/// `graph-out/graph.json` an older build left.
fn graph_path(root: &Path) -> PathBuf {
    hexa_graph::graph_file(root)
}

/// Load the graph and warn on stderr when the source has moved past it.
fn load_graph(path: &str) -> anyhow::Result<KnowledgeGraph> {
    Ok(load_graph_checked(path)?.0)
}

/// Load the graph, warn when it is stale, and return the newer files too.
fn load_graph_checked(path: &str) -> anyhow::Result<(KnowledgeGraph, Vec<String>)> {
    let graph = read_graph(path)?;
    let newer = newer_than_graph(path, STALE_LIMIT)?;
    warn_if_stale(&newer);
    Ok((graph, newer))
}

fn read_graph(path: &str) -> anyhow::Result<KnowledgeGraph> {
    let root = resolve_root(path)?;
    let out_path = graph_path(&root);
    let raw = std::fs::read_to_string(&out_path).map_err(|e| {
        anyhow::anyhow!(
            "no graph at {} ({e}). Build it first: `hexa graph build`",
            out_path.display()
        )
    })?;
    KnowledgeGraph::from_json(&raw)
        .map_err(|e| anyhow::anyhow!("corrupt {}: {e}", out_path.display()))
}

/// Source files changed since the graph was written, at most `limit`. Empty
/// when the graph's age cannot be read, so a missing mtime never invents
/// staleness.
fn newer_than_graph(path: &str, limit: usize) -> anyhow::Result<Vec<String>> {
    let root = resolve_root(path)?;
    let Ok(written) = std::fs::metadata(graph_path(&root)).and_then(|m| m.modified()) else {
        return Ok(Vec::new());
    };
    Ok(hexa_graph::newer_sources(&root, written, limit))
}

/// Tell the reader, on stderr, that the graph predates the source it describes.
fn warn_if_stale(newer: &[String]) {
    if newer.is_empty() {
        return;
    }
    eprintln!(
        "{} the graph is older than {} source file(s) ({}{}): its answer may be wrong. \
         Rebuild it: `hexa graph build`",
        "warning:".yellow().bold(),
        if newer.len() >= STALE_LIMIT { format!("{}+", newer.len()) } else { newer.len().to_string() },
        newer.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
        if newer.len() > 3 { ", …" } else { "" },
    );
}

const STALE_LIMIT: usize = 20;

/// Write the graph atomically: a temporary file, then a rename, so a reader
/// never sees half a graph (ADR-2611221610 §2).
fn write_graph(out_path: &Path, graph: &KnowledgeGraph) -> anyhow::Result<()> {
    let json = graph.to_json().map_err(|e| anyhow::anyhow!("serialize graph: {e}"))?;
    write_atomic(out_path, json.as_bytes())
}

/// `.hexa/graph/` is build output: it ignores itself, so a project whose own
/// `.gitignore` does not cover it never sees the graph, its stamp, its lock or
/// its markers as untracked files.
fn ensure_graph_dir(root: &Path) -> anyhow::Result<()> {
    let dir = root.join(".hexa/graph");
    std::fs::create_dir_all(&dir)?;
    let ignore = dir.join(".gitignore");
    if !ignore.is_file() {
        std::fs::write(ignore, "# hexa graph build output (ADR-2611221610)\n*\n")?;
    }
    Ok(())
}

fn write_atomic(out_path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = out_path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, out_path)?;
    Ok(())
}

// ── freshness (ADR-2611221610 §2, §3, §5, §6) ────────────────────────────────

/// Whether the graph at a root can be trusted.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphState {
    /// `missing`, `stale` or `fresh`.
    pub state: &'static str,
    pub built_at: Option<String>,
    pub nodes: u64,
    /// Source files modified since the build.
    pub changed: usize,
}

impl GraphState {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "state": self.state,
            "built": self.state != "missing",
            "fresh": self.state == "fresh",
            "built_at": self.built_at,
            "nodes": self.nodes,
            "changed": self.changed,
        })
    }
}

fn read_stamp(root: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(root.join(hexa_graph::STAMP_FILE)).ok()?).ok()
}

/// The graph's state at `root`: `missing` with no graph file, `stale` with no
/// stamp or a fingerprint that differs from the tree's, else `fresh`.
pub fn graph_state(root: &Path) -> GraphState {
    let file = graph_path(root);
    let Ok(meta) = std::fs::metadata(&file) else {
        return GraphState { state: "missing", built_at: None, nodes: 0, changed: 0 };
    };
    let stamp = read_stamp(root);
    let built_at = stamp
        .as_ref()
        .and_then(|s| s["built_at"].as_str().map(str::to_string))
        .or_else(|| meta.modified().ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()));
    let since = built_at
        .as_deref()
        .and_then(|b| chrono::DateTime::parse_from_rfc3339(b).ok())
        .map(std::time::SystemTime::from)
        .or_else(|| meta.modified().ok());
    let changed = since.map_or(0, |t| hexa_graph::newer_sources(root, t, usize::MAX).len());
    let nodes = stamp.as_ref().and_then(|s| s["nodes"].as_u64()).unwrap_or(0);
    let fresh = stamp
        .as_ref()
        .and_then(|s| s["fingerprint"].as_str())
        .is_some_and(|fp| fp == hexa_graph::tree_fingerprint(root));
    GraphState { state: if fresh { "fresh" } else { "stale" }, built_at, nodes, changed }
}

const LOCK_FILE: &str = ".hexa/graph/build.lock";
const LOG_FILE: &str = ".hexa/graph/build.log";
/// A lock older than this is a build that died, not one still running.
const LOCK_LIFETIME: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn lock_held(root: &Path) -> bool {
    std::fs::metadata(root.join(LOCK_FILE))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < LOCK_LIFETIME)
}

/// Build the graph for `root`, write it and its stamp, and return it.
async fn build_and_stamp(root: &Path, mode: Mode, include_docs: bool, model: Option<String>) -> anyhow::Result<KnowledgeGraph> {
    let project_id = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "project".to_string());
    let semantic: Box<dyn SemanticExtractor> =
        if mode == Mode::Deep { Box::new(LocalSemanticExtractor { model }) } else { Box::new(NoopSemanticExtractor) };
    // Before the read: a file changed during the build leaves the stamp stale, never falsely fresh.
    ensure_graph_dir(root)?;
    let fingerprint = hexa_graph::tree_fingerprint(root);
    let started = chrono::Utc::now().to_rfc3339();
    let opts = BuildOpts { project_id, mode, include_docs, ..Default::default() };
    let graph = hexa_graph::build(root, opts, semantic.as_ref()).await;
    let graph = match graph {
        Ok(g) => g,
        Err(e) => {
            let _ = std::fs::remove_file(root.join(LOCK_FILE));
            return Err(e);
        }
    };
    // Always the current location; the legacy file is only ever read.
    write_graph(&root.join(hexa_graph::GRAPH_FILE), &graph)?;
    let stamp = json!({
        "built_at": started,
        "nodes": graph.meta.node_count,
        "edges": graph.meta.edge_count,
        "fingerprint": fingerprint,
    });
    write_atomic(&root.join(hexa_graph::STAMP_FILE), serde_json::to_string_pretty(&stamp)?.as_bytes())?;
    let _ = std::fs::remove_file(root.join(LOCK_FILE));
    Ok(graph)
}

/// `hexa init` builds the graph (ADR-2611221610 §4). One line either way; a
/// failure never fails init.
pub async fn build_after_init(root: &Path) {
    match build_and_stamp(root, Mode::Ast, true, None).await {
        Ok(g) => println!("  {} .hexa/graph/graph.json ({} nodes)", "\u{2713}".green(), g.meta.node_count),
        Err(e) => println!("  {} code graph not built ({e}); `hexa graph build`", "\u{2717}".yellow()),
    }
}

/// The SessionStart line, starting a detached rebuild when the graph is
/// missing or stale and no rebuild holds the lock (ADR-2611221610 §5). The
/// session never waits for it.
pub fn refresh_for_session(root: &Path) -> String {
    let st = graph_state(root);
    if st.state == "fresh" {
        return format!("graph: fresh ({} nodes)", st.nodes);
    }
    let what = match st.state {
        "missing" => "missing".to_string(),
        _ if st.changed > 0 => format!("stale ({} file(s) changed since the build)", st.changed),
        _ => "stale".to_string(),
    };
    if lock_held(root) {
        return format!("graph: {what} \u{2014} a rebuild is already running");
    }
    match spawn_background_build(root) {
        Ok(()) => format!("graph: {what} \u{2014} rebuilding in the background"),
        Err(e) => format!("graph: {what} \u{2014} could not start a rebuild ({e}); run `hexa graph build`"),
    }
}

fn spawn_background_build(root: &Path) -> anyhow::Result<()> {
    ensure_graph_dir(root)?;
    std::fs::write(root.join(LOCK_FILE), format!("{}\n", chrono::Utc::now().to_rfc3339()))?;
    let log = std::fs::File::create(root.join(LOG_FILE))?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args(["graph", "build"])
        .arg(root)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Err(e) = cmd.spawn() {
        let _ = std::fs::remove_file(root.join(LOCK_FILE));
        return Err(e.into());
    }
    Ok(())
}

/// At most this many lines of edit context, and this many items per list
/// (ADR-2611221610 §6).
pub const EDIT_CONTEXT_LINES: usize = 12;
const EDIT_CONTEXT_EACH: usize = 5;

/// The compact graph context the edit hook prints for `rel` (a path from
/// `root`): empty when there is no graph or the graph does not hold the file.
pub fn edit_context(root: &Path, rel: &str) -> Vec<String> {
    let file = graph_path(root);
    let Ok(raw) = std::fs::read_to_string(&file) else { return Vec::new() };
    let Ok(graph) = KnowledgeGraph::from_json(&raw) else { return Vec::new() };
    let opts = hexa_graph::context::ContextOpts { max_each: EDIT_CONTEXT_EACH };
    let Some(b) = hexa_graph::context::context_for(&graph, rel, opts) else { return Vec::new() };
    if b.kind != "file" {
        return Vec::new();
    }
    let stale = graph_state(root).state != "fresh";
    let list = |items: Vec<String>, key: &str| -> String {
        let more = b.truncated.get(key).copied().unwrap_or(0);
        let mut s = items.join(", ");
        if more > 0 {
            s.push_str(&format!(" (+{more})"));
        }
        s
    };
    let mut lines = vec![format!(
        "[HEX] graph: {} \u{2014} defines {} \u{b7} imported by {} \u{b7} used by {}{}",
        b.target,
        b.defines.len() + b.truncated.get("defines").copied().unwrap_or(0),
        b.imported_by.len() + b.truncated.get("imported_by").copied().unwrap_or(0),
        b.used_by.len() + b.truncated.get("used_by").copied().unwrap_or(0),
        if stale { " \u{b7} STALE graph: `hexa graph build`" } else { "" }
    )];
    if !b.defines.is_empty() {
        lines.push(format!("  defines: {}", list(b.defines.iter().map(|e| e.label.clone()).collect(), "defines")));
    }
    if !b.imported_by.is_empty() {
        lines.push(format!("  imported by: {}", list(b.imported_by.clone(), "imported_by")));
    }
    if !b.declared_in.is_empty() {
        lines.push(format!("  declared in: {}", list(b.declared_in.clone(), "declared_in")));
    }
    if !b.used_by.is_empty() {
        lines.push(format!(
            "  used by: {}",
            list(b.used_by.iter().map(|u| format!("{} \u{2192} {}", u.file, u.entity)).collect(), "used_by")
        ));
    }
    if !b.imports.is_empty() {
        lines.push(format!("  imports: {}", list(b.imports.clone(), "imports")));
    }
    lines.push(format!("  more: `hexa graph context {}` \u{b7} before deleting: `hexa graph consumers {}`", b.target, b.target));
    lines.truncate(EDIT_CONTEXT_LINES);
    lines
}

fn status(a: StatusArgs) -> anyhow::Result<()> {
    let root = resolve_root(&a.path)?;
    let st = graph_state(&root);
    if a.json {
        println!("{}", st.to_json());
        return Ok(());
    }
    match st.state {
        "missing" => println!("{} graph: missing \u{2014} `hexa graph build`", "\u{2717}".yellow()),
        "stale" => println!(
            "{} graph: stale ({} nodes, built {}, {} file(s) changed since) \u{2014} `hexa graph build`",
            "\u{26a0}".yellow(),
            st.nodes,
            st.built_at.as_deref().unwrap_or("?"),
            st.changed
        ),
        _ => println!("{} graph: fresh ({} nodes, built {})", "\u{2713}".green(), st.nodes, st.built_at.as_deref().unwrap_or("?")),
    }
    Ok(())
}

// ── deep-mode semantic inference ─────────────────────────────────────────────

/// Mines relationship triples out of prose with one inference call per chunk.
///
/// Lifted from the daemon's `NexusSemanticExtractor`, which held an
/// `Arc<dyn IInferencePort>` off `AppState`. It now goes through
/// `hexa_infer`, which resolves the backend from the registry — so no provider
/// name appears here (founding goal G1).
struct LocalSemanticExtractor {
    model: Option<String>,
}

#[async_trait::async_trait]
impl SemanticExtractor for LocalSemanticExtractor {
    async fn infer_edges(&self, ctx: &SemanticContext) -> Vec<SemanticTriple> {
        // Trim very large prose to keep the prompt bounded.
        let prose: String = ctx.text.chars().take(6000).collect();
        let known = ctx.known_labels.join(", ");
        let prompt = format!(
            "Extract concept relationships from the documentation below. \
             Return ONLY a JSON array of objects with keys: source, target, relation, \
             confident (boolean). Prefer linking to these known entities when \
             relevant: {known}.\n\nDOC ({}):\n{prose}",
            ctx.file
        );
        // Degrade gracefully — the AST graph still stands without these edges.
        let model = self.model.as_deref().unwrap_or_default();
        let Ok(reply) = hexa_infer::complete_text(
            model,
            "You are a precise knowledge-graph relationship extractor. Output JSON only.",
            &prompt,
            1024,
        )
        .await
        else {
            return Vec::new();
        };
        parse_triples(&reply)
    }
}

/// Leniently parse a JSON array of triples from a model response.
fn parse_triples(text: &str) -> Vec<SemanticTriple> {
    let (start, end) = match (text.find('['), text.rfind(']')) {
        (Some(s), Some(e)) if e > s => (s, e),
        _ => return Vec::new(),
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text[start..=end]) else {
        return Vec::new();
    };
    let Some(arr) = parsed.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|o| {
            let source = o.get("source")?.as_str()?.trim().to_string();
            let target = o.get("target")?.as_str()?.trim().to_string();
            if source.is_empty() || target.is_empty() {
                return None;
            }
            Some(SemanticTriple {
                source,
                target,
                relation: o
                    .get("relation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("related")
                    .to_string(),
                confident: o.get("confident").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        })
        .collect()
}

// ── verbs ────────────────────────────────────────────────────────────────────

async fn build(a: BuildArgs) -> anyhow::Result<()> {
    let root = resolve_root(&a.path)?;
    let mode = Mode::from_label(&a.mode);
    println!("{} building knowledge graph ({})…", "\u{2b21}".cyan(), a.mode);
    let graph = build_and_stamp(&root, mode, !a.no_docs, a.model.clone()).await?;
    let out_path = root.join(hexa_graph::GRAPH_FILE);

    println!(
        "{} {} nodes, {} edges, {} communities  [{}]",
        "\u{2713}".green(),
        graph.meta.node_count,
        graph.meta.edge_count,
        graph.meta.community_count,
        graph.meta.mode,
    );
    println!("  {} {}", "graph:".dimmed(), out_path.display());
    let hubs: Vec<&str> = graph.meta.god_nodes.iter().map(String::as_str).take(5).collect();
    if !hubs.is_empty() {
        println!("  {} {}", "hubs:".dimmed(), hubs.join(", "));
    }
    Ok(())
}

fn query(a: QueryArgs) -> anyhow::Result<()> {
    let graph = load_graph(&a.path)?;
    let results = gquery::query(&graph, &a.question, a.limit);
    if results.is_empty() {
        println!("{} no matches for {:?}", "\u{2014}".dimmed(), a.question);
        return Ok(());
    }
    println!("{} results for {:?}:", "\u{2b21}".cyan(), a.question);
    for r in &results {
        println!(
            "  {:<28} {:<10} {}  {}",
            r.label.bold(),
            r.kind.dimmed(),
            r.file.dimmed(),
            format!("{:.1}", r.score).dimmed()
        );
    }
    Ok(())
}

fn path(a: PathArgs) -> anyhow::Result<()> {
    let graph = load_graph(&a.path)?;
    for end in [&a.from, &a.to] {
        if graph.node(end).is_none() && graph.node_by_label(end).is_none() {
            anyhow::bail!("node not found: {end} (give a node id or an exact label)");
        }
    }
    match gquery::shortest_path(&graph, &a.from, &a.to) {
        Some(ids) => {
            let labels: Vec<String> = ids
                .iter()
                .map(|id| graph.node(id).map(|n| n.label.clone()).unwrap_or_else(|| id.clone()))
                .collect();
            println!(
                "{} {}",
                "\u{2b21}".cyan(),
                labels.join(&format!(" {} ", "\u{2192}".dimmed()))
            );
        }
        None => println!("{} no path between {:?} and {:?}", "\u{2014}".dimmed(), a.from, a.to),
    }
    Ok(())
}

fn explain(a: ExplainArgs) -> anyhow::Result<()> {
    let graph = load_graph(&a.path)?;
    let Some(ex) = gquery::explain(&graph, &a.node) else {
        anyhow::bail!("node not found: {}", a.node);
    };
    println!("{} {} {}", "\u{2b21}".cyan(), ex.label.bold(), format!("({})", ex.kind).dimmed());
    if !ex.file.is_empty() {
        println!("  {} {}:{}", "at:".dimmed(), ex.file, ex.line);
    }
    println!("  {} {}  ({})", "community:".dimmed(), ex.community_label, ex.degree);
    for n in ex.neighbors.iter().take(20) {
        let arrow = if n.direction == "out" { "\u{2192}" } else { "\u{2190}" };
        println!(
            "    {} {:<14} {}  {}",
            arrow.dimmed(),
            n.relation.dimmed(),
            n.label,
            format!("[{}]", n.confidence).dimmed()
        );
    }
    Ok(())
}

async fn context(a: ContextArgs) -> anyhow::Result<()> {
    let graph = load_graph(&a.path)?;
    let opts = hexa_graph::context::ContextOpts { max_each: a.max_each.clamp(1, 200) };
    let Some(bundle) = hexa_graph::context::context_for(&graph, &a.target, opts) else {
        anyhow::bail!("no file node for target: {}", a.target);
    };

    let mut markdown = hexa_graph::context::render_markdown(&bundle);
    // Graph-relevant memory: lessons whose text mentions this file's
    // neighbourhood (path/symbols), ranked — not arbitrary recency.
    let lessons = hexa_exec::direct_exec::fetch_lessons(&hexa_exec::memory()).await;
    let ranked = hexa_graph::context::rank_lessons(&bundle, &lessons, 6);
    if !ranked.is_empty() {
        markdown.push_str("\n## Lessons (most relevant to this file)\n");
        for l in &ranked {
            markdown.push_str(&format!("- [{}] {}\n", l.key, l.value));
        }
    }

    if a.json {
        let mut value = serde_json::to_value(&bundle)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("markdown".to_string(), json!(markdown));
            obj.insert("lessons".to_string(), serde_json::to_value(&ranked)?);
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    print!("{markdown}");
    Ok(())
}

/// Standalone graph-driven consumer trace + delete-safety verdict. Reuses the
/// same `hexa_graph::context` engine the executor uses, so "trace ALL consumers
/// before deleting" (ADR-2026-04-05-0900) is a deterministic hexa verb instead
/// of a manual grep.
fn consumers(a: ConsumersArgs) -> anyhow::Result<()> {
    let (graph, newer) = load_graph_checked(&a.path)?;
    let bundle = hexa_graph::context::context_for(
        &graph,
        &a.target,
        hexa_graph::context::ContextOpts { max_each: a.max_each },
    );
    let Some(b) = bundle else {
        anyhow::bail!(
            "'{}' not found in the graph — check the path, or rebuild with `hexa graph build`",
            a.target
        );
    };

    let importers: Vec<String> = b.imported_by.clone();
    let users: Vec<String> =
        b.used_by.iter().map(|u| format!("{} (uses {})", u.file, u.entity)).collect();
    // A `mod x;` line is where a module is wired in, not a caller of it: it is
    // listed, and it does not block the verdict.
    let declared_in: Vec<String> = b.declared_in.clone();
    let safe = importers.is_empty() && users.is_empty();

    if a.json {
        println!(
            "{}",
            json!({
                "target": b.label,
                "safe_to_remove": safe,
                "imported_by": importers,
                "used_by": users,
                "declared_in": declared_in,
                "stale": !newer.is_empty(),
                "newer_files": newer,
            })
        );
        return Ok(());
    }

    println!("{} {}", "⬡ consumers of".cyan().bold(), b.label.bold());
    println!("  {} {}  ·  degree {}", "kind".dimmed(), b.kind, b.degree);
    if safe {
        println!(
            "\n  {} no inbound importers or entity consumers in the graph.",
            "SAFE TO REMOVE —".green().bold()
        );
        for f in &declared_in {
            println!("  {} {} (remove its `mod` line too)", "declared in".dimmed(), f);
        }
        println!(
            "  {}",
            "Confirm with `cargo check --workspace` after cutting any wiring.".dimmed()
        );
    } else {
        println!("\n  {} {} consumer(s):", "BLOCKED —".red().bold(), importers.len() + users.len());
        if !importers.is_empty() {
            println!("  {} ({})", "imported by".yellow(), importers.len());
            for f in &importers {
                println!("    • {}", f);
            }
        }
        if !users.is_empty() {
            println!("  {} ({})", "entities used by".yellow(), users.len());
            for u in &users {
                println!("    • {}", u);
            }
        }
        for f in &declared_in {
            println!("  {} {}", "declared in".dimmed(), f);
        }
        println!(
            "\n  {}",
            "Sever these references (or the spawn/route wiring) before deleting.".dimmed()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_parse_out_of_a_fenced_reply() {
        let reply = "Sure!\n```json\n[{\"source\":\"A\",\"target\":\"B\",\
                     \"relation\":\"depends on\",\"confident\":true}]\n```";
        let got = parse_triples(reply);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].source, "A");
        assert_eq!(got[0].relation, "depends on");
        assert!(got[0].confident);
    }

    #[test]
    fn a_reply_with_no_array_yields_nothing() {
        assert!(parse_triples("I could not find any relationships.").is_empty());
        assert!(parse_triples("").is_empty());
        assert!(parse_triples("[not json]").is_empty());
    }

    #[test]
    fn triples_missing_a_side_are_dropped() {
        let got = parse_triples(r#"[{"source":"A"},{"source":"A","target":"  "},
                                    {"source":"A","target":"B"}]"#);
        assert_eq!(got.len(), 1, "only the complete triple survives");
        assert_eq!(got[0].relation, "related", "relation defaults");
        assert!(!got[0].confident, "confidence defaults to uncertain");
    }

    #[test]
    fn an_explicit_directory_argument_wins_over_detection() {
        let dir = tempfile::tempdir().unwrap();
        let got = resolve_root(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(got, dir.path());
    }

    #[test]
    fn a_missing_graph_names_the_file_and_the_fix() {
        let dir = tempfile::tempdir().unwrap();
        let err = load_graph(&dir.path().to_string_lossy()).unwrap_err().to_string();
        assert!(err.contains("graph.json"), "{err}");
        assert!(err.contains("hexa graph build"), "{err}");
    }
}
