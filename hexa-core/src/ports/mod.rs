// `inference.rs` declares the inference trait surface.
// Where AG-UI events go (ADR-2611161830).
pub mod agui;
pub mod inference;
// What an editing adapter may not touch (the domain's critical-path rule).
pub mod edit;
// The spec an ADR compiles to, and its coverage rule (ADR-2610221540).
pub mod spec;
// Which runner a scenario's test names, and where tests run (ADR-2611191120).
pub mod test_runner;
// Which build a binary is, and whether it is behind (ADR-2610270853).
pub mod project_floor;
pub mod provenance;
// The repository's forge: remote mapping, issue and PR texts, the secret scan (ADR-2611031914).
pub mod forge;
// Whether every ADR's gate passes, given what is declared unbuilt (ADR-2610271110).
pub mod gate_ledger;
// What a gate's run proved, read by one fail-closed reader (ADR-2611081151).
pub mod evidence;
// Which hexa verb a plain-English request names (ADR-2610271308).
pub mod guide;
pub mod intent;
// What hexa needs from the Claude CLI to install its mods (ADR-2610301129).
pub mod claude_mods;
// The task ledger and the lease rules agents are held to (ADR-2611051850).
pub mod ledger;
// What an ADR says it has built: units, evidence, state, drift (ADR-2611151030).
pub mod adr_board;
// Whether a native build script is keyed the same under every cargo command (ADR-2611151330).
pub mod build_keys;
// State port contract (IStatePort + focused sub-traits + DTOs). Relocated from
// hexa-nexus where it was an anomaly — port traits belong in hexa-core with the
// rest (ADR-2606071340 P1). Implemented by the STDB/SQLite adapters.
// How a model used hexa, scored from a transcript (ADR-2611160910).
pub mod usage_eval;
