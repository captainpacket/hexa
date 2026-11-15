//! What `hexa dev build-keys` is bound by (ADR-2611151330).
//!
//! The rules are the domain's (`domain::build_keys`); the adapter reaches them here.

pub use crate::domain::build_keys::{analyze, label, Difference, Report, Script, Summary};
