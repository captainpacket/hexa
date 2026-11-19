//! Which runner a scenario's test names, and where tests run (ADR-2611191120).
//!
//! Pure rules over values: parsing a test reference, choosing its runner by
//! extension, filling command templates, the remote-run protocol (markers and
//! the commit it must name), and judging one test's output. Running anything
//! is the adapter's (`hexa-exec`).

use std::collections::BTreeMap;

/// A scenario's test: `<file>::<name>`, or a bare `<file>.sh` (§1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRef {
    /// The path from the repository root, as written.
    pub file: String,
    /// What follows the first `::`; `None` only for a bare `.sh`.
    pub name: Option<String>,
}

impl TestRef {
    /// The file's extension, without its dot.
    pub fn ext(&self) -> &str {
        todo!("ADR-2611191120 §1")
    }
}

/// Split `r` at its first `::` (§1). The file must have an extension; the
/// name must be non-empty, except that a `.sh` file may have none.
pub fn parse_test_ref(r: &str) -> Option<TestRef> {
    let _ = r;
    todo!("ADR-2611191120 §1")
}

/// The runner a test reference uses (§1, §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runner {
    Cargo,
    Vitest,
    Bash,
    /// A `test.runners` template from `.hexa/project.json`.
    Configured(String),
}

/// The runner for files with extension `ext`: a configured template first,
/// then the built-in ones; `None` when there is neither (§1, §2).
pub fn runner_for(ext: &str, configured: &BTreeMap<String, String>) -> Option<Runner> {
    let _ = (ext, configured);
    todo!("ADR-2611191120 §1 §2")
}

/// Whether a file bound to a scenario is a test file that may be sealed (§8).
pub fn is_test_file(path: &str) -> bool {
    let _ = path;
    todo!("ADR-2611191120 §8")
}

/// `s` as one single-quoted shell word.
pub fn shell_quote(s: &str) -> String {
    let _ = s;
    todo!("ADR-2611191120 §2")
}

/// `template` with each `{key}` of `vars` replaced by its value, single-quoted
/// for the shell (§2, §3).
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let _ = (template, vars);
    todo!("ADR-2611191120 §2")
}

/// `Err` naming what is missing when a `test.remote` template lacks `{commit}`
/// or `{cmd}` (§3).
pub fn check_remote_template(template: &str) -> Result<(), String> {
    let _ = template;
    todo!("ADR-2611191120 §3")
}

/// The command that runs `cmd` at `commit` through `template` (§3). `commit`
/// must be a full 40-hex commit id.
pub fn remote_command(template: &str, commit: &str, cmd: &str) -> Result<String, String> {
    let _ = (template, commit, cmd);
    todo!("ADR-2611191120 §3")
}

/// Whether a remote run's output names the full commit it was sent (§5).
pub fn runner_ran(output: &str, commit: &str) -> bool {
    let _ = (output, commit);
    todo!("ADR-2611191120 §5")
}

/// One script that runs every command in turn, each between a begin and an
/// end marker holding `nonce`, the end marker carrying its exit status (§4).
pub fn batch_script(nonce: &str, cmds: &[String]) -> String {
    let _ = (nonce, cmds);
    todo!("ADR-2611191120 §4")
}

/// Each command's (exit 0, output) from a batch run's output, in order. `Err`
/// when any end marker is missing: the run is a runner failure (§4, §5).
pub fn split_batch(nonce: &str, n: usize, output: &str) -> Result<Vec<(bool, String)>, String> {
    let _ = (nonce, n, output);
    todo!("ADR-2611191120 §4 §5")
}

/// What one test's run established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Passed,
    Failed(String),
    /// Nothing ran, or what matched was skipped.
    Vacuous,
    /// More than one test ran.
    Ambiguous(u64),
    /// The output is not one this judge can read: never a pass.
    Unreadable(String),
}

/// The vitest `-t` pattern that selects a test named `name` (§1).
pub fn vitest_pattern(name: &str) -> String {
    let _ = name;
    todo!("ADR-2611191120 §1")
}

/// Judge a `vitest run --reporter=json` output for the test named `name` (§1).
pub fn judge_vitest(output: &str, name: &str) -> Verdict {
    let _ = (output, name);
    todo!("ADR-2611191120 §1")
}

/// Judge a bash test script's run (§1).
pub fn judge_script(exit_ok: bool, output: &str) -> Verdict {
    let _ = (exit_ok, output);
    todo!("ADR-2611191120 §1")
}

/// Judge a configured runner's run, by the evidence reader (§2).
pub fn judge_counted(exit_ok: bool, output: &str) -> Verdict {
    let _ = (exit_ok, output);
    todo!("ADR-2611191120 §2")
}
