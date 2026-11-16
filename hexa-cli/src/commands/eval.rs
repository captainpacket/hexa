//! `hexa eval`: how well a model uses hexa, measured (ADR-2611160910).
//!
//! `hexa eval run` builds each scenario's repository in a temporary directory
//! (the fixture, `git init`, a commit, `hexa init` with the binary under test,
//! the scenario's setup), runs `claude -p` in it so the hooks and skills hexa
//! installed are in force, runs the outcome command and scores the transcript.
//! `--dry-run` builds the repositories and prints the command, spending
//! nothing; `--score` scores a saved transcript. `hexa eval report` prints the
//! newest result and how it moved from the one before.
//!
//! Nothing here writes to the repository it is run from, except
//! `.hexa/evals/`. A real run spends money and is never started by a test.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use hexa_exec::usage_eval::{self as ue, ExpectHit, Scenario, Score};

#[derive(Subcommand, Debug)]
pub enum EvalAction {
    /// Run scenarios against a model, build them only (--dry-run), or score a saved transcript (--score)
    Run(RunArgs),
    /// Print the newest result in .hexa/evals and how it moved from the one before
    Report(ReportArgs),
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// A scenario to run, by name or path (repeatable; default: every scenario)
    #[arg(long = "scenario", value_name = "NAME")]
    pub scenarios: Vec<String>,
    /// The model `claude -p` uses (default: claude's own default)
    #[arg(long)]
    pub model: Option<String>,
    /// Build each scenario's repository and print the command it would run; spends nothing
    #[arg(long)]
    pub dry_run: bool,
    /// Score this saved stream-json transcript against the one --scenario; runs no model
    #[arg(long, value_name = "TRANSCRIPT")]
    pub score: Option<PathBuf>,
    /// With --score: the outcome, when it was judged elsewhere
    #[arg(long, value_parser = ["pass", "fail"], requires = "score")]
    pub outcome: Option<String>,
    /// With --score: run the scenario's outcome command in this repository
    #[arg(long, value_name = "DIR", requires = "score", conflicts_with = "outcome")]
    pub repo: Option<PathBuf>,
    /// Where the scenarios live (default: evals/hexa-usage at the repository root)
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// The hexa binary under test (default: this one)
    #[arg(long, value_name = "PATH")]
    pub hexa: Option<PathBuf>,
    /// Keep each scenario's temporary repository, and print where it is
    #[arg(long)]
    pub keep: bool,
    /// With --score: print the score as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// The result file to print (default: the newest in .hexa/evals)
    pub file: Option<PathBuf>,
    /// The result file to compare against (default: the one before it)
    #[arg(long, value_name = "FILE")]
    pub against: Option<PathBuf>,
}

/// One scenario's row in a result file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub scenario: String,
    pub outcome: Option<bool>,
    pub process: f64,
    pub bypasses: u32,
    pub refusals: u32,
    pub turns: Option<u32>,
    pub cost_usd: Option<f64>,
    /// Why the run stopped: the result record's subtype, or what went wrong.
    pub stop: Option<String>,
    #[serde(default)]
    pub expected: Vec<ExpectHit>,
    #[serde(default)]
    pub bypass_calls: Vec<String>,
    /// The saved transcript, relative to the result file's directory.
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// `.hexa/evals/<timestamp>-<model>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFile {
    pub schema: u32,
    pub model: String,
    pub hexa: String,
    pub started: String,
    pub rows: Vec<Row>,
}

/// The longest one scenario may run before it is killed.
const WALL_CLOCK: Duration = Duration::from_secs(30 * 60);

/// What the forge clients see in a scenario: a token that authenticates
/// nothing, so a model that tries to open a real pull request cannot.
const DEAD_TOKEN: &str = "hexa-eval-no-forge-access";

pub async fn run(action: EvalAction) -> anyhow::Result<()> {
    match action {
        EvalAction::Run(a) => run_cmd(a),
        EvalAction::Report(a) => report(a),
    }
}

fn root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or(cwd)
}

fn evals_out(root: &Path) -> PathBuf {
    root.join(".hexa/evals")
}

