//! Running a scenario's test or a gate, here or through a project's remote
//! template (ADR-2611191120).
//!
//! The rules (which runner, the template, the batch markers, the judges) are
//! the domain's, reached through `hexa_core::ports::test_runner`. This module
//! reads the project's setting, builds each command, runs it, and refuses a
//! remote run that could not describe the commit concerned. A remote run that
//! does not prove itself is a runner failure: never a pass, never red, and
//! never retried here (CLAUDE.md: a gate that degrades silently is worse than
//! no gate).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hexa_core::ports::test_runner::{
    batch_script, check_remote_template, fill, judge_counted, judge_script, judge_vitest, parse_test_ref, remote_command,
    runner_for, runner_ran, shell_quote, split_batch, vitest_pattern, Runner, Verdict,
};

/// What a skip site prints when `HEXA_NO_SKIP` forbids the skip.
pub const NOT_PROVEN_HERE: &str = "not proven here:";

/// One scenario's test, run alone.
pub enum TestRun {
    Passed,
    /// The test ran and failed.
    Failed(String),
    /// It never ran: the build failed (`compile` is true) or cargo did not.
    Broken { compile: bool, tail: String },
    NotFound(String),
    Vacuous,
    Ambiguous(u64),
    NotProvenHere(String),
    /// The remote runner proved nothing: unreachable, it did not name the
    /// commit, or a marker was lost (ADR-2611191120 §5). Never a pass, never red.
    RunnerFailed(String),
}

/// The cargo arguments that run `f` in `file` and nothing else: an
/// integration test by its target, a library or binary test by its module
/// path in the target that compiles it. A module below `src/` may be compiled
/// by the library or by `main.rs`, so it is run in each one the crate has.
/// The manifest is named from `root`, so the command runs the same in any
/// checkout of the repository, here or on a remote runner (ADR-2611191120 §4).
fn cargo_args(root: &Path, krate: &Path, file: &Path, f: &str) -> Option<Vec<String>> {
    let rel = file.strip_prefix(krate).ok()?;
    let manifest = krate.strip_prefix(root).ok()?.join("Cargo.toml").display().to_string();
    let mut parts: Vec<String> = rel.with_extension("").iter().map(|s| s.to_string_lossy().to_string()).collect();
    let (target, name) = match parts.first().map(String::as_str) {
        Some("tests") if parts.len() == 2 => (vec!["--test".to_string(), parts[1].clone()], f.to_string()),
        Some("src") if parts.get(1).map(String::as_str) == Some("bin") && parts.len() > 2 => {
            // src/bin/x.rs, or src/bin/x/main.rs and the modules beside it.
            let bin = parts[2].clone();
            let mut path: Vec<String> = parts.split_off(3);
            if matches!(path.last().map(String::as_str), Some("main" | "mod")) {
                path.pop();
            }
            path.push(f.to_string());
            (vec!["--bin".to_string(), bin], path.join("::"))
        }
        Some("src") => {
            parts.remove(0);
            let root = parts.len() == 1;
            let target = match parts.last().map(String::as_str) {
                Some("main") if root => vec!["--bins".to_string()],
                Some("lib") if root => vec!["--lib".to_string()],
                _ => {
                    let src = krate.join("src");
                    let has_bin = src.join("main.rs").is_file() || src.join("bin").is_dir();
                    let mut t = Vec::new();
                    if src.join("lib.rs").is_file() || !has_bin {
                        t.push("--lib".to_string());
                    }
                    if has_bin {
                        t.push("--bins".to_string());
                    }
                    t
                }
            };
            if matches!(parts.last().map(String::as_str), Some("lib" | "mod" | "main")) {
                parts.pop();
            }
            parts.push(f.to_string());
            (target, parts.join("::"))
        }
        _ => return None,
    };
    let mut args = vec!["test".to_string(), "--manifest-path".to_string(), manifest];
    args.extend(target);
    // A module ref (`…::`) selects every test under it; a fn ref exactly one.
    let module = name.ends_with("::");
    args.extend(["--".to_string(), name]);
    if !module {
        args.push("--exact".to_string());
    }
    Some(args)
}


/// `.hexa/project.json` → `test` (§2, §3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestConfig {
    /// Extension (no dot) → command template with `{file}` and `{name}`.
    pub runners: BTreeMap<String, String>,
    /// The remote template with `{commit}` and `{cmd}`, when tests run elsewhere.
    pub remote: Option<String>,
}

