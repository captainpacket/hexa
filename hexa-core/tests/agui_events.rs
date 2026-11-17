//! hexa's AG-UI events, checked against the protocol's own schema
//! (ADR-2611161830 §9), and the ordering rules the schema cannot say.

use hexa_core::domain::agui::{run_started, Event, Outcome, Stream, SubagentOutcome, TokenUsage, Violation, PROTOCOL_VERSION};
use serde_json::{json, Map, Value};

fn validator() -> jsonschema::Validator {
    let raw = include_str!("fixtures/ag-ui/schema-draft-1.1.json");
    let schema: Value = serde_json::from_str(raw).unwrap();
    jsonschema::draft202012::new(&schema).expect("the vendored schema compiles")
}

fn valid(v: &jsonschema::Validator, line: &str) {
    let doc: Value = serde_json::from_str(line).unwrap();
    let errors: Vec<String> = v.iter_errors(&doc).map(|e| format!("{e} at {}", e.instance_path)).collect();
    assert!(errors.is_empty(), "{line}\n{}", errors.join("\n"));
}

fn every_event() -> Vec<Event> {
    let mut content = Map::new();
    content.insert("open".into(), json!([{ "kind": "task", "what": "[ ] 1 x", "fix": "hexa loop task done 1" }]));
    vec![
        run_started("thread-1", "run-1"),
        Event::StateSnapshot { snapshot: json!({ "adr": "ADR-1", "stage": "build", "checklist": [] }), subagent_run_id: None },
        Event::StepStarted { step_name: "gate".into(), subagent_run_id: None },
        Event::StateDelta { delta: vec![json!({ "op": "replace", "path": "/stage", "value": "harden" })], subagent_run_id: None },
        Event::ActivitySnapshot { message_id: "act-1".into(), activity_type: "hexa.open_work".into(), content, replace: None, subagent_run_id: None },
        Event::ActivityDelta { message_id: "act-1".into(), activity_type: "hexa.open_work".into(), patch: vec![json!({ "op": "remove", "path": "/open/0" })], subagent_run_id: None },
        Event::SubagentStarted { subagent_run_id: "sa-1".into(), name: "reviewer".into(), description: Some("red-teams the design".into()), parent_subagent_run_id: None },
        Event::StepStarted { step_name: "review".into(), subagent_run_id: Some("sa-1".into()) },
        Event::StepFinished { step_name: "review".into(), subagent_run_id: Some("sa-1".into()) },
        Event::SubagentFinished { subagent_run_id: "sa-1".into(), result: Some(json!({ "findings": 2 })), outcome: Some(SubagentOutcome::Success) },
        Event::SubagentStarted { subagent_run_id: "sa-2".into(), name: "candidate".into(), description: None, parent_subagent_run_id: None },
        Event::SubagentError { subagent_run_id: "sa-2".into(), message: "it did not build".into(), code: Some("build_failed".into()) },
        Event::Custom { name: "hexa.note".into(), value: Value::Null, subagent_run_id: None },
        Event::StepFinished { step_name: "gate".into(), subagent_run_id: None },
        Event::RunFinished {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            result: Some(json!({ "committed": "abc1234" })),
            outcome: Some(Outcome::Success),
            usage: Some(vec![TokenUsage { provider: Some("anthropic".into()), model: Some("m".into()), input_tokens: Some(10), output_tokens: Some(5), total_tokens: Some(15), cached_input_tokens: Some(4), ..Default::default() }]),
        },
    ]
}

#[test]
fn agui_every_event_hexa_builds_is_valid_against_the_protocol_schema() {
    let v = validator();
    let mut s = Stream::new();
    for e in every_event() {
        let line = s.emit(e).unwrap();
        valid(&v, &line);
    }
    let mut f = Stream::new();
    for line in f.fail("could not reach the gate", Some("refused"), None) {
        valid(&v, &line);
    }
    let mut c = Stream::new();
    c.emit(run_started("t", "r")).unwrap();
    for line in c.finish(None, Some(Outcome::Cancelled), None) {
        valid(&v, &line);
    }
}

