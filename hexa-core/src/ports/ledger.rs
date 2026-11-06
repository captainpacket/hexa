//! What the ledger adapters (the `hexa agent` verbs, the pre-edit hook, the
//! `hexa do` staging) are bound by (ADR-2611051850).
//!
//! The rules are the domain's (`domain::ledger`); they reach them here.

pub use crate::domain::ledger::{
    covers, find_overlap, is_credential_path, is_strict_credential_path, lease_decision,
    never_leasable, normalize_lease, overlaps, Decision, Ledger, LedgerError, Task, TaskStatus,
};
