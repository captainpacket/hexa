//! One text completion, without a daemon in the path.
//!
//! `hexa-exec` used to POST `{model, messages, max_tokens}` to
//! `http://127.0.0.1:$HEXA_NEXUS_PORT/api/inference/complete` and read `{content}` back. The daemon
//! then called the provider. It added routing and logging to a call that already knew its model —
//! and it made the agent loop unable to run at all unless a control plane was up.
//!
//! This is that call as a function. Same inputs, same output, one process.
//!
//! The use case only: build the request, run it on whatever the [`Backends`] port provides,
//! record the spend, shape the reply. Which adapter serves a model is wiring, and lives in
//! [`crate::wiring`] — it was here, and made this use case import every adapter it might pick.

use hexa_core::domain::messages::{ContentBlock, Message, Role};
use hexa_core::domain::messages::StopReason;
use hexa_core::ports::inference::{InferenceRequest, Priority};

use crate::ports::{Backends, PromptCompressor};


/// Tools, or a real error — never a silent empty list.
///
/// This was `.ok().unwrap_or_default()`, which turned any shape mismatch into "no tools". A model
/// given no tools does not fail: it invents a `{"tool": ...}` JSON-in-text convention, which
/// `extract_tool_uses` cannot see, so the loop reports "ended with no edit" and the MODEL takes the
/// blame for a bridge that dropped its arguments on the floor.
fn parse_tools(v: &serde_json::Value) -> Result<Vec<hexa_core::domain::tools::ToolDefinition>, String> {
    if v.is_null() {
        return Ok(Vec::new());
    }
    serde_json::from_value(v.clone())
        .map_err(|e| format!("inference: tools did not match ToolDefinition ({e})"))
}

/// Send one system+user turn and return the text of the reply.
///
/// The return is `Result<String, String>` rather than a typed error because every caller in
/// `hexa-exec` already funnels failures into a string it shows the operator, and widening that here
/// would be a change to the loop rather than to inference.
pub async fn complete_text_with(
    backends: &dyn Backends,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, String> {
    complete_text_cached(backends, model, system, user, max_tokens, true).await
}

/// [`complete_text_with`], never from the cache and never into it: for a
/// reply that is judged after it returns, such as a spec author's or a test
/// writer's, which a retry must not be served again (ADR-2610281516 §2).
pub async fn complete_text_fresh_with(
    backends: &dyn Backends,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, String> {
    complete_text_cached(backends, model, system, user, max_tokens, false).await
}

async fn complete_text_cached(
    backends: &dyn Backends,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    use_cache: bool,
) -> Result<String, String> {
    let request = InferenceRequest {
        model: model.to_string(),
        system_prompt: system.to_string(),
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: user.to_string() }],
        }],
        tools: Vec::new(),
        max_tokens,
        // 0.0: this path asks for a precise edit or a structured reply, never for variety. It is
        // what the daemon route sent, kept rather than re-decided.
        temperature: 0.0,
        thinking_budget: None,
        cache_control: false,
        priority: Priority::default(),
        grammar: None,
    };

    // An identical tool-less completion is answered once (ADR-2610281516 §2).
    // `HEXA_NO_CACHE=1` (which `--fresh` sets) asks for a fresh one.
    let bypass = std::env::var("HEXA_NO_CACHE").is_ok_and(|v| !v.is_empty() && v != "0");
    let cache = if bypass || !use_cache { None } else { backends.cache() };
    let key = crate::ports::completion_key(model, system, user);
    if let Some(reply) = cache.and_then(|c| c.get(&key)) {
        backends.record_cache_hit(model);
        return Ok(reply);
    }

    let (response, usage) = backends.complete_with_usage(request).await.map_err(|e| e.to_string())?;

    backends.record_usage(model, &usage);

    // The daemon returned a flat `content` string. Concatenating the text blocks reproduces that
    // exactly for a reply with no tool use, which is all this path ever asks for.
    let text = text_or_truncation(&response.content, response.stop_reason, model, max_tokens)?;
    if let Some(c) = cache {
        c.put(&key, &text);
    }
    Ok(text)
}