/// The project's test setting. A `test.remote` that lacks a placeholder, or a
/// `test` section of the wrong shape, is an error, never ignored.
pub fn test_config(root: &Path) -> Result<TestConfig, String> {
    let path = root.join(".hexa/project.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(TestConfig::default()) };
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: not JSON ({e}), so its test setting cannot be read", path.display()))?;
    let Some(test) = v.get("test").filter(|t| !t.is_null()) else { return Ok(TestConfig::default()) };
    let mut cfg = TestConfig::default();
    if let Some(r) = test.get("runners").filter(|r| !r.is_null()) {
        let obj = r.as_object().ok_or("test.runners in .hexa/project.json must map an extension to a command template")?;
        for (ext, t) in obj {
            let t = t.as_str().ok_or_else(|| format!("test.runners.{ext} must be a command template string"))?;
            cfg.runners.insert(ext.trim_start_matches('.').to_string(), t.to_string());
        }
    }
    if let Some(r) = test.get("remote").filter(|r| !r.is_null()) {
        let t = r.as_str().ok_or("test.remote in .hexa/project.json must be a command template string")?;
        check_remote_template(t)?;
        cfg.remote = Some(t.to_string());
    }
    Ok(cfg)
}

/// Where a run goes.
#[derive(Debug, Clone, Copy)]
pub enum Where<'a> {
    /// In the checkout given, on this machine.
    Here,
    /// Through the remote template, for this full commit id.
    Remote { template: &'a str, commit: &'a str },
}

/// How one test's output is judged.
#[derive(Debug, Clone)]
enum Judge {
    Cargo { module: bool },
    Vitest(String),
    Script,
    Counted,
}

/// A test, ready to run from the root of a checkout.
#[derive(Debug, Clone)]
pub struct Planned {
    pub cmd: String,
    judge: Judge,
}

/// The environment a test runs under. `path_env` and `target_dir` are for a
/// run here; a remote runner has its own PATH and target.
pub fn env_prefix(path_env: Option<&str>, target_dir: Option<&Path>) -> String {
    let path = path_env.map(|p| format!(" PATH={p:?}")).unwrap_or_default();
    let target = target_dir.map(|t| format!(" CARGO_TARGET_DIR={:?}", t.display().to_string())).unwrap_or_default();
    // CARGO_TERM_COLOR=never: this output is parsed, and an inherited
    // `always` (CI sets it) wraps the lines read here in escape codes.
    format!("export{path} HEXA_NO_SKIP=1 CARGO_TERM_COLOR=never{target}; ")
}

/// The nearest directory at or above `file`, inside `root`, holding `marker`.
fn nearest(root: &Path, file: &Path, marker: &str) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    loop {
        if dir.join(marker).exists() {
            return Some(dir.to_path_buf());
        }
        if dir == root {
            return None;
        }
        dir = dir.parent()?;
    }
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.display().to_string())
}

/// Plan the test `r` in the checkout at `root` (§1, §2). `prefix` is
/// [`env_prefix`]. A ref that names no runnable test is its `TestRun`.
pub fn plan_test(root: &Path, r: &str, cfg: &TestConfig, prefix: &str) -> Result<Planned, TestRun> {
    let Some(t) = parse_test_ref(r) else {
        return Err(TestRun::NotFound(format!(
            "{r} is not a test reference: `<path>.rs::<fn>`, `<path>.ts::<test name>`, `<path>.sh[::<arg>]`, or `<path>.<ext>::<name>` for a runner in test.runners"
        )));
    };
    let Some(runner) = runner_for(t.ext(), &cfg.runners) else {
        return Err(TestRun::NotFound(format!(
            "no runner for `.{}` ({r}): add one to test.runners in .hexa/project.json, e.g. \"{}\": \"<command with {{file}} and {{name}}>\"",
            t.ext(),
            t.ext()
        )));
    };
    let abs = root.join(&t.file);
    if !abs.is_file() {
        return Err(TestRun::NotFound(format!("{} not found", t.file)));
    }
    let name = t.name.clone().unwrap_or_default();
    match runner {
        Runner::Cargo => {
            let module = name.ends_with("::");
            let leaf = name.trim_end_matches("::").rsplit("::").next().unwrap_or(&name).to_string();
            let text = std::fs::read_to_string(&abs).unwrap_or_default();
            let declared = if module { format!("mod {leaf} ") } else { format!("fn {leaf}(") };
            if !text.contains(&declared) {
                return Err(TestRun::NotFound(format!("{leaf} not found in {}", t.file)));
            }
            let Some(args) = nearest(root, &abs, "Cargo.toml").and_then(|k| cargo_args(root, &k, &abs, &name))
            else {
                return Err(TestRun::NotFound(format!("{} is in no crate's tests/ or src/", t.file)));
            };
            let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
            Ok(Planned { cmd: format!("{prefix}cargo {}", quoted.join(" ")), judge: Judge::Cargo { module } })
        }
        Runner::Vitest => {
            let Some(pkg) = nearest(root, &abs, "package.json") else {
                return Err(TestRun::NotFound(format!("no package.json at or above {}: vitest runs from its package", t.file)));
            };
            let in_pkg = rel(&pkg, &abs);
            let cd = if pkg == root { String::new() } else { format!("cd {} && ", shell_quote(&rel(root, &pkg))) };
            let cmd = format!(
                "{prefix}{cd}npx vitest run {} -t {} --reporter=json",
                shell_quote(&in_pkg),
                shell_quote(&vitest_pattern(&name))
            );
            Ok(Planned { cmd, judge: Judge::Vitest(name) })
        }
        Runner::Bash => {
            let arg = t.name.as_deref().map(|a| format!(" {}", shell_quote(a))).unwrap_or_default();
            Ok(Planned { cmd: format!("{prefix}bash {}{arg}", shell_quote(&t.file)), judge: Judge::Script })
        }
        Runner::Configured(template) => {
            let cmd = fill(&template, &[("file", &t.file), ("name", &name)]);
            Ok(Planned { cmd: format!("{prefix}{cmd}"), judge: Judge::Counted })
        }
    }
}