/// A scenario by name under `dir`, or by path to its directory or its TOML.
fn resolve(dir: &Path, name: &str) -> anyhow::Result<(String, PathBuf)> {
    let as_path = PathBuf::from(name);
    let d = if as_path.join("scenario.toml").is_file() {
        as_path
    } else if as_path.is_file() && as_path.file_name().is_some_and(|f| f == "scenario.toml") {
        as_path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        dir.join(name)
    };
    if !d.join("scenario.toml").is_file() {
        bail!("no scenario {name:?}: {} has no scenario.toml", d.display());
    }
    let canon = d.canonicalize().unwrap_or(d.clone());
    let n = canon.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string());
    Ok((n, canon))
}

fn load(d: &Path) -> anyhow::Result<Scenario> {
    let p = d.join("scenario.toml");
    let text = std::fs::read_to_string(&p).with_context(|| p.display().to_string())?;
    ue::parse_scenario(&text).map_err(|e| anyhow!("{}: {e}", p.display()))
}

fn all_scenarios(dir: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| format!("no scenarios at {}", dir.display()))? {
        let p = e?.path();
        if p.join("scenario.toml").is_file() {
            out.push(resolve(dir, &p.to_string_lossy())?);
        }
    }
    out.sort();
    if out.is_empty() {
        bail!("no scenarios under {}", dir.display());
    }
    Ok(out)
}

fn run_cmd(a: RunArgs) -> anyhow::Result<()> {
    let root = root();
    let dir = a.dir.clone().unwrap_or_else(|| root.join("evals/hexa-usage"));
    if let Some(t) = &a.score {
        let [name] = a.scenarios.as_slice() else {
            bail!("--score scores one transcript against one --scenario");
        };
        return score_one(&dir, name, t, &a);
    }
    let picked = if a.scenarios.is_empty() {
        all_scenarios(&dir)?
    } else {
        a.scenarios.iter().map(|n| resolve(&dir, n)).collect::<anyhow::Result<_>>()?
    };
    let hexa = match &a.hexa {
        Some(p) => p.canonicalize().with_context(|| format!("--hexa {}", p.display()))?,
        None => std::env::current_exe()?,
    };
    if a.dry_run {
        dry_run(&picked, &hexa, &a)
    } else {
        real_run(&root, &picked, &hexa, &a)
    }
}

fn score_one(dir: &Path, name: &str, transcript: &Path, a: &RunArgs) -> anyhow::Result<()> {
    let (n, d) = resolve(dir, name)?;
    let s = load(&d)?;
    let text = std::fs::read_to_string(transcript).with_context(|| transcript.display().to_string())?;
    let outcome = match (&a.outcome, &a.repo) {
        (Some(o), _) => Some(o == "pass"),
        (None, Some(repo)) => Some(check_outcome(&s, repo, None)?.0),
        (None, None) => None,
    };
    let sc = ue::score_transcript(&s, &text, outcome).map_err(|e| anyhow!("{}: {e}", transcript.display()))?;
    let row = row_of(&n, &sc, None, None);
    if a.json {
        println!("{}", serde_json::to_string_pretty(&row)?);
    } else {
        print_table(std::slice::from_ref(&row), None);
        print_detail(&row);
    }
    Ok(())
}

fn row_of(name: &str, s: &Score, transcript: Option<String>, error: Option<String>) -> Row {
    Row {
        scenario: name.to_string(),
        outcome: s.outcome,
        process: s.process,
        bypasses: s.bypasses,
        refusals: s.refusals,
        turns: s.turns,
        cost_usd: s.cost_usd,
        stop: s.stop.clone(),
        expected: s.expected.clone(),
        bypass_calls: s.bypass_calls.clone(),
        transcript,
        error,
    }
}

// ── building a scenario repository ──────────────────────────────────────────

