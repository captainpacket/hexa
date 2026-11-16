//! Leases that follow the architecture (ADR-2611160750): what `hexa agent
//! register` and `hexa dev worktree merge-order` need from the layer map.
//!
//! - A lease may name a **layer slice** (`usecases/billing`,
//!   `adapters/secondary/postgres`). It resolves through the analyzer's layer
//!   map (`.hexa/project.json` declarations, then the built-in patterns) to
//!   the paths it covers, and those paths are what the ledger records.
//! - A lease whose files are in `domain/` or `ports/` is a **contract lease**.
//!   It is refused while a live lease covers a file that imports one of them.
//! - A use-case or adapter lease records the contract files it imports as
//!   **frozen**.
//!
//! Who imports what is read by a direct scan of the current files — Rust
//! `crate::` paths (including grouped `use crate::{…}`) and TS/JS relative
//! specifiers — not from `graph-out/graph.json`. That file carries no
//! freshness signal, so a contract check read from it could pass on a graph
//! built before the import existed. A scan of the files on disk cannot be
//! stale, and a file it cannot read is a refusal, never a skip.
//!
//! Not seen by the scan: imports across crates (`other_crate::ports::…`),
//! `super::` paths, and TS path aliases. Those consumers are missed, not
//! misreported.

use hexa_analysis::layer_classifier::LayerMap;
use hexa_analysis::ports::HexLayer;
use hexa_core::ports::ledger::{covers, Ledger, LedgerError};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One checkout's files and layer map.
pub struct Arch {
    root: PathBuf,
    files: Vec<String>,
    set: HashSet<String>,
    layers: LayerMap,
}

fn layers_unreadable(root: &Path, e: String) -> LedgerError {
    LedgerError::new(
        "layers_unreadable",
        format!("cannot read the layer map of {}: {e}", root.display()),
        "Fix `analyze.layers` in .hexa/project.json; a lease is not judged against a layer map that could not be read.".into(),
        None,
        false,
    )
}

fn scan_failed(path: &Path, e: std::io::Error) -> LedgerError {
    LedgerError::new(
        "lease_scan_failed",
        format!("cannot read {} to find what it imports: {e}", path.display()),
        "Fix the file's permissions, or release the lease that covers it; a contract check does not skip a file it cannot read.".into(),
        Some("lease"),
        true,
    )
}

/// The working tree's top level for `dir`, or `dir` outside a repository.
pub fn checkout_root(dir: &Path) -> PathBuf {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_else(|| dir.to_path_buf())
}