fn tail(out: &str, n: usize) -> String {
    let lines: Vec<&str> = out.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n") + "\n"
}

/// What one run of a planned test established.
pub fn judge(p: &Planned, ok: bool, out: &str) -> TestRun {
    if let Some(line) = out.lines().find(|l| l.contains(NOT_PROVEN_HERE)) {
        let reason = line.split(NOT_PROVEN_HERE).nth(1).unwrap_or("").trim();
        return TestRun::NotProvenHere(reason.trim_matches(|c| c == '"' || c == '\'').to_string());
    }
    let verdict = match &p.judge {
        Judge::Cargo { module } => return judge_cargo(*module, ok, out),
        Judge::Vitest(name) => judge_vitest(out, name),
        Judge::Script => judge_script(ok, out),
        Judge::Counted => judge_counted(ok, out),
    };
    match verdict {
        Verdict::Passed => TestRun::Passed,
        Verdict::Failed(why) => TestRun::Failed(why),
        Verdict::Vacuous => TestRun::Vacuous,
        Verdict::Ambiguous(n) => TestRun::Ambiguous(n),
        Verdict::Unreadable(why) => TestRun::Broken { compile: false, tail: format!("{why}\n{}", tail(out, 10)) },
    }
}

fn judge_cargo(module: bool, ok: bool, out: &str) -> TestRun {
    if !ok {
        if out.contains("test result: FAILED") {
            return TestRun::Failed(tail(out, 10));
        }
        let compile = out.contains("could not compile") || out.contains("error[E");
        return TestRun::Broken { compile, tail: tail(out, 10) };
    }
    let passed: u64 = out
        .lines()
        .filter_map(|l| l.strip_prefix("test result: ok. "))
        .filter_map(|l| l.split(" passed").next()?.trim().parse::<u64>().ok())
        .sum();
    match passed {
        0 => TestRun::Vacuous,
        1 => TestRun::Passed,
        // A module scenario passes when every test in it ran and passed.
        _ if module => TestRun::Passed,
        n => TestRun::Ambiguous(n),
    }
}

fn nonce() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    format!("{:x}{:x}", t, std::process::id())
}

/// Run every plan in `dir` (§4). Here, one at a time; remote, all in one run
/// whose output must name the commit and carry every end marker (§5).
pub async fn run_plans(dir: &Path, at: Where<'_>, plans: &[Planned]) -> Vec<TestRun> {
    match at {
        Where::Here => {
            let mut out = Vec::with_capacity(plans.len());
            for p in plans {
                let (ok, text) = crate::direct_exec::run_evidence(&p.cmd, dir).await;
                out.push(judge(p, ok, &text));
            }
            out
        }
        Where::Remote { template, commit } => match run_remote(dir, template, commit, plans.iter().map(|p| p.cmd.clone()).collect()).await {
            Ok(segments) => plans.iter().zip(segments).map(|(p, (ok, text))| judge(p, ok, &text)).collect(),
            Err(why) => plans.iter().map(|_| TestRun::RunnerFailed(why.clone())).collect(),
        },
    }
}

