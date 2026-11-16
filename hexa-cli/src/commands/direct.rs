//! `hexa do` — drive the direct executor (ADR-2026-06-04-1740 Path A) from the
//! terminal. The new doing-path: task → one agent → evidence-gated edit → commit.
//! No SOP/persona pipeline. Backed by POST /api/direct/execute + GET /api/direct/runs.

use clap::Subcommand;
use colored::Colorize;
use serde_json::json;


#[derive(Subcommand)]
pub enum DoAction {
    /// Run one evidence-gated task: edit a file until the evidence command exits 0, then commit.
    #[command(after_help = "Environment: CLAUDE_TIMEOUT_SECS — seconds a claude-* delegate may run before it is stopped and its files restored (default 300).")]
    Run {
        /// What to do, in plain language.
        instruction: String,
        /// Repo-relative file to edit.
        #[arg(short, long)]
        file: String,
        /// Shell command that must exit 0 (e.g. "cargo test -p hexa-nexus --lib my_test").
        #[arg(short, long)]
        evidence: String,
        /// Reasoning model override.
        #[arg(short, long)]
        model: Option<String>,
        /// Max edit→verify attempts in --fast mode (default 3).
        #[arg(short, long)]
        attempts: Option<u32>,
        /// Use the single-shot path (read → one edit → evidence) instead of the
        /// default multi-step ReAct tool-use loop (ADR-2606071XXX).
        #[arg(long)]
        fast: bool,
        /// Max ReAct loop steps before giving up (default 12). Ignored with --fast.
        #[arg(long)]
        max_steps: Option<u32>,
        /// An additional repo-relative file the run may create or edit besides --file
        /// (repeatable; may not exist yet). Each that exists after a passing run is
        /// committed in the same commit as --file; on a failed run they are restored
        /// (created ones deleted). No other new file is ever committed. Any
        /// other file a claude-* delegate changes is restored before the gate
        /// in an isolated run, and left (and named) on the operator's tree.
        #[arg(long = "also", value_name = "ALSO")]
        also: Vec<String>,
        /// The evidence runs no test runner (a build, a lint): its exit status
        /// alone decides, and the report and commit say `uncounted`
        /// (ADR-2611081151 §6). Without it, output no runner wrote fails.
        #[arg(long)]
        uncounted: bool,
        /// Commit the tracked files you had already modified in the target's
        /// package together with this change. Without it, such files refuse the
        /// run before any change is made: name the ones the change needs with
        /// --also, and commit or stash the rest (ADR-2611160730 §2).
        #[arg(long = "include-dirty")]
        include_dirty: bool,
    },
    /// Gate the change already in your tree and commit ONLY the named files (ADR-2611160730 §3).
    /// No model is called: use it for a patch you wrote yourself. A failing or vacuous gate
    /// commits nothing and leaves the change where it is.
    Gate {
        /// The files the change is in (repeatable, or several after one --files). Only these are committed.
        #[arg(long = "files", value_name = "PATH", num_args = 1.., required = true)]
        files: Vec<String>,
        /// Shell command that must pass (a test runner that reports at least one test, or any
        /// command with --uncounted).
        #[arg(short, long)]
        evidence: String,
        /// The commit message.
        #[arg(short, long)]
        message: String,
        /// The evidence runs no test runner: its exit status alone decides, and the commit says so.
        #[arg(long)]
        uncounted: bool,
    },
    /// List recent direct runs (task, evidence verdict, commit) in this repository.
    Runs {
        /// List every repository's runs, and runs logged before runs named theirs.
        #[arg(long)]
        all: bool,
    },
}

/// Where a failed `do run` stopped, read from its result: `commit` when the
/// evidence passed; `evidence` when the evidence ran and failed; otherwise the
/// stage before it — `parse` (a reply that held no usable edit), `inference`
/// (no model, or no reply), or `setup` (the run never reached the model).
///
/// The evidence ran exactly when it left output; a failing command with no
/// output at all is read from the error, which then names an evidence failure.
fn failure_stage(error: &str, evidence_passed: bool, evidence_output: &str) -> &'static str {
    if evidence_passed {
        return "commit";
    }
    if !evidence_output.is_empty() {
        return "evidence";
    }
    // The last attempt's reason is what stopped the run.
    let last = error.rsplit("last: ").next().unwrap_or(error);
    const PARSE: [&str; 7] = [
        "no fenced code block",
        "fenced blocks",
        "non-empty old snippet",
        "empty replacement",
        "apply:",
        "loop ended with no edit",
        "propose_edit",
    ];
    if PARSE.iter().any(|m| last.contains(m)) {
        return "parse";
    }
    if last.starts_with("no model configured") || last.contains("inference:") || last.contains("claude spawn failed") {
        return "inference";
    }
    if last.contains("evidence") && !last.contains("before") {
        return "evidence";
    }
    "setup"
}