/// A scenario's temporary directory: `bin/hexa` (the binary under test),
/// `repo/` (the scenario repository), and room for setup's fake remote.
struct Sandbox {
    base: PathBuf,
    repo: PathBuf,
    path_env: String,
    keep: bool,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

impl Sandbox {
    /// Every command in the sandbox: the binary under test first on PATH,
    /// a fixed git identity, no forge credentials, and no hexa session of
    /// the caller's leaking in.
    fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.current_dir(&self.repo)
            .env("PATH", &self.path_env)
            .env("GIT_AUTHOR_NAME", "hexa-eval")
            .env("GIT_AUTHOR_EMAIL", "hexa-eval@localhost")
            .env("GIT_COMMITTER_NAME", "hexa-eval")
            .env("GIT_COMMITTER_EMAIL", "hexa-eval@localhost")
            .env("GH_TOKEN", DEAD_TOKEN)
            .env("GITHUB_TOKEN", DEAD_TOKEN)
            .env("GH_CONFIG_DIR", self.base.join("gh"))
            .env("HEXA_NO_CLAUDE_MODS", "1");
        for (k, _) in std::env::vars_os() {
            if !inherited(&k.to_string_lossy()) {
                c.env_remove(&k);
            }
        }
        c
    }

    fn sh(&self, step: &str) -> anyhow::Result<String> {
        let out = self.cmd("sh").args(["-c", step]).output()?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if !out.status.success() {
            bail!("`{step}` exited {}:\n{}", out.status.code().unwrap_or(-1), tail(&text, 20));
        }
        Ok(text)
    }
}

/// Whether a variable of the caller's reaches the scenario. A scenario is a
/// fresh top-level session: when `hexa eval` runs inside a Claude Code
/// session, that session's id (`CLAUDE_CODE_SESSION_ID`, `CLAUDE_SESSION_ID`)
/// would otherwise bind setup's `hexa loop` records to the caller, and the
/// model would see them as another session's work. `CLAUDE_CONFIG_DIR` is
/// where claude keeps its login, and `HEXA_HOME` where the person keeps
/// hexa's state; every other `CLAUDE*`, `HEXA_*` and agent-host variable is
/// the caller's own.
fn inherited(key: &str) -> bool {
    if key == "CLAUDE_CONFIG_DIR" || key == "HEXA_HOME" || key == "HEXA_NO_CLAUDE_MODS" {
        return true;
    }
    !(key.starts_with("CLAUDE") || key.starts_with("HEXA_") || key.starts_with("T3CODE_") || key == "AI_AGENT")
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        if name == "target" || name == ".git" {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if e.file_type()?.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn build(name: &str, d: &Path, s: &Scenario, hexa: &Path, keep: bool) -> anyhow::Result<Sandbox> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.f");
    let base = std::env::temp_dir().join(format!("hexa-eval-{name}-{}-{stamp}", std::process::id()));
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(hexa, bin.join("hexa"))?;
    #[cfg(not(unix))]
    std::fs::copy(hexa, bin.join("hexa"))?;
    let path_env = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let sb = Sandbox { repo: base.join("repo"), base, path_env, keep };
    copy_tree(&d.join("fixture"), &sb.repo).with_context(|| format!("{name}: copying the fixture"))?;
    for step in [
        "git init -q -b main",
        "git add -A",
        "git commit -qm 'fixture'",
        "hexa init",
        "git add -A",
        "git commit -q --allow-empty -m 'hexa init'",
    ] {
        sb.sh(step).with_context(|| format!("{name}: building the repository"))?;
    }
    for step in &s.setup {
        sb.sh(step).with_context(|| format!("{name}: setup"))?;
    }
    Ok(sb)
}

/// Run the outcome command. `(passed, output tail)`.
fn check_outcome(s: &Scenario, repo: &Path, sb: Option<&Sandbox>) -> anyhow::Result<(bool, String)> {
    let mut c = match sb {
        Some(sb) => sb.cmd("sh"),
        None => {
            let mut c = Command::new("sh");
            c.current_dir(repo);
            c
        }
    };
    let out = c.args(["-c", &s.outcome]).output().context("running the outcome command")?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), tail(&text, 10)))
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn claude_args(s: &Scenario, model: Option<&str>) -> Vec<String> {
    let mut v: Vec<String> = vec![
        "-p".into(),
        s.prompt.trim().to_string(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--max-turns".into(),
        s.max_turns.to_string(),
        "--max-budget-usd".into(),
        format!("{}", s.budget_usd),
        // Nobody is there to answer a permission prompt. The hooks still run.
        "--permission-mode".into(),
        "bypassPermissions".into(),
        // Only the scenario's own settings: the hooks `hexa init` installed.
        "--setting-sources".into(),
        "project,local".into(),
        "--no-session-persistence".into(),
    ];
    if let Some(m) = model {
        v.push("--model".into());
        v.push(m.into());
    }
    v
}

fn printable(args: &[String]) -> String {
    std::iter::once("claude".to_string()).chain(args.iter().map(|a| shell_quote(a))).collect::<Vec<_>>().join(" ")
}

fn dry_run(picked: &[(String, PathBuf)], hexa: &Path, a: &RunArgs) -> anyhow::Result<()> {
    println!("{} hexa eval --dry-run: {} scenario(s), hexa {}", "⬡".cyan(), picked.len(), hexa.display());
    let mut failed = 0;
    for (name, d) in picked {
        let built = load(d).and_then(|s| {
            let sb = build(name, d, &s, hexa, a.keep)?;
            if s.outcome_fails_at_start {
                let (passed, out) = check_outcome(&s, &sb.repo, Some(&sb))?;
                if passed {
                    bail!("the outcome already passes before the model runs, so the scenario measures nothing:\n{out}");
                }
            }
            Ok((s, sb))
        });
        match built {
            Ok((s, sb)) => {
                println!("\n{} {name}", "✓".green());
                println!("  repo:  {}{}", sb.repo.display(), if a.keep { "" } else { " (removed)" });
                println!("  would run (in the repo, with {} first on PATH):", sb.base.join("bin").display());
                println!("    {}", printable(&claude_args(&s, a.model.as_deref())));
                println!("  then:  sh -c {}", shell_quote(s.outcome.trim()));
            }
            Err(e) => {
                failed += 1;
                println!("\n{} {name}: {e:#}", "✗".red());
            }
        }
    }
    println!("\nnothing was spent, and nothing was written in this repository");
    if failed > 0 {
        bail!("{failed} scenario(s) could not be built");
    }
    Ok(())
}

// ── a real run ──────────────────────────────────────────────────────────────

fn on_path(program: &str) -> bool {
    Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// hexa's daily budget (`inference.budget_usd_per_day`), counting what this
/// run has already spent: the eval's `claude -p` calls are not in the log.
fn budget_allows(spent_here: f64) -> Result<(), String> {
    let mut rows = hexa_infer::spend_entries();
    if spent_here > 0.0 {
        rows.push(serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "model": "hexa-eval",
            "source": "hexa-eval",
            "input_tokens": 0,
            "output_tokens": 0,
            "cost_usd": spent_here,
        }));
    }
    hexa_infer::spend_report::budget_verdict(&rows, hexa_infer::spend_budget())
}