/// The reply's text, or an error when there is none because the model ran
/// out of budget (ADR-2609131835 §1).
///
/// A reasoning model emits its thinking first and its answer after, into one
/// budget. When the budget covers only the thinking the reply carries no
/// content at all, and `Ok("")` makes that indistinguishable from a model
/// that answered with nothing.
fn text_or_truncation(
    content: &[ContentBlock],
    stop_reason: StopReason,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let text: String = content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    if text.trim().is_empty() && stop_reason == StopReason::MaxTokens {
        return Err(format!(
            "{model} produced no answer within {max_tokens} tokens — it stopped at the limit, \
             which a reasoning model does when the budget covers its thinking but not its reply; \
             raise it with HEXA_REVIEW_MAX_TOKENS"
        ));
    }
    Ok(text)
}


/// The daemon's `/api/inference/complete` contract, as a function.
///
/// Takes and returns the same JSON the HTTP route did, so the ReAct loop's request building and
/// its `extract_tool_uses` parsing are untouched. That is deliberate: a phase should move the
/// boundary, not rewrite what sits either side of it. `hexa-exec` is 9,000 lines of loop that
/// works; the only thing wrong with it was the hop.
///
/// Tolerant about message content on the way in — the loop sends `"content": "text"` for the seed
/// and block arrays for tool results, and the route accepted both.
pub async fn complete_raw_with(
    backends: &dyn Backends,
    compressor: &dyn PromptCompressor,
    req: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let model = req.get("model").and_then(|v| v.as_str()).unwrap_or_default();
    if model.is_empty() {
        return Err("inference: request has no model".into());
    }

    let mut messages = req
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(message_from_json).collect::<Vec<_>>())
        .unwrap_or_default();

    // One budget, one owner (ADR-2611050000, amendment). A caller that says
    // it compressed is not compressed again; otherwise the compressor runs
    // only above its threshold, which is never below the shared one. Neither
    // path can fail the call: a compressor error sends the transcript as it
    // came, and says so.
    let compression_telemetry = match caller_compression(req) {
        Some(t) => t,
        None => {
            let original = crate::ports::estimate_message_tokens(&messages);
            if original > compressor.threshold_tokens(model) {
                match compressor.compress(model, messages.clone()).await {
                    Ok((compressed, telemetry)) => {
                        messages = compressed;
                        telemetry
                    }
                    Err(e) => no_compression(original, Some(format!("compression unavailable, sent uncompressed: {e}"))),
                }
            } else {
                no_compression(original, None)
            }
        }
    };

    let tools = parse_tools(req.get("tools").unwrap_or(&serde_json::Value::Null))?;


    let request = InferenceRequest {
        model: model.to_string(),
        system_prompt: req.get("system").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        messages,
        tools,
        max_tokens: u32::try_from(req.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096))
            .unwrap_or(u32::MAX),
        temperature: req.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        thinking_budget: None,
        cache_control: false,
        priority: Priority::default(),
        grammar: None,
    };

    // Which backend runs it — including sending a tool-using request to one
    // that carries tools and the transcript — is the port's decision.
    let mut response = backends.complete(request).await.map_err(|e| e.to_string())?;

    // Merge compression telemetry into the response
    response.telemetry.extend(compression_telemetry);

    backends.record_spend(model, response.input_tokens, response.output_tokens);

    Ok(serde_json::json!({
        "content": serde_json::to_value(&response.content).map_err(|e| e.to_string())?,
        "model": response.model_used,
        "usage": {
            "input_tokens": response.input_tokens,
            "output_tokens": response.output_tokens,
        },
        "telemetry": response.telemetry,
    }))
}

