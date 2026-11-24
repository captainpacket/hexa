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

/// How many tests a run's output reports, by the runners this reader knows
/// (ADR-2611231400 §1). `recognised` is false when no line came from one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestCounts {
    pub passed: u64,
    pub failed: u64,
    pub recognised: bool,
}

/// The tests `output` reports passed and failed (ADR-2611231400 §1).
pub fn count_tests(output: &str) -> TestCounts {
    let t = tally(output);
    TestCounts { passed: t.passed, failed: t.failed, recognised: t.recognised }
}

/// The first line of `output` that names a failure, trimmed: a failed test
/// (`test x ... FAILED`, `--- FAIL`, `not ok`, vitest's `×`/`FAIL`), a build
/// error, or a panic (ADR-2611231400 §1). `None` when no line does.
pub fn first_failure(output: &str) -> Option<String> {
    output.lines().map(str::trim).find(|l| is_failure_line(l)).map(str::to_string)
}

fn is_failure_line(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    (line.starts_with("test ") && line.ends_with("... FAILED"))
        || line.starts_with("--- FAIL")
        || line.starts_with("not ok ")
        || line.starts_with("FAIL ")
        || line.starts_with("FAIL\t")
        || line.starts_with("× ")
        || line.starts_with("✗ ")
        || line.contains("panicked at")
        || is_build_error_line(line)
}

/// A line a compiler or a bundler writes for a build that did not finish
/// (ADR-2611231400 §4): rustc's `error[E…]`, `error: could not compile` and
/// `error: aborting due to`, TypeScript's `error TS…`, vitest's `Failed to
/// load` and `Transform failed`.
fn is_build_error_line(line: &str) -> bool {
    let l = line.trim();
    l.starts_with("error[E")
        || l.starts_with("error: could not compile")
        || l.starts_with("error: aborting due to")
        || l.contains(": error TS")
        || l.starts_with("error TS")
        || l.contains("Failed to load url")
        || l.starts_with("Error: Failed to load")
        || l.contains("Transform failed")
}

/// What a gate expected to fail proved (ADR-2611231400 §4). Only `Red` commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedEvidence {
    /// Exited non-zero, no build error, and a known runner counted ≥ 1 failed test.
    Red { passed: u64, failed: u64 },
    /// The gate passed: tests written first must fail before the code.
    NotRed,
    /// The gate failed on a build or compile error: not a red test.
    BuildError { line: String },
    /// The gate failed and no runner this reader knows counted a failed test.
    Uncounted,
    /// A runner this reader knows reported zero tests.
    Vacuous,
}

/// Read a gate that is expected to fail, by its command and output
/// (ADR-2611231400 §4). A build error wins over counted failures: a red that
/// is half a compile error hides which tests are red.
pub fn read_red_evidence(cmd: &str, exit_ok: bool, output: &str) -> RedEvidence {
    if let Some(line) = output.lines().find(|l| is_build_error_line(l)) {
        return RedEvidence::BuildError { line: line.trim().to_string() };
    }
    let t = tally(output);
    if exit_ok && t.failed == 0 {
        return match read_gate_evidence(cmd, true, output, false) {
            Evidence::Vacuous => RedEvidence::Vacuous,
            _ => RedEvidence::NotRed,
        };
    }
    if t.failed > 0 {
        return RedEvidence::Red { passed: t.passed, failed: t.failed };
    }
    if t.recognised && t.passed == 0 {
        return RedEvidence::Vacuous;
    }
    RedEvidence::Uncounted
}

