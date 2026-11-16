//! Value objects shared across the workspace. Pure data, no I/O.

pub mod adr_board;
pub mod api_optimization;
// Which steps install hexa's Claude Code mods (ADR-2610301129).
pub mod claude_mods;
pub mod build_keys;
pub mod forge;
pub mod gate_ledger;
pub mod guide;
pub mod intent;
pub mod design;
// What a gate's run proved, read by one fail-closed reader (ADR-2611081151).
pub mod evidence;
pub mod ledger;
pub mod messages;
pub mod project_floor;
pub mod provenance;
pub mod spec;
pub mod tools;
pub mod workflow;
pub mod validation;
// How a model used hexa, scored from a transcript (ADR-2611160910).
pub mod usage_eval;