/// The telemetry of a request no layer compressed.
fn no_compression(tokens: u64, note: Option<String>) -> std::collections::HashMap<String, String> {
    let mut t = std::collections::HashMap::new();
    t.insert("compression_layer".to_string(), "none".to_string());
    t.insert("original_tokens".to_string(), tokens.to_string());
    t.insert("compressed_tokens".to_string(), tokens.to_string());
    t.insert("summarization_calls".to_string(), "0".to_string());
    if let Some(n) = note {
        t.insert("compression_note".to_string(), n);
    }
    t
}

/// The caller's own compression report, when it says it already compressed
/// (`"compression": {"already_compressed": true, …}`, see
/// [`crate::ports::COMPRESSION_KEY`]): its figures become the telemetry, so
/// the layer named is the one that acted.
fn caller_compression(req: &serde_json::Value) -> Option<std::collections::HashMap<String, String>> {
    let c = req.get(crate::ports::COMPRESSION_KEY)?;
    if c.get("already_compressed").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let num = |k: &str| c.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let (original, compressed) = (num("original_tokens"), num("compressed_tokens"));
    let mut t = std::collections::HashMap::new();
    let layer = c.get("layer").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("caller");
    t.insert("compression_layer".to_string(), layer.to_string());
    t.insert("original_tokens".to_string(), original.to_string());
    t.insert("compressed_tokens".to_string(), compressed.to_string());
    t.insert("summarization_calls".to_string(), num("summarization_calls").to_string());
    if compressed > 0 {
        t.insert("compression_ratio".to_string(), format!("{:.1}x", original as f64 / compressed as f64));
        let saved = original.saturating_sub(compressed);
        let label = if saved < 1000 { saved.to_string() } else { format!("{:.1}k", saved as f64 / 1000.0) };
        t.insert("tokens_saved".to_string(), label);
    }
    Some(t)
}

