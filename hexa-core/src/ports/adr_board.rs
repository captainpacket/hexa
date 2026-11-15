//! What `hexa adr board` is bound by (ADR-2611151030).
//!
//! The rules are the domain's (`domain::adr_board`); the adapter reaches them here.

pub use crate::domain::adr_board::{analyze, summarize, Analysis, Phase, State, Summary, Units};