#[test]
fn agui_a_run_declares_the_protocol_version_it_speaks() {
    let mut s = Stream::without_timestamps();
    let line = s.emit(run_started("t", "r")).unwrap();
    let doc: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(doc["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(doc["type"], "RUN_STARTED");
}

#[test]
fn agui_an_optional_field_with_no_value_is_absent_never_null() {
    let mut s = Stream::without_timestamps();
    s.emit(Event::RunStarted { thread_id: "t".into(), run_id: "r".into(), protocol_version: None, parent_run_id: None }).unwrap();
    let fin = s.finish(Some(Value::Null), None, None);
    let last = fin.last().unwrap();
    assert_eq!(last, r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r"}"#, "a null result is dropped, and nothing optional is sent");
    assert!(!last.contains("null"));
    let mut t = Stream::new();
    let started = t.emit(run_started("t", "r")).unwrap();
    assert!(!started.contains("null") && started.contains("\"timestamp\":"), "{started}");
}

#[test]
fn agui_a_stream_begins_with_its_run() {
    let mut s = Stream::new();
    assert_eq!(s.emit(Event::StepStarted { step_name: "gate".into(), subagent_run_id: None }), Err(Violation::BeforeRun("STEP_STARTED")));
    let mut f = Stream::new();
    assert!(f.emit(Event::RunError { message: "no agent".into(), code: None, usage: None }).is_ok(), "a run can fail before it begins");
}

#[test]
fn agui_steps_are_balanced_and_runs_do_not_nest() {
    let mut s = Stream::new();
    s.emit(run_started("t", "r")).unwrap();
    assert_eq!(s.emit(run_started("t", "r2")), Err(Violation::NestedRun));
    s.emit(Event::StepStarted { step_name: "gate".into(), subagent_run_id: None }).unwrap();
    assert_eq!(s.emit(Event::StepStarted { step_name: "gate".into(), subagent_run_id: None }), Err(Violation::StepOpen("gate".into())));
    assert_eq!(s.emit(Event::StepFinished { step_name: "land".into(), subagent_run_id: None }), Err(Violation::StepNotOpen("land".into())));
    let early = s.emit(Event::RunFinished { thread_id: "t".into(), run_id: "r".into(), result: None, outcome: None, usage: None });
    assert!(matches!(early, Err(Violation::StillOpen(_))), "{early:?}");
    assert!(matches!(s.emit(Event::ActivityDelta { message_id: "nope".into(), activity_type: "x".into(), patch: vec![], subagent_run_id: None }), Err(Violation::NoActivity(_))));
}

#[test]
fn agui_nothing_follows_a_closed_run_but_a_new_run_or_a_late_failure() {
    let mut s = Stream::new();
    s.emit(run_started("t", "r")).unwrap();
    s.finish(None, None, None);
    assert_eq!(s.emit(Event::StepStarted { step_name: "x".into(), subagent_run_id: None }), Err(Violation::AfterClose("STEP_STARTED")));
    assert!(s.emit(Event::RunError { message: "flush failed".into(), code: None, usage: None }).is_ok(), "a late failure after success is allowed");
    assert_eq!(s.emit(Event::RunError { message: "again".into(), code: None, usage: None }), Err(Violation::AfterClose("RUN_ERROR")));
    assert!(s.emit(run_started("t", "r2")).is_ok(), "a new run may follow");
}

#[test]
fn agui_ending_a_run_closes_what_it_opened_first() {
    let v = validator();
    for failing in [false, true] {
        let mut s = Stream::without_timestamps();
        s.emit(run_started("t", "r")).unwrap();
        s.emit(Event::StepStarted { step_name: "gate".into(), subagent_run_id: None }).unwrap();
        s.emit(Event::StepStarted { step_name: "land".into(), subagent_run_id: None }).unwrap();
        s.emit(Event::SubagentStarted { subagent_run_id: "sa".into(), name: "reviewer".into(), description: None, parent_subagent_run_id: None }).unwrap();
        let lines = if failing { s.fail("the gate failed", Some("gate_failed"), None) } else { s.finish(None, None, None) };
        let types: Vec<String> = lines.iter().map(|l| serde_json::from_str::<Value>(l).unwrap()["type"].as_str().unwrap().to_string()).collect();
        let end = if failing { "RUN_ERROR" } else { "RUN_FINISHED" };
        assert_eq!(types, vec!["SUBAGENT_ERROR", "STEP_FINISHED", "STEP_FINISHED", end]);
        for l in &lines {
            valid(&v, l);
        }
        assert!(!s.active());
    }
}

#[test]
fn agui_the_oracle_rejects_what_the_protocol_forbids() {
    // a vacuous oracle is a failed gate: the schema must say no to known-bad events
    let v = validator();
    for bad in [
        r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","result":null}"#,
        r#"{"type":"RUN_STARTED","threadId":"t"}"#,
        r#"{"type":"STEP_STARTED","stepName":"x","extra":1}"#,
        r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"done"}}"#,
        r#"{"type":"NOT_AN_EVENT"}"#,
    ] {
        let doc: Value = serde_json::from_str(bad).unwrap();
        assert!(!v.is_valid(&doc), "the schema accepts {bad}");
    }
}
