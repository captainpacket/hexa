//! Whether a native build script is keyed the same under every cargo command
//! (ADR-2611151330).
//!
//! A build script reruns when its own build changes, and a native one (it
//! depends on `cc` or `cmake`) compiles C or C++ every time it reruns. Cargo's
//! unit graph lists each unit's features and profile for one command. Two
//! commands that give a script's dependency tree different features or
//! settings give it different cache keys, so the script is rebuilt and rerun
//! when the commands alternate. Everything here is a pure function of the
//! graphs, so the rules are tested without cargo.

use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The most differing units a finding lists.
const MAX_DIFFERENCES: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Difference {
    pub unit: String,
    /// `features`, `profile` or `presence`.
    pub kind: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Script {
    pub package: String,
    pub stable: bool,
    /// How many of the selections contain this script.
    pub selections: usize,
    /// Selections that share a key; one group means stable.
    pub groups: Vec<Vec<String>>,
    pub differences: Vec<Difference>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub selections: usize,
    pub native_scripts: usize,
    pub unstable: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub stable: bool,
    pub summary: Summary,
    pub scripts: Vec<Script>,
    pub notes: Vec<String>,
}

/// `name@version` from a unit graph package id, in any of the forms cargo has used:
/// `registry+...#name@1.0.0`, `path+file:///x/name#0.1.0`, `name 1.0.0 (source)`.
pub fn label(pkg_id: &str) -> String {
    if let Some((head, frag)) = pkg_id.rsplit_once('#') {
        if frag.contains('@') {
            return frag.to_string();
        }
        let name = head.trim_end_matches('/').rsplit('/').next().unwrap_or(head);
        return format!("{name}@{frag}");
    }
    let mut parts = pkg_id.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(n), Some(v)) => format!("{n}@{v}"),
        _ => pkg_id.to_string(),
    }
}

fn package_name(label: &str) -> &str {
    label.split('@').next().unwrap_or(label)
}

/// One unit of a script's dependency tree, without the profile's name: cargo
/// shares artifacts between `dev` and `test` units whose settings are equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UnitKey {
    package: String,
    target: String,
    mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnitVal {
    features: Vec<String>,
    /// Profile settings as `field -> value`, the name left out.
    profile: BTreeMap<String, String>,
}

type Closure = BTreeMap<UnitKey, UnitVal>;

