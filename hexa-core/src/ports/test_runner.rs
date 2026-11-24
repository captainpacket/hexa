//! Which runner a scenario's test names, and where tests run (ADR-2611191120).
//!
//! The rules are the domain's (`domain::test_runner`). Every adapter that runs
//! a scenario's test or a gate (`spec verify`, `spec red`, `loop stage done`,
//! `do gate`, `land`) is bound by them, so they are part of their contract.

pub use crate::domain::test_runner::{
    batch_script, check_remote_template, fill, is_test_file, judge_counted, judge_script, judge_vitest, parse_test_ref,
    remote_command, runner_for, runner_ran, shell_quote, split_batch, vitest_pattern, Runner, TestRef, Verdict,
};
