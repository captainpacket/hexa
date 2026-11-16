//! What `hexa eval` scores against (ADR-2611160910).
//!
//! The rules are the domain's (`domain::usage_eval`); the use case reaches them here.

pub use crate::domain::usage_eval::{
    bash_writes_source, parse_transcript, score, Action, Expect, ExpectHit, Forbid, RunResult, Scenario, Score,
    Step, Transcript, DEFAULT_GATE_PATTERN,
};