fn list_files(root: &Path) -> Vec<String> {
    let git = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
        .ok()
        .filter(|o| o.status.success());
    let mut files: Vec<String> = match git {
        Some(o) => String::from_utf8_lossy(&o.stdout)
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        None => {
            let mut out = Vec::new();
            walk(root, root, &mut out);
            out
        }
    };
    // `--cached` lists a tracked file deleted from the working tree.
    files.retain(|f| root.join(f).is_file());
    files.sort();
    files.dedup();
    files
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// The layer a slice's first segments name, and how many segments that took.
fn slice_layer(parts: &[&str]) -> Option<(Vec<HexLayer>, usize)> {
    use HexLayer::*;
    match parts {
        ["domain", ..] => Some((vec![Domain], 1)),
        ["ports", ..] => Some((vec![Ports], 1)),
        ["usecases", ..] => Some((vec![Usecases], 1)),
        ["adapters", "primary", ..] => Some((vec![AdaptersPrimary], 2)),
        ["adapters", "secondary", ..] => Some((vec![AdaptersSecondary], 2)),
        ["adapters", ..] => Some((vec![AdaptersPrimary, AdaptersSecondary], 1)),
        _ => None,
    }
}

pub fn is_contract(l: HexLayer) -> bool {
    matches!(l, HexLayer::Domain | HexLayer::Ports)
}

fn is_outer(l: HexLayer) -> bool {
    matches!(
        l,
        HexLayer::Usecases | HexLayer::AdaptersPrimary | HexLayer::AdaptersSecondary
    )
}

/// Where a finished task merges: inner layers first (ADR-2611160750 §4).
pub fn merge_rank(l: HexLayer) -> u8 {
    use HexLayer::*;
    match l {
        Domain => 0,
        Ports => 1,
        Usecases => 2,
        AdaptersPrimary | AdaptersSecondary | Infrastructure => 3,
        CompositionRoot | EntryPoint => 4,
        Unknown => 5,
    }
}

const SOURCE_EXTS: &[&str] = &["rs", "ts", "tsx", "mts", "js", "jsx", "mjs", "cjs"];
const TS_EXTS: &[&str] = &["ts", "tsx", "mts", "js", "jsx", "mjs", "cjs"];

impl Arch {
    pub fn load(dir: &Path) -> Result<Arch, LedgerError> {
        let root = checkout_root(dir);
        let layers = LayerMap::from_project(&root).map_err(|e| layers_unreadable(&root, e))?;
        let files = list_files(&root);
        let set = files.iter().cloned().collect();
        Ok(Arch {
            root,
            files,
            set,
            layers,
        })
    }

    /// The layer map alone, for a caller that classifies lease paths without
    /// reading files (`merge-order`).
    pub fn layer_map(dir: &Path) -> Result<LayerMap, LedgerError> {
        let root = checkout_root(dir);
        LayerMap::from_project(&root).map_err(|e| layers_unreadable(&root, e))
    }

    pub fn layer(&self, path: &str) -> HexLayer {
        self.layers.classify(path)
    }

    /// A normalized lease as the paths the ledger records. A path that exists,
    /// or whose first segment names no layer, is returned unchanged. A layer
    /// slice resolves to the shortest paths covering its files, and a slice
    /// that covers none is refused.
    pub fn resolve(&self, lease: &str) -> Result<Vec<String>, LedgerError> {
        let parts: Vec<&str> = lease.split('/').collect();
        let Some((want, _)) = slice_layer(&parts) else {
            return Ok(vec![lease.to_string()]);
        };
        if self.root.join(lease).exists() {
            return Ok(vec![lease.to_string()]);
        }
        let n = parts.len();
        let mut found: Vec<String> = Vec::new();
        for f in &self.files {
            if !want.contains(&self.layer(f)) {
                continue;
            }
            let comps: Vec<&str> = f.split('/').collect();
            let hit = (0..comps.len().saturating_sub(n - 1)).find(|&i| {
                (0..n).all(|k| {
                    let c = comps[i + k];
                    let last_of_file = k == n - 1 && i + k == comps.len() - 1;
                    c == parts[k]
                        || (last_of_file && c.rsplit_once('.').is_some_and(|(stem, _)| stem == parts[k]))
                })
            });
            if let Some(i) = hit {
                let prefix = comps[..i + n].join("/");
                if !found.contains(&prefix) {
                    found.push(prefix);
                }
            }
        }
        let covered: Vec<String> = found
            .iter()
            .filter(|p| !found.iter().any(|q| q != *p && covers(q, p)))
            .cloned()
            .collect();
        if covered.is_empty() {
            return Err(LedgerError::new(
                "lease_slice_empty",
                format!("lease `{lease}` names a layer slice, and no file in that layer matches it"),
                format!(
                    "Check the slice name against the tree (`hexa analyze .` lists the layers). If the task creates this path, lease its full path (such as `src/{lease}/`) or create the directory first: an existing path is always a plain lease."
                ),
                Some("lease"),
                false,
            ));
        }
        Ok(covered)
    }

    fn files_under<'a>(&'a self, lease: &'a str) -> impl Iterator<Item = &'a String> + 'a {
        self.files.iter().filter(move |f| covers(lease, f))
    }

    fn exists(&self, rel: &str) -> bool {
        self.set.contains(rel)
    }

    /// The repo files `file` imports, as far as the scan can resolve them.
    pub fn imports_of(&self, file: &str) -> Result<Vec<String>, LedgerError> {
        let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if !SOURCE_EXTS.contains(&ext) {
            return Ok(Vec::new());
        }
        let abs = self.root.join(file);
        let text = std::fs::read_to_string(&abs).map_err(|e| scan_failed(&abs, e))?;
        let mut out = Vec::new();
        if ext == "rs" {
            let Some(src) = self.crate_src(file) else { return Ok(out) };
            for path in rust_crate_paths(&strip_rust_comments(&text)) {
                if let Some(t) = self.resolve_rust(&src, &path) {
                    out.push(t);
                }
            }
        } else {
            let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            for spec in ts_relative_specs(&text) {
                if let Some(t) = self.resolve_ts(dir, &spec) {
                    out.push(t);
                }
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// `<crate>/src` for the nearest `Cargo.toml` above `file`.
    fn crate_src(&self, file: &str) -> Option<String> {
        let mut dir = file.rsplit_once('/').map(|(d, _)| d.to_string());
        while let Some(d) = dir {
            let manifest = if d.is_empty() { "Cargo.toml".to_string() } else { format!("{d}/Cargo.toml") };
            if self.exists(&manifest) {
                return Some(if d.is_empty() { "src".into() } else { format!("{d}/src") });
            }
            dir = if d.is_empty() { None } else { Some(d.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default()) };
        }
        None
    }

    /// The longest module prefix of `crate::a::b::C` that is a file.
    fn resolve_rust(&self, src: &str, path: &[String]) -> Option<String> {
        for k in (1..=path.len()).rev() {
            let m = path[..k].join("/");
            for cand in [format!("{src}/{m}.rs"), format!("{src}/{m}/mod.rs")] {
                if self.exists(&cand) {
                    return Some(cand);
                }
            }
        }
        None
    }

    fn resolve_ts(&self, dir: &str, spec: &str) -> Option<String> {
        let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
        for c in spec.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                c => parts.push(c),
            }
        }
        let base = parts.join("/");
        if self.exists(&base) && !base.ends_with(".js") {
            return Some(base);
        }
        let stem = ["js", "jsx", "mjs", "cjs"]
            .iter()
            .find_map(|e| base.strip_suffix(&format!(".{e}")))
            .unwrap_or(&base);
        let mut cands: Vec<String> = TS_EXTS.iter().map(|e| format!("{stem}.{e}")).collect();
        cands.extend(TS_EXTS.iter().map(|e| format!("{base}/index.{e}")));
        cands.into_iter().find(|c| self.exists(c))
    }

    /// The contract files the outer-layer files under `leases` import, minus
    /// what the leases cover themselves.
    pub fn frozen_for(&self, leases: &[String]) -> Result<Vec<String>, LedgerError> {
        let mut out = BTreeSet::new();
        for l in leases {
            for f in self.files_under(l) {
                if !is_outer(self.layer(f)) {
                    continue;
                }
                for t in self.imports_of(f)? {
                    if is_contract(self.layer(&t)) && !leases.iter().any(|x| covers(x, &t)) {
                        out.insert(t);
                    }
                }
            }
        }
        Ok(out.into_iter().collect())
    }

    /// Does `lease` hold contract files (or name a contract path that does not exist yet)?
    fn is_contract_lease(&self, lease: &str) -> bool {
        is_contract(self.layer(lease)) || self.files_under(lease).any(|f| is_contract(self.layer(f)))
    }
}

/// Refuse a contract lease that a live lease's files import, and a lease
/// whose frozen contracts a live lease holds. `here` is the caller's
/// checkout; a live task is scanned in its own worktree when that exists.
pub fn check_contracts(
    here: &Arch,
    ledger: &Ledger,
    leases: &[String],
    frozen: &[String],
) -> Result<(), LedgerError> {
    for f in frozen {
        for t in ledger.active() {
            if let Some(h) = t.leases.iter().find(|h| covers(h, f)) {
                return Err(LedgerError::new(
                    "lease_contract_held",
                    format!(
                        "this lease imports `{f}`, and task {} ({}, {}) holds `{h}`: its contract may change under you",
                        t.id, t.name, t.status
                    ),
                    format!(
                        "Wait until task {} reports or is released, then register again; or agree the contract change with it first.",
                        t.id
                    ),
                    Some("lease"),
                    true,
                ));
            }
        }
    }
    let contracts: Vec<&String> = leases.iter().filter(|l| here.is_contract_lease(l)).collect();
    if contracts.is_empty() {
        return Ok(());
    }
    let mut archs: HashMap<PathBuf, Arch> = HashMap::new();
    for t in ledger.active() {
        let scanned = match t.worktree.as_deref().map(Path::new).filter(|w| w.is_dir()) {
            Some(w) => {
                let key = checkout_root(w);
                if key != here.root && !archs.contains_key(&key) {
                    archs.insert(key.clone(), Arch::load(w)?);
                }
                if key == here.root { here } else { &archs[&key] }
            }
            None => here,
        };
        let refuse = |consumer: &str, contract: &str, held: &str| {
            LedgerError::new(
                "lease_contract_in_use",
                format!(
                    "lease `{held}` is a contract: `{consumer}`, under task {} ({}, {}), imports `{contract}`",
                    t.id, t.name, t.status
                ),
                format!(
                    "Wait until task {} reports or is released, or agree the change with it first; a port or domain file changes while nobody leases code that uses it.",
                    t.id
                ),
                Some("lease"),
                true,
            )
        };
        for l in &t.leases {
            for c in scanned.files_under(l) {
                for x in scanned.imports_of(c)? {
                    if let Some(held) = contracts.iter().find(|h| covers(h, &x)) {
                        if is_contract(scanned.layer(&x)) {
                            return Err(refuse(c, &x, held));
                        }
                    }
                }
            }
        }
        for x in &t.frozen {
            if let Some(held) = contracts.iter().find(|h| covers(h, x)) {
                return Err(refuse("its frozen contracts", x, held));
            }
        }
    }
    Ok(())
}

// ── import scanning ──────────────────────────────────────────────────────────

/// Drop `//` line comments and `/* */` block comments; strings are kept (a
/// `crate::` inside a string literal is a rare false consumer, never a miss).
fn strip_rust_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("//") {
            rest = r.find('\n').map_or("", |i| &r[i..]);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.find("*/").map_or("", |i| &r[i + 2..]);
            out.push(' ');
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Every `crate::a::b` path in `text`, with `use crate::{a::b, c}` groups
/// expanded. `self` segments are dropped; a glob ends the path.
fn rust_crate_paths(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find("crate::") {
        let at = from + i;
        let ident_before = text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        from = at + "crate::".len();
        if !ident_before {
            parse_tree(&text[from..], Vec::new(), &mut out);
        }
    }
    out
}

fn parse_tree(s: &str, mut prefix: Vec<String>, out: &mut Vec<Vec<String>>) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    };
    loop {
        skip_ws(&mut i);
        if i < b.len() && b[i] == b'{' {
            i += 1;
            loop {
                skip_ws(&mut i);
                if i >= b.len() {
                    return i;
                }
                match b[i] {
                    b'}' => return i + 1,
                    b',' => i += 1,
                    _ => {
                        let n = parse_tree(&s[i..], prefix.clone(), out);
                        i += n.max(1);
                    }
                }
            }
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let ident = &s[start..i];
        if !ident.is_empty() && ident != "self" {
            prefix.push(ident.to_string());
        }
        skip_ws(&mut i);
        if !ident.is_empty() && s[i..].starts_with("::") {
            i += 2;
            continue;
        }
        if !prefix.is_empty() {
            out.push(prefix);
        }
        return i;
    }
}

/// Relative specifiers in `import … from '…'`, `import '…'`, `export … from '…'`,
/// `import('…')` and `require('…')`.
fn ts_relative_specs(text: &str) -> Vec<String> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"(?:\bfrom|\bimport|\brequire)\s*\(?\s*['"](\.{1,2}/[^'"]+)['"]"#)
            .expect("static regex")
    });
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<String> {
        rust_crate_paths(text).into_iter().map(|p| p.join("::")).collect()
    }

    #[test]
    fn rust_paths_expand_groups_drop_self_and_ignore_other_crates() {
        assert_eq!(paths("use crate::ports::billing::Billing;"), vec!["ports::billing::Billing"]);
        assert_eq!(
            paths("use crate::{ports::billing::{self, Billing}, domain::invoice::Invoice};"),
            vec!["ports::billing", "ports::billing::Billing", "domain::invoice::Invoice"]
        );
        assert!(paths("use mycrate::ports::x; use other_crate::y;").is_empty());
        assert_eq!(paths("fn f() { crate::domain::x::run() }"), vec!["domain::x::run"]);
        assert!(paths(&strip_rust_comments("// use crate::ports::x;\n/* crate::domain::y */")).is_empty());
    }

    #[test]
    fn ts_specs_are_the_relative_ones() {
        let t = "import { A } from '../ports/a.js';\nimport type B from \"./b\";\nimport 'x';\nexport * from './c.js';\nconst d = require('../d');\nconst e = await import('./e.js');";
        assert_eq!(ts_relative_specs(t), vec!["../ports/a.js", "./b", "./c.js", "../d", "./e.js"]);
    }

    #[test]
    fn merge_rank_runs_inner_to_outer() {
        use HexLayer::*;
        let order = [Domain, Ports, Usecases, AdaptersSecondary, CompositionRoot, Unknown];
        assert!(order.windows(2).all(|w| merge_rank(w[0]) < merge_rank(w[1])));
        assert_eq!(merge_rank(AdaptersPrimary), merge_rank(AdaptersSecondary));
    }
}
