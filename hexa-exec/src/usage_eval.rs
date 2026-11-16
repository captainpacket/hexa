//! `hexa eval`'s use case: read a scenario, score a transcript (ADR-2611160910).
//!
//! The scoring rules are the domain's (`hexa_core::domain::usage_eval`). This
//! file supplies what the domain does not carry: TOML, and a regex engine.
//! Every regex a scenario names is compiled before anything is scored, so a
//! bad pattern is an error rather than a pattern that silently never matches.

use std::collections::HashMap;

use regex::Regex;

pub use hexa_core::ports::usage_eval::{
    parse_transcript, Action, Expect, ExpectHit, Forbid, RunResult, Scenario, Score, Step, Transcript,
};

/// Parse and validate `scenario.toml`, and compile every pattern it names.
pub fn parse_scenario(text: &str) -> Result<Scenario, String> {
    let s: Scenario = toml::from_str(text).map_err(|e| format!("scenario.toml: {e}"))?;
    s.validate()?;
    compile(&s)?;
    Ok(s)
}

fn compile(s: &Scenario) -> Result<HashMap<String, Regex>, String> {
    let mut out = HashMap::new();
    for p in s.patterns() {
        if !out.contains_key(p) {
            let re = Regex::new(p).map_err(|e| format!("pattern {p:?}: {e}"))?;
            out.insert(p.to_string(), re);
        }
    }
    Ok(out)
}

/// Score a stream-json transcript against a scenario. `outcome` is the
/// verdict of the scenario's outcome command, or `None` when it was not run.
pub fn score_transcript(s: &Scenario, transcript: &str, outcome: Option<bool>) -> Result<Score, String> {
    let res = compile(s)?;
    let t = parse_transcript(transcript)?;
    let is_match = |p: &str, text: &str| res.get(p).is_some_and(|re| re.is_match(text));
    Ok(hexa_core::ports::usage_eval::score(s, &t, outcome, &is_match))
}