fn file_safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

fn real_run(root: &Path, picked: &[(String, PathBuf)], hexa: &Path, a: &RunArgs) -> anyhow::Result<()> {
    if !on_path("claude") {
        bail!("`claude` is not on PATH, so no scenario can run. `hexa eval run --dry-run` builds them without it.");
    }
    let scenarios: Vec<(String, PathBuf, Scenario)> =
        picked.iter().map(|(n, d)| load(d).map(|s| (n.clone(), d.clone(), s))).collect::<anyhow::Result<_>>()?;
    let model = a.model.clone().unwrap_or_else(|| "default".into());
    let started = chrono::Utc::now();
    let stem = format!("{}-{}", started.format("%Y%m%dT%H%M%SZ"), file_safe(&model));
    let out_dir = evals_out(root);
    let transcripts = out_dir.join(&stem);
    std::fs::create_dir_all(&transcripts)?;
    let hexa_version = Command::new(hexa)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let mut file = RunFile { schema: 1, model: model.clone(), hexa: hexa_version, started: started.to_rfc3339(), rows: vec![] };
    let result_path = out_dir.join(format!("{stem}.json"));
    let worst_case: f64 = scenarios.iter().map(|(_, _, s)| s.budget_usd).sum();
    println!(
        "{} hexa eval: {} scenario(s) on {model}; at most ${worst_case:.2} (each scenario stops at its budget_usd)",
        "⬡".cyan(),
        scenarios.len()
    );

    let mut spent = 0.0;
    for (name, d, s) in &scenarios {
        if let Err(why) = budget_allows(spent) {
            println!("{} stopping before {name}: {why}", "⛔".red());
            break;
        }
        print!("  {name} … ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let row = match one_scenario(name, d, s, hexa, a, &transcripts, &stem) {
            Ok(r) => r,
            Err(e) => Row {
                scenario: name.clone(),
                outcome: None,
                process: 0.0,
                bypasses: 0,
                refusals: 0,
                turns: None,
                cost_usd: None,
                stop: Some("error".into()),
                expected: vec![],
                bypass_calls: vec![],
                transcript: None,
                error: Some(format!("{e:#}")),
            },
        };
        spent += row.cost_usd.unwrap_or(0.0);
        println!(
            "{} · process {:.2} · {} bypass(es) · {} refusal(s) · ${:.2}{}",
            outcome_word(row.outcome),
            row.process,
            row.bypasses,
            row.refusals,
            row.cost_usd.unwrap_or(0.0),
            row.error.as_deref().map(|e| format!(" · {}", e.lines().next().unwrap_or(""))).unwrap_or_default()
        );
        file.rows.push(row);
        // Written after every scenario, so a killed run keeps what it measured.
        std::fs::write(&result_path, serde_json::to_string_pretty(&file)?)?;
    }
    println!("\n{} {}", "wrote".dimmed(), result_path.display());
    let previous = previous_of(&out_dir, &result_path);
    print_table(&file.rows, previous.as_ref().map(|(_, f)| f));
    Ok(())
}