/// One message, from the loop's JSON. Content may be a bare string or a block array.
fn message_from_json(v: &serde_json::Value) -> Message {
    let role = match v.get("role").and_then(|r| r.as_str()) {
        Some("assistant") => Role::Assistant,
        _ => Role::User,
    };
    let content = match v.get("content") {
        Some(serde_json::Value::String(s)) => vec![ContentBlock::Text { text: s.clone() }],
        Some(arr @ serde_json::Value::Array(_)) => serde_json::from_value(arr.clone())
            .unwrap_or_else(|_| vec![ContentBlock::Text { text: arr.to_string() }]),
        _ => Vec::new(),
    };
    Message { role, content }
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    /// The shape `direct_react::curated_schema` actually emits.
    fn curated_like() -> serde_json::Value {
        serde_json::json!([
            { "name": "repo_read", "description": "Read a file",
              "input_schema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] } },
            { "name": "propose_edit", "description": "Apply the edit and run evidence",
              "input_schema": { "type": "object", "properties": { "mode": { "type": "string" } }, "required": [] } }
        ])
    }

    #[test]
    fn the_curated_schema_survives_the_bridge() {
        let tools = parse_tools(&curated_like()).expect("curated schema must deserialize");
        assert_eq!(tools.len(), 2, "both tools must reach the model");
        assert_eq!(tools[0].name, "repo_read");
    }

    /// The bug this replaced: `.ok().unwrap_or_default()` turned a shape mismatch into an EMPTY
    /// tool list. The model then has no tools, invents a `{"tool": …}` JSON-in-text convention,
    /// and `extract_tool_uses` finds nothing — so the loop reports "ended with no edit" and it
    /// reads as the model being incapable. Observed with devstral-small-2:24b, which emits a
    /// perfectly good structured tool_call when it is actually given tools.
    #[test]
    fn a_malformed_tool_is_an_error_not_a_silent_empty_list() {
        // `input_schema` missing its required `type`.
        let bad = serde_json::json!([{ "name": "x", "description": "d", "input_schema": { "properties": {} } }]);
        assert!(parse_tools(&bad).is_err(), "a shape mismatch must be reported, never swallowed");
    }

    #[test]
    fn absent_tools_are_legitimately_empty() {
        assert_eq!(parse_tools(&serde_json::Value::Null).unwrap().len(), 0);
    }
}

#[cfg(test)]
mod truncated_tests {
    use super::text_or_truncation;
    use hexa_core::domain::messages::{ContentBlock, StopReason};

    fn text(t: &str) -> Vec<ContentBlock> {
        vec![ContentBlock::Text { text: t.to_string() }]
    }

    /// ADR-2609131835 §1: no text plus a max-tokens stop is a truncation,
    /// named, with the knob to turn. This is the reply gpt-oss-120b sent.
    #[test]
    fn no_text_and_a_max_tokens_stop_is_an_error_that_names_the_budget() {
        let err = text_or_truncation(&[], StopReason::MaxTokens, "openai/gpt-oss-120b", 4096).unwrap_err();
        assert!(err.contains("openai/gpt-oss-120b"), "{err}");
        assert!(err.contains("4096"), "names the budget it hit: {err}");
        assert!(err.contains("HEXA_REVIEW_MAX_TOKENS"), "names the knob: {err}");

        // Whitespace-only is no answer either.
        assert!(text_or_truncation(&text("  \n "), StopReason::MaxTokens, "m", 10).is_err());
    }

    /// An ordinary stop with no text is a real, if empty, answer — the
    /// distinction ADR-2609131646 drew, kept here.
    #[test]
    fn no_text_and_an_ordinary_stop_is_an_empty_answer_not_an_error() {
        assert_eq!(text_or_truncation(&[], StopReason::EndTurn, "m", 4096).unwrap(), "");
    }

    /// Text is returned whole under either stop: a truncated reply that
    /// still said something is that something.
    #[test]
    fn text_is_returned_under_either_stop() {
        assert_eq!(text_or_truncation(&text("{\"findings\":[]}"), StopReason::EndTurn, "m", 4096).unwrap(), "{\"findings\":[]}");
        assert_eq!(text_or_truncation(&text("partial"), StopReason::MaxTokens, "m", 4096).unwrap(), "partial");
    }
}

/// One budget, one owner (ADR-2611050000, amendment): `complete_raw_with`
/// compresses only above the shared threshold, and never a request whose
/// caller already compressed it.
#[cfg(test)]
mod compression_tests {
    use super::*;
    use crate::ports::{COMPRESSION_KEY, COMPRESSION_THRESHOLD_TOKENS};
    use hexa_core::ports::inference::{InferenceError, InferenceRequest, InferenceResponse};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A backend that records every request and answers "ok".
    #[derive(Default)]
    struct Recording {
        calls: Mutex<Vec<InferenceRequest>>,
    }

    #[async_trait::async_trait]
    impl Backends for Recording {
        async fn complete(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
            self.calls.lock().unwrap().push(request.clone());
            Ok(InferenceResponse {
                content: vec![ContentBlock::Text { text: "ok".into() }],
                model_used: request.model,
                stop_reason: StopReason::EndTurn,
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                latency_ms: 0,
                telemetry: HashMap::new(),
            })
        }
        fn record_spend(&self, _: &str, _: u64, _: u64) {}
    }

    /// A compressor that counts its calls and halves the transcript.
    #[derive(Default)]
    struct Counting {
        calls: Mutex<u32>,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl PromptCompressor for Counting {
        async fn compress(&self, _: &str, messages: Vec<Message>) -> Result<(Vec<Message>, HashMap<String, String>), String> {
            *self.calls.lock().unwrap() += 1;
            if self.fail {
                return Err("summarizer down".into());
            }
            let keep = messages.len() / 2;
            let mut t = HashMap::new();
            t.insert("compression_layer".to_string(), "semantic".to_string());
            Ok((messages[keep..].to_vec(), t))
        }
    }

    /// A tool-result transcript of about `tokens` estimated tokens.
    fn transcript(tokens: u64) -> serde_json::Value {
        let per = 4_000usize; // chars per result ≈ 1 000 tokens
        let n = (tokens as usize * 4).div_ceil(per);
        let mut msgs = vec![serde_json::json!({"role": "user", "content": "seed"})];
        for i in 0..n {
            msgs.push(serde_json::json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": format!("t{i}"), "name": "repo_grep", "input": {}}]}));
            msgs.push(serde_json::json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": format!("t{i}"), "content": "g".repeat(per)}]}));
        }
        serde_json::Value::Array(msgs)
    }

    fn req(messages: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"model": "m", "max_tokens": 16, "messages": messages})
    }

    #[test]
    fn the_shared_threshold_is_at_least_the_loops_budget() {
        const { assert!(COMPRESSION_THRESHOLD_TOKENS >= 12_000) };
        assert_eq!(Counting::default().threshold_tokens("any"), COMPRESSION_THRESHOLD_TOKENS);
    }

    /// The 4 000-token trigger this replaced fired on most loop steps.
    #[tokio::test]
    async fn nothing_is_compressed_at_or_below_the_shared_threshold() {
        let backends = Recording::default();
        let compressor = Counting::default();
        let r = req(transcript(COMPRESSION_THRESHOLD_TOKENS - 2_000));
        let out = complete_raw_with(&backends, &compressor, &r).await.unwrap();
        assert_eq!(*compressor.calls.lock().unwrap(), 0, "compressed below the threshold");
        assert_eq!(backends.calls.lock().unwrap().len(), 1);
        assert_eq!(out["telemetry"]["compression_layer"], "none");
    }

    /// Tool results count toward the estimate. `text_content` skipped them,
    /// so a transcript of large reads estimated near zero.
    #[tokio::test]
    async fn above_the_threshold_tool_results_count_and_the_compressor_runs_once() {
        let backends = Recording::default();
        let compressor = Counting::default();
        let r = req(transcript(COMPRESSION_THRESHOLD_TOKENS + 4_000));
        let out = complete_raw_with(&backends, &compressor, &r).await.unwrap();
        assert_eq!(*compressor.calls.lock().unwrap(), 1);
        assert_eq!(backends.calls.lock().unwrap().len(), 1);
        assert_eq!(out["telemetry"]["compression_layer"], "semantic");
    }

    /// One layer only: the ReAct loop says it compressed, so nothing here does.
    #[tokio::test]
    async fn a_caller_that_already_compressed_is_not_compressed_again() {
        let backends = Recording::default();
        let compressor = Counting::default();
        let mut r = req(transcript(COMPRESSION_THRESHOLD_TOKENS * 3));
        r[COMPRESSION_KEY] = serde_json::json!({
            "already_compressed": true, "layer": "mechanical",
            "original_tokens": 90000, "compressed_tokens": 36000, "summarization_calls": 1,
        });
        let out = complete_raw_with(&backends, &compressor, &r).await.unwrap();
        assert_eq!(*compressor.calls.lock().unwrap(), 0, "a second layer ran");
        assert_eq!(backends.calls.lock().unwrap().len(), 1, "exactly the user's call");
        let t = &out["telemetry"];
        assert_eq!(t["compression_layer"], "mechanical", "the layer that acted is the caller's: {t}");
        assert_eq!(t["original_tokens"], "90000");
        assert_eq!(t["compressed_tokens"], "36000");
        assert_eq!(t["summarization_calls"], "1");
        assert_eq!(t["compression_ratio"], "2.5x");
    }

    /// Summarization unavailable never fails the user's call.
    #[tokio::test]
    async fn a_failing_compressor_does_not_fail_the_call() {
        let backends = Recording::default();
        let compressor = Counting { fail: true, ..Default::default() };
        let r = req(transcript(COMPRESSION_THRESHOLD_TOKENS + 4_000));
        let out = complete_raw_with(&backends, &compressor, &r).await.expect("the call must survive");
        assert_eq!(backends.calls.lock().unwrap().len(), 1);
        let t = &out["telemetry"];
        assert_eq!(t["compression_layer"], "none", "{t}");
        assert!(t["compression_note"].as_str().unwrap_or("").contains("summarizer down"), "{t}");
    }
}
