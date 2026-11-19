//! What a gate's run proved (ADR-2611081151 §4–§6).
//!
//! The one reader of test-runner output. Only `Passed`, or `Uncounted` for
//! a gate a person declared uncounted, passes; output it cannot read is
//! `Unrecognised`, never a pass.

/// The result of reading one gate run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// Exited 0, a recognised runner reported at least one test, none failed.
    Passed { tests: u64 },
    /// Exited non-zero, or a recognised line reported a failed test.
    Failed { reason: String },
    /// Exited 0 and every recognised runner reported zero tests.
    Vacuous,
    /// Exited 0 and no line came from a runner this reader knows.
    Unrecognised,
    /// Declared uncounted (§6) and exited 0.
    Uncounted,
}

impl Evidence {
    /// Passed or Uncounted (§5, §6).
    pub fn passes(&self) -> bool {
        matches!(self, Evidence::Passed { .. } | Evidence::Uncounted)
    }
}

/// The runners this reader knows, for a refusal that has to name them (§5).
pub const KNOWN_RUNNERS: &str = "cargo test, go test, node --test (TAP or the spec reporter), bun test, vitest (its summary or --reporter=json), npx vitest, hexa spec verify, bash (one script alone, ADR-2611191120 §7)";

/// Read a gate run. `uncounted` is the declaration of §6; without it the
/// exit status alone never passes.
pub fn read_evidence(exit_ok: bool, output: &str, uncounted: bool) -> Evidence {
    if !exit_ok {
        return Evidence::Failed { reason: "the gate exited non-zero".to_string() };
    }
    if uncounted {
        return Evidence::Uncounted;
    }
    let t = tally(output);
    if t.failed > 0 {
        return Evidence::Failed { reason: format!("the output reports {} failed test(s)", t.failed) };
    }
    if !t.recognised {
        return Evidence::Unrecognised;
    }
    if t.passed == 0 {
        return Evidence::Vacuous;
    }
    Evidence::Passed { tests: t.passed }
}

/// Read a gate run by its command too (ADR-2611191120 §7): a gate that is
/// exactly one bash script and exits 0 counts as one passed test when its
/// output holds no runner line this reader knows.
pub fn read_gate_evidence(cmd: &str, exit_ok: bool, output: &str, uncounted: bool) -> Evidence {
    match read_evidence(exit_ok, output, uncounted) {
        Evidence::Unrecognised if is_single_script(cmd) => Evidence::Passed { tests: 1 },
        e => e,
    }
}

/// Whether `cmd` is exactly one bash script: `bash <p>.sh [args]`, `sh <p>.sh`
/// or `./<p>.sh`, with no `;`, `&&`, `||` or `|` (ADR-2611191120 §7).
pub fn is_single_script(cmd: &str) -> bool {
    let cmd = cmd.trim();
    if ["&", "|", ";", "\n", "`", "$("].iter().any(|op| cmd.contains(op)) {
        return false;
    }
    let words: Vec<&str> = cmd.split_whitespace().collect();
    match words.as_slice() {
        [shell, script, ..] if matches!(*shell, "bash" | "sh") => script.ends_with(".sh") && !script.starts_with('-'),
        [script, ..] => script.ends_with(".sh") && script.contains('/'),
        [] => false,
    }
}

#[derive(Default)]
struct Tally {
    recognised: bool,
    passed: u64,
    failed: u64,
}

impl Tally {
    fn saw(&mut self, passed: u64, failed: u64) {
        self.recognised = true;
        self.passed = self.passed.saturating_add(passed);
        self.failed = self.failed.saturating_add(failed);
    }
}

/// The number before `word` in `text`, as in `5 passed` or `0 fail`.
fn count_before(text: &str, word: &str) -> Option<u64> {
    let words: Vec<&str> = text.split(|c: char| c.is_whitespace() || c == ';' || c == ',').filter(|w| !w.is_empty()).collect();
    words.windows(2).find(|w| w[1].trim_end_matches(['.', ':']) == word).and_then(|w| w[0].parse().ok())
}

/// The number after `prefix` at the start of `line`, as in `# pass 3`.
fn count_after(line: &str, prefix: &str) -> Option<u64> {
    line.strip_prefix(prefix)?.split_whitespace().next()?.parse().ok()
}