/// Send `cmds` for `commit` through `template` in one run, from `dir`; each
/// command's (exit 0, output), or why the runner proved nothing (§4, §5).
pub async fn run_remote(dir: &Path, template: &str, commit: &str, cmds: Vec<String>) -> Result<Vec<(bool, String)>, String> {
    let n = nonce();
    let full = remote_command(template, commit, &batch_script(&n, &cmds))?;
    let (_, out) = crate::direct_exec::run_evidence(&full, dir).await;
    if !runner_ran(&out, commit) {
        return Err(format!(
            "runner failure: the remote runner's output does not name commit {commit}, so it is not known to have run it \
             (unreachable, or it checked out something else). Nothing was run here instead. Its last lines:\n{}",
            tail(&out, 8)
        ));
    }
    split_batch(&n, cmds.len(), &out).map_err(|e| format!("runner failure: {e}. Its last lines:\n{}", tail(&out, 8)))
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = std::process::Command::new("git").args(args).current_dir(dir).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The full id of `rev` in `dir`.
pub fn full_commit(dir: &Path, rev: &str) -> Option<String> {
    git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")])
}

/// Refuse a remote run that could not describe `commit` (§5): a commit on no
/// remote-tracking ref, or, when `head_tree`, a tree whose tracked files
/// differ from HEAD.
pub fn remote_preflight(dir: &Path, commit: &str, head_tree: bool) -> Result<(), String> {
    let short: String = commit.chars().take(12).collect();
    let pushed = git(dir, &["for-each-ref", "--contains", commit, "--count=1", "--format=%(refname)", "refs/remotes"])
        .is_some_and(|s| !s.is_empty());
    if !pushed {
        return Err(format!(
            "{short} is on no remote branch, so the remote runner (test.remote) cannot check it out: push it first \
             (`git push`), or run here with --local"
        ));
    }
    if head_tree {
        let dirty = git(dir, &["status", "--porcelain", "--untracked-files=no"]).unwrap_or_default();
        if !dirty.is_empty() {
            let files: Vec<&str> = dirty.lines().filter_map(|l| l.get(3..)).take(5).collect();
            return Err(format!(
                "tracked files differ from HEAD ({}): a remote run tests HEAD {short}, not this tree. Commit and push, \
                 or run here with --local",
                files.join(", ")
            ));
        }
    }
    Ok(())
}

/// Run a gate command for HEAD of `dir` (§4): here, or through the remote.
/// `Err` is a refusal or a runner failure, never a gate result.
pub async fn run_gate(dir: &Path, gate: &str, cfg: &TestConfig, local: bool) -> Result<(bool, String), String> {
    match cfg.remote.as_deref().filter(|_| !local) {
        None => Ok(crate::direct_exec::run_evidence(gate, dir).await),
        Some(template) => {
            let head = full_commit(dir, "HEAD").ok_or("HEAD is not a commit: there is nothing for the remote runner to check out")?;
            remote_preflight(dir, &head, true)?;
            println!("  sending the gate to the remote runner for {}", &head[..12]);
            let mut out = run_remote(dir, template, &head, vec![gate.to_string()]).await?;
            Ok(out.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_test_runs_by_its_module_path() {
        let k = Path::new("/w/hexa-cli");
        let a = cargo_args(k, k, &k.join("src/commands/adr/mod.rs"), "adr_gates::f").unwrap();
        assert!(a.ends_with(&["--lib".into(), "--".into(), "commands::adr::adr_gates::f".into(), "--exact".into()]), "{a:?}");
        let a = cargo_args(k, k, &k.join("tests/x.rs"), "f").unwrap();
        assert!(a.ends_with(&["--test".into(), "x".into(), "--".into(), "f".into(), "--exact".into()]), "{a:?}");
        assert!(cargo_args(k, k, &k.join("benches/b.rs"), "f").is_none());
    }

    #[test]
    fn a_binary_test_runs_in_its_binary_target() {
        let dir = tempfile::tempdir().unwrap();
        let k = dir.path();
        std::fs::create_dir_all(k.join("src/bin")).unwrap();
        std::fs::write(k.join("src/lib.rs"), "").unwrap();
        std::fs::write(k.join("src/main.rs"), "").unwrap();
        let tail = |a: Vec<String>, n: usize| a[a.len() - n..].to_vec();
        let a = cargo_args(k, k, &k.join("src/main.rs"), "f").unwrap();
        assert_eq!(tail(a, 4), ["--bins", "--", "f", "--exact"], "src/main.rs is the binary, not the library");
        let a = cargo_args(k, k, &k.join("src/bin/x.rs"), "f").unwrap();
        assert_eq!(tail(a, 5), ["--bin", "x", "--", "f", "--exact"], "not the bogus path bin::x::f");
        let a = cargo_args(k, k, &k.join("src/bin/x/util.rs"), "f").unwrap();
        assert_eq!(tail(a, 5), ["--bin", "x", "--", "util::f", "--exact"]);
        let a = cargo_args(k, k, &k.join("src/lib.rs"), "f").unwrap();
        assert_eq!(tail(a, 4), ["--lib", "--", "f", "--exact"]);
        let a = cargo_args(k, k, &k.join("src/cli/mod.rs"), "f").unwrap();
        assert_eq!(tail(a, 5), ["--lib", "--bins", "--", "cli::f", "--exact"], "a module main.rs may own");
        std::fs::remove_file(k.join("src/lib.rs")).unwrap();
        let a = cargo_args(k, k, &k.join("src/cli/mod.rs"), "f").unwrap();
        assert_eq!(tail(a, 4), ["--bins", "--", "cli::f", "--exact"], "no library target to ask for");
    }
}