pub async fn run(action: DoAction) -> anyhow::Result<()> {
    // No daemon. `hexa do` used to POST the whole task to /api/direct/execute and require nexus to
    // be up first — for a route that was a one-line passthrough to `execute_direct`, in a crate
    // hexa-cli already depends on. The control plane was relaying a call to a library sitting in
    // the same binary.

    match action {
        DoAction::Run { instruction, file, evidence, model, attempts, fast, max_steps, also, uncounted, include_dirty } => {
            if let Err(e) = hexa_exec::direct_exec::validate_also_paths(&also) {
                anyhow::bail!("{e}");
            }
            // Interactive operator run: commit on the operator's own branch
            // (ADR-2606071323 scopes `hexa do` out of worktree isolation — the
            // human owns their tree). Unset/autonomous callers isolate by default.
            // The evidence command becomes the loop's gate only if it has none
            // (ADR-2611020815): side work must not replace the work's gate.
            crate::commands::loop_cmd::note_run_gate(&evidence, "build", None);
            if uncounted {
                crate::commands::loop_cmd::note_gate_uncounted(&evidence);
            }
            let mut body = json!({ "instruction": instruction, "file": file, "evidence": evidence, "fast": fast, "isolate": false, "also": also, "uncounted": uncounted, "include_dirty": include_dirty });
            if let Some(m) = model {
                body["model"] = json!(m);
            }
            if let Some(a) = attempts {
                body["max_attempts"] = json!(a);
            }
            if let Some(s) = max_steps {
                body["max_steps"] = json!(s);
            }
            let mode = if fast { "single-shot" } else { "react loop" };
            println!("{} {} {}", "⬡ direct:".cyan().bold(), instruction, format!("[{}]", mode).dimmed());
            println!("  {} {}  {} {}", "file".dimmed(), file, "evidence".dimmed(), evidence);
            if uncounted {
                println!("  {} evidence uncounted: its exit status alone decides (ADR-2611081151 §6)", "!".yellow());
            }
            if !also.is_empty() {
                println!("  {} {}", "also".dimmed(), also.join(", "));
            }

            let task: hexa_exec::direct_exec::DirectTask = serde_json::from_value(body)?;
            let r = serde_json::to_value(hexa_exec::execute_direct(task).await)?;

            let ok = r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let ev = r.get("evidence_passed").and_then(|v| v.as_bool()).unwrap_or(false);
            let attempts_n = r.get("attempts").and_then(|v| v.as_u64()).unwrap_or(0);
            let committed = r.get("committed").and_then(|v| v.as_str());
            let step_word = if fast { "attempt(s)" } else { "step(s)" };
            let ev_label = match (ev, uncounted) {
                (true, true) => "pass (uncounted)".green(),
                (true, false) => "pass".green(),
                (false, _) => "fail".red(),
            };

            // Files the run changed but was not given (parley K17): name them
            // whatever the outcome, so they are never left without a word.
            let names = |key: &str| -> Vec<String> {
                r.get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default()
            };
            let restored = names("restored_unlisted");
            if !restored.is_empty() {
                println!(
                    "  {} restored {} file(s) the run was not given: {}",
                    "!".yellow().bold(),
                    restored.len(),
                    restored.join(", ")
                );
            }
            let left = names("left_modified");
            if !left.is_empty() {
                println!(
                    "  {} {} file(s) changed during the run that it was not given were left as they are (operator tree): {}",
                    "!".yellow().bold(),
                    left.len(),
                    left.join(", ")
                );
            }

            if ok {
                println!(
                    "{} evidence {} · {} {} · commit {}",
                    "✓ done".green().bold(),
                    ev_label,
                    attempts_n,
                    step_word,
                    committed.unwrap_or("—").yellow()
                );
            } else {
                let err = r.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                let output = r.get("evidence_output").and_then(|v| v.as_str()).unwrap_or("");
                let stage = failure_stage(err, ev, output);
                // Only a run whose evidence ran has an evidence verdict to show.
                let verdict = match stage {
                    "evidence" | "commit" => format!("evidence {ev_label}"),
                    _ => "evidence not run".to_string(),
                };
                println!(
                    "{} {} · {} {}\n  {}",
                    format!("✗ failed at {stage}").red().bold(),
                    verdict,
                    attempts_n,
                    step_word,
                    err.dimmed()
                );
                if let Some(out) = r.get("evidence_output").and_then(|v| v.as_str()) {
                    let tail: Vec<&str> = out.lines().rev().take(8).collect();
                    for line in tail.into_iter().rev() {
                        println!("  {}", line.dimmed());
                    }
                }
                // Name the half that actually failed. This said "did not pass
                // evidence" for every failure, including a run whose evidence
                // passed and whose commit did not — which is the common case in
                // a fresh clone with no git identity.
                let evidence_passed = r
                    .get("evidence_passed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if evidence_passed {
                    anyhow::bail!("evidence passed; the run did not complete (see above)");
                }
                match stage {
                    "evidence" => anyhow::bail!("direct run did not pass evidence"),
                    other => anyhow::bail!("direct run failed at {other}, before its evidence ran"),
                }
            }
        }
        DoAction::Gate { files, evidence, message, uncounted } => {
            let top = std::process::Command::new("git").args(["rev-parse", "--show-toplevel"]).output()?;
            if !top.status.success() {
                anyhow::bail!("hexa do gate runs inside a git repository");
            }
            let root = std::path::PathBuf::from(String::from_utf8_lossy(&top.stdout).trim().to_string());
            // Paths are given from where you stand; the gate runs and commits from the repository root.
            let cwd = std::env::current_dir()?;
            let rel: Vec<String> = files
                .iter()
                .map(|f| {
                    let abs = cwd.join(f);
                    abs.strip_prefix(&root).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| f.clone())
                })
                .collect();
            crate::commands::loop_cmd::note_run_gate(&evidence, "build", None);
            if uncounted {
                crate::commands::loop_cmd::note_gate_uncounted(&evidence);
            }
            println!("{} {}", "⬡ gate:".cyan().bold(), evidence);
            println!("  {} {}", "files".dimmed(), rel.join(", "));
            let started_at = chrono::Utc::now().to_rfc3339();
            let clock = std::time::Instant::now();
            let outcome = hexa_exec::direct_exec::gate_patch(&root, &rel, &evidence, &message, uncounted).await;
            // `do runs` lists gate runs beside `do run`'s, from one feed.
            hexa_exec::direct_exec::record_gate_run(
                &hexa_exec::run_log(),
                &root,
                &rel,
                &message,
                &outcome,
                started_at,
                u64::try_from(clock.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
            match outcome {
                Ok(sha) => println!("{} commit {} · {} file(s)", "✓ gated".green().bold(), sha.yellow(), rel.len()),
                Err(e) => {
                    println!("{}\n  {}", "✗ not committed".red().bold(), e.replace('\n', "\n  "));
                    anyhow::bail!("the gate did not pass");
                }
            }
        }
        DoAction::Runs { all } => {
            let here = if all {
                None
            } else {
                let root = std::process::Command::new("git")
                    .args(["rev-parse", "--show-toplevel"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string()))
                    .unwrap_or(std::env::current_dir()?);
                Some(hexa_exec::direct_exec::repo_key(&root))
            };
            let (runs, hidden) = hexa_exec::direct_exec::runs_for_repo(&hexa_exec::run_log(), here.as_deref());
            let r = json!({
                "summary": hexa_exec::direct_exec::summary_of(&runs),
                "runs": runs,
            });
            let s = &r["summary"];
            let pass_pct = (s["pass_rate"].as_f64().unwrap_or(0.0) * 100.0).trunc() as u32;
            println!(
                "{}  {} runs · {} passed · {} failed · {} committed · {}% pass",
                "⬡ Direct Runs".cyan().bold(),
                s["total"],
                s["passed"].to_string().green(),
                s["failed"],
                s["committed"].to_string().yellow(),
                pass_pct
            );
            if let Some(runs) = r["runs"].as_array() {
                if runs.is_empty() {
                    println!("  {}", "no runs here yet — `hexa do run …` to start".dimmed());
                }
                for run in runs.iter().take(30) {
                    let ev = run["evidence_passed"].as_bool().unwrap_or(false);
                    let mark = if ev { "✓".green() } else { "✗".red() };
                    let commit = run["committed"].as_str().unwrap_or("—");
                    let file = run["file"].as_str().unwrap_or("").rsplit('/').next().unwrap_or("");
                    let instr: String = run["instruction"].as_str().unwrap_or("").chars().take(64).collect();
                    println!("  {} {:<9} {:<18} {}", mark, commit.yellow(), file.dimmed(), instr);
                }
            }
            if hidden > 0 {
                println!(
                    "  {}",
                    format!("{hidden} run(s) of other repositories (or logged before runs named theirs) not shown; --all lists them")
                        .dimmed()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::failure_stage;

    #[test]
    fn a_run_that_never_reached_its_evidence_says_where_it_stopped() {
        assert_eq!(failure_stage("no model configured — run `hexa config model t2 <model>`", false, ""), "inference");
        assert_eq!(failure_stage("inference: connection refused", false, ""), "inference");
        assert_eq!(
            failure_stage("exhausted 3 attempts without passing evidence. last: inference: no fenced code block in reply", false, ""),
            "parse"
        );
        assert_eq!(failure_stage("loop ended with no edit: I think the file is fine", false, ""), "parse");
        assert_eq!(failure_stage("workspace: no such worktree", false, ""), "setup");
    }

    #[test]
    fn a_run_whose_evidence_ran_fails_at_evidence_or_commit() {
        assert_eq!(failure_stage("exhausted 3 attempts", false, "test result: FAILED"), "evidence");
        assert_eq!(failure_stage("evidence PASSED but the commit failed", true, "ok"), "commit");
        assert_eq!(
            failure_stage("exhausted 3 attempts without passing evidence. last: evidence `false` exited non-zero", false, ""),
            "evidence"
        );
    }
}