/// The number after `"key":` in a JSON report, as in `"numPassedTests": 4`.
fn json_count(output: &str, key: &str) -> Option<u64> {
    let at = output.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = output[at..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn tally(output: &str) -> Tally {
    let mut t = Tally::default();
    // vitest's (and Jest's) JSON report (ADR-2611191120 §7).
    if let Some(passed) = json_count(output, "numPassedTests") {
        t.saw(passed, json_count(output, "numFailedTests").unwrap_or(0));
    }
    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        // vitest's summary: "Test Files  1 passed (1)" names the runner;
        // "Tests  1 failed | 4 passed (5)" counts (ADR-2611191120 §7).
        if line.starts_with("Test Files ") && line.ends_with(')') {
            t.recognised = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("Tests ").filter(|_| line.ends_with(')')) {
            t.saw(count_before(rest, "passed").unwrap_or(0), count_before(rest, "failed").unwrap_or(0));
            continue;
        }

        // cargo: "test result: ok. 5 passed; 0 failed; …" and "test x ... FAILED".
        if let Some(rest) = line.strip_prefix("test result:") {
            let passed = count_before(rest, "passed").unwrap_or(0);
            let mut failed = count_before(rest, "failed").unwrap_or(0);
            if failed == 0 && rest.trim_start().starts_with("FAILED") {
                failed = 1;
            }
            t.saw(passed, failed);
            continue;
        }
        if line.starts_with("test ") && line.ends_with("... FAILED") {
            t.saw(0, 1);
            continue;
        }
        // cargo: "running 5 tests" names the runner but counts nothing itself.
        if let Some(rest) = line.strip_prefix("running ") {
            let n = rest.strip_suffix(" tests").or_else(|| rest.strip_suffix(" test"));
            if n.is_some_and(|n| n.parse::<u64>().is_ok()) {
                t.recognised = true;
            }
            continue;
        }

        // node --test, TAP ("# pass 3") and the spec reporter of Node 23+ ("ℹ pass 3").
        for marker in ["# ", "ℹ "] {
            if let Some(n) = count_after(line, &format!("{marker}pass ")) {
                t.saw(n, 0);
            } else if let Some(n) = count_after(line, &format!("{marker}fail ")) {
                t.saw(0, n);
            } else if count_after(line, &format!("{marker}tests ")).is_some() {
                t.recognised = true;
            }
        }

        // go test: "--- PASS: T", "--- FAIL: T", "ok  \tpkg\t0.01s", "FAIL\tpkg", the
        // bare "FAIL" summary, and the zero-test markers.
        if line.starts_with("--- PASS") {
            t.saw(1, 0);
            continue;
        }
        if line.starts_with("--- FAIL") || line == "FAIL" || line.starts_with("FAIL\t") {
            t.saw(0, 1);
            continue;
        }
        if line.contains("[no test files]") || line.contains("[no tests to run]") {
            t.recognised = true;
            continue;
        }
        if line.starts_with("ok  \t") || line.starts_with("ok\t") {
            t.saw(1, 0);
            continue;
        }

        // bun test: " 3 pass", " 0 fail", "Ran 3 tests across 1 files."
        if let Some(n) = line.strip_suffix(" pass").and_then(|n| n.parse::<u64>().ok()) {
            t.saw(n, 0);
            continue;
        }
        if let Some(n) = line.strip_suffix(" fail").and_then(|n| n.parse::<u64>().ok()) {
            t.saw(0, n);
            continue;
        }

        // hexa spec verify: "6 passed · 0 failed · 0 not proven here · 0 pending".
        if line.contains(" passed · ") {
            let passed = count_before(line, "passed").unwrap_or(0);
            let failed = count_before(line, "failed").unwrap_or(0);
            t.saw(passed, failed);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multi_suite_cargo_run_sums_its_suites() {
        let out = "running 3 tests\ntest result: ok. 3 passed; 0 failed\n\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed\n";
        assert_eq!(read_evidence(true, out, false), Evidence::Passed { tests: 3 });
    }

    #[test]
    fn a_go_package_without_tests_alone_is_vacuous() {
        assert_eq!(read_evidence(true, "?   \tdemo/internal/ports\t[no test files]", false), Evidence::Vacuous);
    }

    #[test]
    fn a_cargo_test_line_marked_failed_fails_the_run() {
        let out = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n";
        assert!(matches!(read_evidence(true, out, false), Evidence::Failed { .. }));
    }
}
