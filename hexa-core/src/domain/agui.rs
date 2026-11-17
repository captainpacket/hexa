//! AG-UI events and the rules a producer keeps (ADR-2611161830).
//!
//! hexa reports a verb's work as an AG-UI 1.1 (draft) event stream: one run,
//! its phases as steps, the loop as state, progress as activity, and the
//! agents it starts as subagents. The shapes are the protocol schema's; this
//! module builds them and keeps the ordering rules the schema cannot say.
//! Pure: it turns events into lines and refuses a stream that would break the
//! protocol. Writing the lines is an adapter's job.

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The protocol version hexa speaks, declared on every `RUN_STARTED`. The
/// draft's version until a frozen one is published (ADR-2611161830 §9).
pub const PROTOCOL_VERSION: &str = "1.1";

/// How a run that did not fail ended. Absent on the wire means success.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Cancelled,
}

/// How a subagent's segment ended. Absent on the wire means success.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SubagentOutcome {
    Success,
}

/// Token counts for one provider and model, in the protocol's accounting:
/// `input_tokens` and `output_tokens` are totals; reasoning and cache counts
/// are parts of them. A count the provider did not report stays `None`, never 0.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u64>,
}

/// The events hexa emits: the run lifecycle, steps, state, activity,
/// subagents and custom. Text messages, tool calls and reasoning are the
/// agent's own to report, not hexa's (ADR-2611161830, Consequences).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum Event {
    RunStarted {
        thread_id: String,
        run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_run_id: Option<String>,
    },
    RunFinished {
        thread_id: String,
        run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<Outcome>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Vec<TokenUsage>>,
    },
    RunError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Vec<TokenUsage>>,
    },
    StepStarted {
        step_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
    StepFinished {
        step_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
    StateSnapshot {
        snapshot: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
    StateDelta {
        delta: Vec<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
    ActivitySnapshot {
        message_id: String,
        activity_type: String,
        content: Map<String, Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        replace: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
    ActivityDelta {
        message_id: String,
        activity_type: String,
        patch: Vec<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
    SubagentStarted {
        subagent_run_id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_subagent_run_id: Option<String>,
    },
    SubagentFinished {
        subagent_run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<SubagentOutcome>,
    },
    SubagentError {
        subagent_run_id: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    Custom {
        name: String,
        value: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_run_id: Option<String>,
    },
}

/// One event on the wire: the event and when it was made (milliseconds since
/// the epoch, informational only: arrival order is the protocol's order).
#[derive(Debug, Clone, Serialize)]
struct Framed<'a> {
    #[serde(flatten)]
    event: &'a Event,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<i64>,
}

/// Why an event was refused: it would break the protocol's ordering rules.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Violation {
    #[error("a stream begins with RUN_STARTED or RUN_ERROR, not {0}")]
    BeforeRun(&'static str),
    #[error("the run is closed; only RUN_STARTED (or RUN_ERROR after RUN_FINISHED) may follow, not {0}")]
    AfterClose(&'static str),
    #[error("a run is already active; close it before starting another")]
    NestedRun,
    #[error("step {0:?} is already open")]
    StepOpen(String),
    #[error("step {0:?} was never opened")]
    StepNotOpen(String),
    #[error("subagent {0:?} is already active")]
    SubagentActive(String),
    #[error("subagent {0:?} is not active")]
    SubagentNotActive(String),
    #[error("RUN_FINISHED with {0} still open; close it first")]
    StillOpen(String),
    #[error("an activity delta for {0:?}, which no snapshot created")]
    NoActivity(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Active,
    Finished,
    Failed,
}

/// A producer's stream: it admits an event only if the protocol allows it
/// next, and frames it as one JSON line.
#[derive(Debug, Clone)]
pub struct Stream {
    phase: Phase,
    thread_id: String,
    run_id: String,
    steps: BTreeSet<String>,
    subagents: BTreeSet<String>,
    activities: BTreeSet<String>,
    clock: fn() -> Option<i64>,
}

fn now_ms() -> Option<i64> {
    Some(chrono::Utc::now().timestamp_millis())
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}

impl Stream {
    pub fn new() -> Self {
        Stream {
            phase: Phase::Idle,
            thread_id: String::new(),
            run_id: String::new(),
            steps: BTreeSet::new(),
            subagents: BTreeSet::new(),
            activities: BTreeSet::new(),
            clock: now_ms,
        }
    }

    /// A stream whose events carry no timestamp, for byte-stable tests.
    pub fn without_timestamps() -> Self {
        Stream { clock: || None, ..Self::new() }
    }

    /// True between `RUN_STARTED` and the run's terminal event.
    pub fn active(&self) -> bool {
        self.phase == Phase::Active
    }

    /// The steps open now, in name order.
    pub fn open_steps(&self) -> Vec<String> {
        self.steps.iter().cloned().collect()
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Admit `event` and return its line (no trailing newline), or refuse it.
    /// A `null` given for an optional JSON field is dropped, never sent:
    /// absent means absent.
    pub fn emit(&mut self, event: Event) -> Result<String, Violation> {
        let event = absent_not_null(event);
        self.admit(&event)?;
        let line = serde_json::to_string(&Framed { event: &event, timestamp: (self.clock)() }).expect("events serialize");
        Ok(line)
    }

    fn admit(&mut self, e: &Event) -> Result<(), Violation> {
        let name = type_name(e);
        match (&self.phase, e) {
            (Phase::Active, Event::RunStarted { .. }) => return Err(Violation::NestedRun),
            (_, Event::RunStarted { thread_id, run_id, .. }) => {
                self.phase = Phase::Active;
                self.thread_id = thread_id.clone();
                self.run_id = run_id.clone();
                self.steps.clear();
                self.subagents.clear();
                self.activities.clear();
                return Ok(());
            }
            (Phase::Idle | Phase::Active | Phase::Finished, Event::RunError { .. }) => {
                self.phase = Phase::Failed;
                return Ok(());
            }
            (Phase::Idle, _) => return Err(Violation::BeforeRun(name)),
            (Phase::Finished | Phase::Failed, _) => return Err(Violation::AfterClose(name)),
            (Phase::Active, _) => {}
        }
        match e {
            Event::RunFinished { .. } => {
                if let Some(s) = self.steps.iter().next() {
                    return Err(Violation::StillOpen(format!("step {s:?}")));
                }
                if let Some(s) = self.subagents.iter().next() {
                    return Err(Violation::StillOpen(format!("subagent {s:?}")));
                }
                self.phase = Phase::Finished;
            }
            Event::StepStarted { step_name, .. } => {
                if !self.steps.insert(step_name.clone()) {
                    return Err(Violation::StepOpen(step_name.clone()));
                }
            }
            Event::StepFinished { step_name, .. } => {
                if !self.steps.remove(step_name) {
                    return Err(Violation::StepNotOpen(step_name.clone()));
                }
            }
            Event::SubagentStarted { subagent_run_id, .. } => {
                if !self.subagents.insert(subagent_run_id.clone()) {
                    return Err(Violation::SubagentActive(subagent_run_id.clone()));
                }
            }
            Event::SubagentFinished { subagent_run_id, .. } | Event::SubagentError { subagent_run_id, .. } => {
                if !self.subagents.remove(subagent_run_id) {
                    return Err(Violation::SubagentNotActive(subagent_run_id.clone()));
                }
            }
            Event::ActivitySnapshot { message_id, .. } => {
                self.activities.insert(message_id.clone());
            }
            Event::ActivityDelta { message_id, .. } if !self.activities.contains(message_id) => {
                return Err(Violation::NoActivity(message_id.clone()));
            }
            _ => {}
        }
        Ok(())
    }

    /// The events that end the run well: every open step and subagent closed,
    /// then `RUN_FINISHED`. Lines, in order; empty when no run is active.
    pub fn finish(&mut self, result: Option<Value>, outcome: Option<Outcome>, usage: Option<Vec<TokenUsage>>) -> Vec<String> {
        if !self.active() {
            return Vec::new();
        }
        let mut out = self.close_open();
        let (thread_id, run_id) = (self.thread_id.clone(), self.run_id.clone());
        out.extend(self.emit(Event::RunFinished { thread_id, run_id, result, outcome, usage }).ok());
        out
    }

    /// The events that end the run as failed: open steps and subagents
    /// closed, then `RUN_ERROR`. A stream that never started fails with
    /// `RUN_ERROR` alone, which the protocol allows first.
    pub fn fail(&mut self, message: &str, code: Option<&str>, usage: Option<Vec<TokenUsage>>) -> Vec<String> {
        if matches!(self.phase, Phase::Failed) {
            return Vec::new();
        }
        let mut out = if self.active() { self.close_open() } else { Vec::new() };
        out.extend(self.emit(Event::RunError { message: message.to_string(), code: code.map(String::from), usage }).ok());
        out
    }

    fn close_open(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for id in self.subagents.clone() {
            out.extend(self.emit(Event::SubagentError { subagent_run_id: id, message: "the run ended before this subagent did".into(), code: Some("run_ended".into()) }).ok());
        }
        for step in self.steps.clone().into_iter().rev() {
            out.extend(self.emit(Event::StepFinished { step_name: step, subagent_run_id: None }).ok());
        }
        out
    }
}

/// `Some(Value::Null)` in an optional JSON field is an absent field.
fn absent_not_null(e: Event) -> Event {
    let drop_null = |v: Option<Value>| v.filter(|x| !x.is_null());
    match e {
        Event::RunFinished { thread_id, run_id, result, outcome, usage } => {
            Event::RunFinished { thread_id, run_id, result: drop_null(result), outcome, usage }
        }
        Event::SubagentFinished { subagent_run_id, result, outcome } => {
            Event::SubagentFinished { subagent_run_id, result: drop_null(result), outcome }
        }
        other => other,
    }
}

fn type_name(e: &Event) -> &'static str {
    match e {
        Event::RunStarted { .. } => "RUN_STARTED",
        Event::RunFinished { .. } => "RUN_FINISHED",
        Event::RunError { .. } => "RUN_ERROR",
        Event::StepStarted { .. } => "STEP_STARTED",
        Event::StepFinished { .. } => "STEP_FINISHED",
        Event::StateSnapshot { .. } => "STATE_SNAPSHOT",
        Event::StateDelta { .. } => "STATE_DELTA",
        Event::ActivitySnapshot { .. } => "ACTIVITY_SNAPSHOT",
        Event::ActivityDelta { .. } => "ACTIVITY_DELTA",
        Event::SubagentStarted { .. } => "SUBAGENT_STARTED",
        Event::SubagentFinished { .. } => "SUBAGENT_FINISHED",
        Event::SubagentError { .. } => "SUBAGENT_ERROR",
        Event::Custom { .. } => "CUSTOM",
    }
}

/// The opening event of a hexa run.
pub fn run_started(thread_id: &str, run_id: &str) -> Event {
    Event::RunStarted { thread_id: thread_id.to_string(), run_id: run_id.to_string(), protocol_version: Some(PROTOCOL_VERSION.to_string()), parent_run_id: None }
}
