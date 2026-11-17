//! Where AG-UI events go (ADR-2611161830 §8). The domain builds and orders
//! the events; a sink only writes the lines it is given. The domain types are
//! re-exported so an adapter imports the port, not the domain.

pub use crate::domain::agui::{run_started, Event, Outcome, Stream, SubagentOutcome, TokenUsage, Violation, PROTOCOL_VERSION};

/// Writes one framed event: a single JSON object, one line, flushed.
pub trait EventSink: Send {
    fn line(&mut self, line: &str) -> std::io::Result<()>;
}