/// Every native build script in one graph, with its dependency tree.
fn scripts_of(graph: &Value) -> BTreeMap<String, Closure> {
    let mut out = BTreeMap::new();
    let Some(units) = graph.get("units").and_then(Value::as_array) else {
        return out;
    };
    let kind_of = |u: &Value| u["target"]["kind"][0].as_str().unwrap_or("").to_string();
    for (start, u) in units.iter().enumerate() {
        if kind_of(u) != "custom-build" || u["mode"].as_str() != Some("build") {
            continue;
        }
        // The script's tree: every unit reachable through `dependencies`.
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            if !seen.insert(i) {
                continue;
            }
            if let Some(deps) = units[i]["dependencies"].as_array() {
                stack.extend(deps.iter().filter_map(|d| d["index"].as_u64()).map(|i| i as usize));
            }
        }
        let native = seen.iter().any(|&i| {
            let l = label(units[i]["pkg_id"].as_str().unwrap_or(""));
            matches!(package_name(&l), "cc" | "cmake")
        });
        if !native {
            continue;
        }
        let mut closure = Closure::new();
        for &i in &seen {
            let v = &units[i];
            let mut features: Vec<String> = v["features"]
                .as_array()
                .map(|a| a.iter().filter_map(|f| f.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            features.sort();
            let mut profile = BTreeMap::new();
            if let Some(p) = v["profile"].as_object() {
                for (k, val) in p {
                    if k != "name" {
                        profile.insert(k.clone(), val.to_string());
                    }
                }
            }
            closure.insert(
                UnitKey {
                    package: label(v["pkg_id"].as_str().unwrap_or("")),
                    target: v["target"]["name"].as_str().unwrap_or("").to_string(),
                    mode: v["mode"].as_str().unwrap_or("").to_string(),
                },
                UnitVal { features, profile },
            );
        }
        out.insert(label(u["pkg_id"].as_str().unwrap_or("")), closure);
    }
    out
}

fn show_features(f: &[String]) -> String {
    format!("[{}]", f.join(", "))
}

/// How one unit differs across the distinct keys of a script.
fn difference(key: &UnitKey, vals: &[Option<&UnitVal>]) -> Option<Difference> {
    let distinct: BTreeSet<String> = vals
        .iter()
        .map(|v| v.map_or("absent".to_string(), |v| format!("{:?}|{:?}", v.features, v.profile)))
        .collect();
    if distinct.len() < 2 {
        return None;
    }
    let mut unit = if key.target.is_empty() || key.target == package_name(&key.package) {
        key.package.clone()
    } else {
        format!("{} ({})", key.package, key.target)
    };
    if key.mode != "build" {
        unit = format!("{unit} [{}]", key.mode);
    }
    if vals.iter().any(Option::is_none) {
        let n = vals.iter().filter(|v| v.is_some()).count();
        return Some(Difference {
            unit,
            kind: "presence",
            detail: format!("in {n} of {} keys", vals.len()),
        });
    }
    let vals: Vec<&UnitVal> = vals.iter().flatten().copied().collect();
    if vals.iter().any(|v| v.features != vals[0].features) {
        // Only the features some keys have and others lack: a full set (syn has ten) hides the difference.
        let all: BTreeSet<&String> = vals.iter().flat_map(|v| v.features.iter()).collect();
        let partial: Vec<String> = all
            .into_iter()
            .filter(|f| !vals.iter().all(|v| v.features.contains(*f)))
            .cloned()
            .collect();
        return Some(Difference { unit, kind: "features", detail: format!("not in every key: {}", show_features(&partial)) });
    }
    let fields: BTreeSet<&String> = vals.iter().flat_map(|v| v.profile.keys()).collect();
    let changed: Vec<String> = fields
        .into_iter()
        .filter(|f| vals.iter().any(|v| v.profile.get(*f) != vals[0].profile.get(*f)))
        .map(|f| {
            let seen: Vec<String> = vals.iter().map(|v| v.profile.get(f).cloned().unwrap_or_default()).collect();
            format!("{f} {}", seen.join(" vs "))
        })
        .collect();
    Some(Difference { unit, kind: "profile", detail: changed.join("; ") })
}

fn hint_for(diffs: &[Difference]) -> Option<String> {
    let has = |k: &str| diffs.iter().any(|d| d.kind == k);
    let mut parts = Vec::new();
    if has("profile") {
        parts.push(
            "a crate is built host-only by one command and shared with the target by another: make the build-override \
             agree with the profile, for example `[profile.dev.build-override] debug = true`"
                .to_string(),
        );
    }
    if has("features") || has("presence") {
        parts.push(
            "build-dependency features unify per command: pin the unified set where every selection sees it, as a \
             build-dependency of a crate every selection includes (what a cargo-hakari `workspace-hack` crate does)"
                .to_string(),
        );
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("{}. This is a prediction from the unit graph, not a measured rebuild.", parts.join("; and ")))
}

/// Compare the unit graphs of several cargo selections. `selections` is
/// `(cargo arguments, unit graph)`.
pub fn analyze(selections: &[(String, Value)]) -> Report {
    let per_selection: Vec<(&String, BTreeMap<String, Closure>)> =
        selections.iter().map(|(name, g)| (name, scripts_of(g))).collect();
    let all_scripts: BTreeSet<&String> = per_selection.iter().flat_map(|(_, s)| s.keys()).collect();

    let mut scripts = Vec::new();
    for pkg in all_scripts {
        // Group the selections that contain this script by its key, in first-seen order.
        let mut groups: Vec<(&Closure, Vec<String>)> = Vec::new();
        for (name, found) in &per_selection {
            let Some(closure) = found.get(pkg) else { continue };
            match groups.iter_mut().find(|(c, _)| *c == closure) {
                Some((_, names)) => names.push((*name).clone()),
                None => groups.push((closure, vec![(*name).clone()])),
            }
        }
        let count: usize = groups.iter().map(|(_, n)| n.len()).sum();

        let mut differences = Vec::new();
        if groups.len() > 1 {
            let keys: BTreeSet<&UnitKey> = groups.iter().flat_map(|(c, _)| c.keys()).collect();
            for k in keys {
                let vals: Vec<Option<&UnitVal>> = groups.iter().map(|(c, _)| c.get(k)).collect();
                differences.extend(difference(k, &vals));
            }
            // Features and presence first: they are the cause a profile setting cannot fix.
            differences.sort_by_key(|d| (d.kind == "profile", d.unit.clone()));
            differences.truncate(MAX_DIFFERENCES);
        }
        let hint = hint_for(&differences);
        scripts.push(Script {
            package: pkg.clone(),
            stable: groups.len() <= 1,
            selections: count,
            groups: groups.into_iter().map(|(_, n)| n).collect(),
            differences,
            hint,
        });
    }

    let unstable = scripts.iter().filter(|s| !s.stable).count();
    let mut notes = Vec::new();
    if scripts.is_empty() {
        notes.push("no native build scripts found (no build script depends on cc or cmake); nothing to protect".to_string());
    }
    for s in scripts.iter().filter(|s| s.selections < 2) {
        notes.push(format!("{} appears in only one selection, so nothing was compared for it", s.package));
    }
    Report {
        stable: unstable == 0,
        summary: Summary { selections: selections.len(), native_scripts: scripts.len(), unstable },
        scripts,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(name: &str, debuginfo: u64) -> Value {
        json!({"name": name, "opt_level": "0", "lto": "false", "debuginfo": debuginfo, "debug_assertions": true,
               "overflow_checks": true, "incremental": false, "panic": "unwind"})
    }

    /// A graph with one native script `sys` whose tree is `cc` and `shared`.
    fn graph(profile_name: &str, shared_debuginfo: u64, shared_features: &[&str], with_cc: bool) -> Value {
        let mut units = vec![
            json!({"pkg_id": "path+file:///w/sys#0.1.0", "target": {"name": "build-script-build", "kind": ["custom-build"]},
                   "mode": "build", "features": [], "profile": profile("dev", 0),
                   "dependencies": [{"index": 1}, {"index": 2}]}),
            json!({"pkg_id": "path+file:///w/shared#0.1.0", "target": {"name": "shared", "kind": ["lib"]}, "mode": "build",
                   "features": shared_features, "profile": profile(profile_name, shared_debuginfo), "dependencies": []}),
        ];
        units.push(if with_cc {
            json!({"pkg_id": "registry+https://x#cc@1.11.1", "target": {"name": "cc", "kind": ["lib"]}, "mode": "build",
                   "features": [], "profile": profile("dev", 0), "dependencies": []})
        } else {
            json!({"pkg_id": "registry+https://x#log@0.4.0", "target": {"name": "log", "kind": ["lib"]}, "mode": "build",
                   "features": [], "profile": profile("dev", 0), "dependencies": []})
        });
        json!({"version": 1, "units": units, "roots": [0]})
    }

    fn sel(name: &str, g: Value) -> (String, Value) {
        (name.to_string(), g)
    }

    #[test]
    fn labels_read_every_package_id_form_cargo_has_used() {
        assert_eq!(label("registry+https://github.com/rust-lang/crates.io-index#libduckdb-sys@1.20000.1"), "libduckdb-sys@1.20000.1");
        assert_eq!(label("path+file:///w/native-sys#0.1.0"), "native-sys@0.1.0");
        assert_eq!(label("path+file:///w/shared#shared@0.2.0"), "shared@0.2.0");
        assert_eq!(label("cc 1.11.1 (registry+https://x)"), "cc@1.11.1");
    }

    #[test]
    fn identical_trees_are_one_key() {
        let r = analyze(&[sel("a", graph("dev", 2, &[], true)), sel("b", graph("dev", 2, &[], true))]);
        assert!(r.stable);
        assert_eq!((r.summary.native_scripts, r.summary.unstable), (1, 0));
        assert_eq!(r.scripts[0].groups, vec![vec!["a".to_string(), "b".to_string()]]);
    }

    #[test]
    fn a_different_debuginfo_is_a_profile_difference() {
        let r = analyze(&[sel("-p sys", graph("dev", 0, &[], true)), sel("-p app", graph("dev", 2, &[], true))]);
        assert!(!r.stable);
        let s = &r.scripts[0];
        assert_eq!(s.groups.len(), 2);
        assert_eq!(s.differences.len(), 1);
        assert_eq!((s.differences[0].unit.as_str(), s.differences[0].kind), ("shared@0.1.0", "profile"));
        assert!(s.differences[0].detail.starts_with("debuginfo 0 vs 2"), "{}", s.differences[0].detail);
        assert!(s.hint.as_deref().unwrap().contains("build-override"));
    }

    #[test]
    fn different_features_are_a_features_difference() {
        let r = analyze(&[sel("a", graph("dev", 0, &[], true)), sel("b", graph("dev", 0, &["extra"], true))]);
        let d = &r.scripts[0].differences[0];
        assert_eq!((d.kind, d.detail.as_str()), ("features", "not in every key: [extra]"));
        assert!(r.scripts[0].hint.as_deref().unwrap().contains("workspace-hack"));
    }

    #[test]
    fn the_profile_name_is_not_a_difference() {
        let r = analyze(&[sel("build", graph("dev", 2, &[], true)), sel("test", graph("test", 2, &[], true))]);
        assert!(r.stable, "dev and test with equal settings share artifacts in cargo");
    }

    #[test]
    fn a_unit_in_one_tree_only_is_presence() {
        let mut g = graph("dev", 0, &[], true);
        g["units"].as_array_mut().unwrap().push(json!({"pkg_id": "registry+https://x#extra@1.0.0",
            "target": {"name": "extra", "kind": ["lib"]}, "mode": "build", "features": [],
            "profile": profile("dev", 0), "dependencies": []}));
        g["units"][0]["dependencies"].as_array_mut().unwrap().push(json!({"index": 3}));
        let r = analyze(&[sel("a", graph("dev", 0, &[], true)), sel("b", g)]);
        assert_eq!((r.scripts[0].differences[0].kind, r.scripts[0].differences[0].unit.as_str()), ("presence", "extra@1.0.0"));
    }

    #[test]
    fn a_unit_built_in_another_mode_says_so_in_its_name() {
        let k = UnitKey { package: "libc@0.2.191".into(), target: "build-script-build".into(), mode: "run-custom-build".into() };
        let a = UnitVal { features: vec![], profile: BTreeMap::new() };
        let mut b = a.clone();
        b.features = vec!["std".into()];
        let d = difference(&k, &[Some(&a), Some(&b)]).unwrap();
        assert_eq!(d.unit, "libc@0.2.191 (build-script-build) [run-custom-build]");
    }

    #[test]
    fn a_script_without_cc_or_cmake_is_not_native() {
        let r = analyze(&[sel("a", graph("dev", 0, &[], false)), sel("b", graph("dev", 2, &[], false))]);
        assert_eq!(r.summary.native_scripts, 0);
        assert!(r.stable);
        assert!(r.notes[0].contains("no native build scripts"));
    }

    #[test]
    fn a_script_in_one_selection_only_cannot_be_compared_and_says_so() {
        let empty = json!({"version": 1, "units": [], "roots": []});
        let r = analyze(&[sel("a", graph("dev", 0, &[], true)), sel("b", empty)]);
        assert!(r.stable);
        assert!(r.notes.iter().any(|n| n.contains("only one selection")), "{:?}", r.notes);
    }

    fn add_dep(g: &mut Value, i: usize, debuginfo: u64) {
        let idx = g["units"].as_array().unwrap().len();
        g["units"].as_array_mut().unwrap().push(json!({"pkg_id": format!("registry+https://x#dep{i}@1.0.0"),
            "target": {"name": format!("dep{i}"), "kind": ["lib"]}, "mode": "build", "features": [],
            "profile": profile("dev", debuginfo), "dependencies": []}));
        g["units"][0]["dependencies"].as_array_mut().unwrap().push(json!({"index": idx}));
    }

    #[test]
    fn differences_are_capped_and_features_come_before_profile() {
        let mut a = graph("dev", 0, &[], true);
        let mut b = graph("dev", 2, &["extra"], true);
        for i in 0..20 {
            add_dep(&mut a, i, 0);
            add_dep(&mut b, i, 2);
        }
        let r = analyze(&[sel("a", a), sel("b", b)]);
        let d = &r.scripts[0].differences;
        assert_eq!(d.len(), MAX_DIFFERENCES);
        assert_eq!(d[0].kind, "features", "features sort before profile");
    }
}