fn one_scenario(
    name: &str,
    d: &Path,
    s: &Scenario,
    hexa: &Path,
    a: &RunArgs,
    transcripts: &Path,
    stem: &str,
) -> anyhow::Result<Row> {
    let sb = build(name, d, s, hexa, a.keep)?;
    let tpath = transcripts.join(format!("{name}.jsonl"));
    let stdout = std::fs::File::create(&tpath)?;
    let stderr = std::fs::File::create(transcripts.join(format!("{name}.stderr")))?;
    let mut child = sb
        .cmd("claude")
        .args(claude_args(s, a.model.as_deref()))
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .context("starting claude")?;
    let t0 = Instant::now();
    let mut killed = false;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if t0.elapsed() > WALL_CLOCK {
            let _ = child.kill();
            let _ = child.wait();
            killed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let (passed, _) = check_outcome(s, &sb.repo, Some(&sb))?;
    let text = std::fs::read_to_string(&tpath)?;
    let rel = format!("{stem}/{name}.jsonl");
    let mut sc = ue::score_transcript(s, &text, Some(passed)).map_err(|e| anyhow!("{}: {e}", tpath.display()))?;
    let error = if killed {
        sc.stop = Some("killed: wall clock".into());
        Some(format!("killed after {}s", WALL_CLOCK.as_secs()))
    } else if sc.stop.is_none() {
        Some(format!("the transcript has no result record; see {name}.stderr"))
    } else {
        None
    };
    if a.keep {
        print!("[kept {}] ", sb.repo.display());
    }
    Ok(row_of(name, &sc, Some(rel), error))
}

// ── reporting ───────────────────────────────────────────────────────────────

fn outcome_word(o: Option<bool>) -> String {
    match o {
        Some(true) => "pass".green().to_string(),
        Some(false) => "fail".red().to_string(),
        None => "not run".dimmed().to_string(),
    }
}

fn result_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn read_run(p: &Path) -> anyhow::Result<RunFile> {
    let text = std::fs::read_to_string(p).with_context(|| p.display().to_string())?;
    serde_json::from_str(&text).map_err(|e| anyhow!("{}: not a hexa eval result: {e}", p.display()))
}

fn previous_of(dir: &Path, current: &Path) -> Option<(PathBuf, RunFile)> {
    let files = result_files(dir);
    let i = files.iter().position(|f| f == current)?;
    let p = files.get(i.checked_sub(1)?)?.clone();
    read_run(&p).ok().map(|f| (p, f))
}

fn report(a: ReportArgs) -> anyhow::Result<()> {
    let dir = evals_out(&root());
    let current = match a.file {
        Some(f) => f,
        None => result_files(&dir)
            .pop()
            .ok_or_else(|| anyhow!("no results in {}; run `hexa eval run` first", dir.display()))?,
    };
    let run = read_run(&current)?;
    let against = match a.against {
        Some(p) => Some((p.clone(), read_run(&p)?)),
        None => previous_of(current.parent().unwrap_or(&dir), &current),
    };
    println!("{} {} · model {} · {}", "⬡".cyan(), current.display(), run.model, run.hexa);
    if let Some((p, _)) = &against {
        println!("  compared with {}", p.display());
    }
    print_table(&run.rows, against.as_ref().map(|(_, f)| f));
    for r in &run.rows {
        if !r.bypass_calls.is_empty() || r.error.is_some() {
            print_detail(r);
        }
    }
    Ok(())
}

fn delta_f(now: f64, then: Option<f64>) -> String {
    match then {
        Some(t) if (now - t).abs() > 1e-9 => format!(" ({:+.2})", now - t),
        _ => String::new(),
    }
}

fn delta_i(now: u32, then: Option<u32>) -> String {
    match then {
        Some(t) if t != now => format!(" ({:+})", now as i64 - t as i64),
        _ => String::new(),
    }
}

fn print_table(rows: &[Row], against: Option<&RunFile>) {
    println!();
    println!(
        "  {:<18} {:<14} {:<14} {:<12} {:<12} {:>6} {:>8}",
        "scenario", "outcome", "process", "bypasses", "refusals", "turns", "cost"
    );
    for r in rows {
        let then = against.and_then(|f| f.rows.iter().find(|x| x.scenario == r.scenario));
        let outcome = match then.map(|t| t.outcome) {
            Some(t) if t != r.outcome => format!("{} (was {})", plain_outcome(r.outcome), plain_outcome(t)),
            _ => plain_outcome(r.outcome).to_string(),
        };
        println!(
            "  {:<18} {:<14} {:<14} {:<12} {:<12} {:>6} {:>8}",
            r.scenario,
            outcome,
            format!("{:.2}{}", r.process, delta_f(r.process, then.map(|t| t.process))),
            format!("{}{}", r.bypasses, delta_i(r.bypasses, then.map(|t| t.bypasses))),
            format!("{}{}", r.refusals, delta_i(r.refusals, then.map(|t| t.refusals))),
            r.turns.map(|t| t.to_string()).unwrap_or_else(|| "-".into()),
            r.cost_usd.map(|c| format!("${c:.2}")).unwrap_or_else(|| "-".into()),
        );
    }
    if rows.len() > 1 {
        let passed = rows.iter().filter(|r| r.outcome == Some(true)).count();
        let process = rows.iter().map(|r| r.process).sum::<f64>() / rows.len() as f64;
        println!(
            "  {:<18} {:<14} {:<14} {:<12} {:<12} {:>6} {:>8}",
            "total",
            format!("{passed}/{} pass", rows.len()),
            format!("{process:.2} mean"),
            rows.iter().map(|r| r.bypasses).sum::<u32>(),
            rows.iter().map(|r| r.refusals).sum::<u32>(),
            rows.iter().filter_map(|r| r.turns).sum::<u32>(),
            format!("${:.2}", rows.iter().filter_map(|r| r.cost_usd).sum::<f64>()),
        );
    }
}

fn plain_outcome(o: Option<bool>) -> &'static str {
    match o {
        Some(true) => "pass",
        Some(false) => "fail",
        None => "not run",
    }
}

fn print_detail(r: &Row) {
    println!("\n  {}", r.scenario.bold());
    for h in &r.expected {
        println!("    {} {} (weight {})", if h.matched { "✓".green() } else { "✗".red() }, h.label, h.weight);
    }
    for b in &r.bypass_calls {
        println!("    {} bypass: {b}", "!".yellow());
    }
    if let Some(e) = &r.error {
        println!("    {} {e}", "error:".red());
    }
}