#[derive(Default)]
struct Tally {
    recognised: bool,
    passed: u64,
    failed: u64,
    /// Per-test failure lines (`test x ... FAILED`, `--- FAIL`): the count
    /// only when no summary counted failures, so one failure is not two
    /// (ADR-2611231400 §1).
    failed_lines: u64,
    /// Package-level failure markers (`FAIL\tpkg`, a bare `FAIL`): one
    /// failure when nothing else counted any.
    fail_marks: u64,
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
            t.recognised = true;
            t.failed_lines += 1;
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
        if line.starts_with("--- FAIL") {
            t.recognised = true;
            t.failed_lines += 1;
            continue;
        }
        if line == "FAIL" || line.starts_with("FAIL\t") {
            t.recognised = true;
            t.fail_marks += 1;
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
    if t.failed == 0 {
        t.failed = if t.failed_lines > 0 { t.failed_lines } else { t.fail_marks.min(1) };
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

    const RED: &str = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\n";

    #[test]
    fn assertion_failures_are_red() {
        assert_eq!(read_red_evidence("cargo test", false, RED), RedEvidence::Red { passed: 1, failed: 1 });
        let vitest = " Test Files  1 failed (1)\n      Tests  2 failed | 3 passed (5)\n";
        assert_eq!(read_red_evidence("npx vitest run", false, vitest), RedEvidence::Red { passed: 3, failed: 2 });
    }

    #[test]
    fn a_compile_error_is_not_red_even_beside_failures() {
        let out = "error[E0425]: cannot find function `x`\nerror: could not compile `p` (test \"t\")\n";
        assert!(matches!(read_red_evidence("cargo test", false, out), RedEvidence::BuildError { .. }));
        let mixed = format!("{RED}error: could not compile `q`\n");
        assert!(matches!(read_red_evidence("cargo test", false, &mixed), RedEvidence::BuildError { .. }), "a build error wins");
        let ts = "src/a.test.ts(3,1): error TS2304: Cannot find name 'x'.\n";
        assert!(matches!(read_red_evidence("npx tsc", false, ts), RedEvidence::BuildError { .. }));
    }

    #[test]
    fn green_zero_and_uncounted_are_not_red() {
        let green = "running 1 test\ntest result: ok. 1 passed; 0 failed\n";
        assert_eq!(read_red_evidence("cargo test", true, green), RedEvidence::NotRed);
        assert_eq!(read_red_evidence("bash ci/x.sh", true, "fine\n"), RedEvidence::NotRed, "one script, exit 0, is a pass");
        let zero = "running 0 tests\ntest result: ok. 0 passed; 0 failed\n";
        assert_eq!(read_red_evidence("cargo test", true, zero), RedEvidence::Vacuous);
        assert_eq!(read_red_evidence("cargo test", false, zero), RedEvidence::Vacuous);
        assert_eq!(read_red_evidence("bash ci/x.sh", false, "boom\n"), RedEvidence::Uncounted);
    }

    #[test]
    fn counts_and_the_first_failure_are_read_from_the_output() {
        assert_eq!(count_tests(RED), TestCounts { passed: 1, failed: 1, recognised: true });
        assert_eq!(count_tests("hello\n"), TestCounts::default());
        assert_eq!(first_failure(RED).as_deref(), Some("test b ... FAILED"));
        assert_eq!(first_failure("all fine\n"), None);
        assert_eq!(first_failure("x\nthread 'a' panicked at src/a.rs:1:1:\n").as_deref(), Some("thread 'a' panicked at src/a.rs:1:1:"));
    }

    // ── ADR-2611240900 §2: cargo-nextest, and colour ─────────────────────────
    // Real output: cargo-nextest 0.9.152 and vitest 2.1.9, captured on max,
    // 2026-11-24, from a crate with two passing, one failing and one ignored
    // test, and a package with two passing tests and one failing.

    const NX_GREEN: &str = include_str!("evidence_samples/nextest-green.txt");
    const NX_RED: &str = include_str!("evidence_samples/nextest-red.txt");
    const NX_RED_COLOUR: &str = include_str!("evidence_samples/nextest-red-colour.txt");
    const NX_ZERO: &str = include_str!("evidence_samples/nextest-zero.txt");
    const NX_ZERO_PASS: &str = include_str!("evidence_samples/nextest-zero-pass.txt");
    const VT_GREEN_COLOUR: &str = include_str!("evidence_samples/vitest-green-colour.txt");
    const VT_RED_COLOUR: &str = include_str!("evidence_samples/vitest-red-colour.txt");

    /// The sample with every escape sequence taken out by hand, independently
    /// of the reader: ESC, `[`, parameters, one final byte.
    fn without_colour(s: &str) -> String {
        let mut out = String::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\u{1b}' && it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    /// S06: A real cargo-nextest run is counted from its summary
    #[test]
    fn s06_a_real_nextest_run_is_counted_from_its_summary() {
        assert_eq!(read_evidence(true, NX_GREEN, false), Evidence::Passed { tests: 2 }, "green: 2 passed, 2 skipped");
        assert_eq!(count_tests(NX_RED), TestCounts { passed: 2, failed: 1, recognised: true }, "the summary, not the libtest lines inside it");
        assert!(matches!(read_evidence(false, NX_RED, false), Evidence::Failed { .. }));
        assert!(matches!(read_evidence(true, NX_RED, false), Evidence::Failed { .. }), "a counted failure fails even on exit 0");
        assert_eq!(read_red_evidence("cargo nextest run", false, NX_RED), RedEvidence::Red { passed: 2, failed: 1 });
        assert_eq!(first_failure(NX_RED).as_deref(), Some("FAIL [   0.002s] (2/3) sample tests::breaks"));
        assert_eq!(read_gate_evidence("cargo nextest run", true, NX_GREEN, false), Evidence::Passed { tests: 2 });
    }

    /// S07: Coloured output reads as the same output without colour
    #[test]
    fn s07_coloured_output_reads_as_without_colour() {
        assert!(VT_GREEN_COLOUR.contains('\u{1b}') && NX_RED_COLOUR.contains('\u{1b}'), "the samples carry colour");
        assert_eq!(read_evidence(true, VT_GREEN_COLOUR, false), Evidence::Passed { tests: 2 }, "coloured vitest that passed");
        assert_eq!(count_tests(VT_RED_COLOUR), TestCounts { passed: 2, failed: 1, recognised: true });
        assert_eq!(read_red_evidence("npx vitest run", false, VT_RED_COLOUR), RedEvidence::Red { passed: 2, failed: 1 });
        assert_eq!(count_tests(NX_RED_COLOUR), TestCounts { passed: 2, failed: 1, recognised: true });
        for s in [VT_GREEN_COLOUR, VT_RED_COLOUR, NX_RED_COLOUR] {
            let plain = without_colour(s);
            assert_eq!(count_tests(s), count_tests(&plain));
            assert_eq!(read_evidence(true, s, false), read_evidence(true, &plain, false));
            assert_eq!(first_failure(s), first_failure(&plain));
            assert!(first_failure(s).is_none_or(|l| !l.contains('\u{1b}')), "the first failure is shown without codes");
        }
        assert_eq!(first_failure(VT_RED_COLOUR).as_deref(), Some("× breaks 3ms"));
    }

    /// S08: A nextest run of zero tests is vacuous, and a cut-off run counts each test once
    #[test]
    fn s08_zero_nextest_tests_are_vacuous_and_a_cut_run_counts_once() {
        assert_eq!(read_evidence(true, NX_ZERO_PASS, false), Evidence::Vacuous, "--no-tests=pass, exit 0");
        assert!(matches!(read_evidence(false, NX_ZERO, false), Evidence::Failed { .. }), "exit 4");
        assert_eq!(read_red_evidence("cargo nextest run", false, NX_ZERO), RedEvidence::Vacuous);
        assert_eq!(read_red_evidence("cargo nextest run", true, NX_ZERO_PASS), RedEvidence::Vacuous);
        let starting_only = "    Starting 0 tests across 1 binary (4 tests skipped)\n";
        assert_eq!(read_evidence(true, starting_only, false), Evidence::Vacuous);
        // Cut before the summary: the FAIL line is printed twice, the test failed once.
        let cut = "    Starting 3 tests across 1 binary\n        PASS [   0.002s] (1/3) s t::a\n        FAIL [   0.002s] (2/3) s t::b\n\
                   \n        PASS [   0.003s] (3/3) s t::c\n        FAIL [   0.002s] (2/3) s t::b\n";
        assert_eq!(count_tests(cut), TestCounts { passed: 2, failed: 1, recognised: true });
        let timed_out = "     Summary [   1.0s] 4 tests run: 2 passed (1 flaky), 1 failed, 1 timed out, 0 skipped\n";
        assert_eq!(count_tests(timed_out), TestCounts { passed: 2, failed: 2, recognised: true });
        let cancelled = "     Summary [   1.0s] 2/5 tests run: 1 passed, 1 failed, 0 skipped\n";
        assert_eq!(count_tests(cancelled), TestCounts { passed: 1, failed: 1, recognised: true });
        // nextest beside vitest sums both.
        let both = format!("{NX_GREEN}\n Test Files  1 passed (1)\n      Tests  3 passed (3)\n");
        assert_eq!(read_evidence(true, &both, false), Evidence::Passed { tests: 5 });
    }

    #[test]
    fn a_cargo_test_line_marked_failed_fails_the_run() {
        let out = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n";
        assert!(matches!(read_evidence(true, out, false), Evidence::Failed { .. }));
    }
}
