//! Agent orientation's contract (ADR-2611221600): the managed sections of
//! AGENTS.md and CLAUDE.md, where builds run, module READMEs and the graph's
//! state. Re-exported for the adapters, which may reach the domain only
//! through a port.

pub use crate::domain::orientation::{
    agents_md_section, append_managed, graph_state, imports_agents_md, is_current, managed_body,
    module_readme, replace_managed, where_builds_run, wrap, CheckState, GraphState, Layer,
    MarkerError, Markers, OrientationCheck, AGENTS_IMPORT, AGENTS_MD, CLAUDE_MD,
};
