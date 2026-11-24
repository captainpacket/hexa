//! Context-window compression for the ReAct loop (ADR-2606071XXX).
//!
//! A multi-step tool-use loop accumulates tool observations (grep hits, file
//! reads, cargo errors) in the message history. Left unbounded they blow the
//! model's context window and bloat every subsequent call. This module keeps the
//! transcript small *deterministically* (no model call) via two mechanisms:
//!
//!   1. per-observation head+tail cap — each tool_result is clamped to a budget,
//!      keeping the informative ends and eliding the middle;
//!   2. rolling window — only the last K observation turns are kept in full;
//!      older ones collapse to a one-line gist.
//!
//! When even the mechanical pass leaves the transcript over a token budget, the
//! loop (not this module) does ONE cheap-model summarization of the elided
//! region — the hybrid strategy. This module stays pure + unit-tested; the LLM
//! step lives in `direct_react`. Recursive decomposition over large observations
//! (RLM, arXiv:2512.24601) is the frontier beyond this compaction baseline.

use serde_json::{json, Value};

/// Knobs for mechanical compression.
#[derive(Debug, Clone, Copy)]
pub struct CompressOpts {
    /// Bytes kept from the head of each observation.
    pub obs_head: usize,
    /// Bytes kept from the tail of each observation.
    pub obs_tail: usize,
    /// How many of the most-recent observation turns to keep verbatim (capped).
    /// Older observation turns collapse to a one-line gist.
    pub keep_recent: usize,
}

impl Default for CompressOpts {
    fn default() -> Self {
        Self { obs_head: 1024, obs_tail: 1024, keep_recent: 4 }
    }
}

/// Clamp a string to `head` + `tail` bytes (on char boundaries), inserting an
/// elision marker. Returns the clamped string. No-op when already small enough.
pub fn cap_str(s: &str, head: usize, tail: usize) -> String {
    if s.len() <= head + tail + 32 {
        return s.to_string();
    }
    let head_end = floor_char_boundary(s, head);
    let tail_start = ceil_char_boundary(s, s.len().saturating_sub(tail));
    let elided = tail_start.saturating_sub(head_end);
    format!("{}\n…[elided {} bytes]…\n{}", &s[..head_end], elided, &s[tail_start..])
}

/// Rough token estimate (chars / 4) over the whole message array — enough to
/// decide when the LLM-overflow summarization should fire.
pub(crate) fn estimate_tokens(messages: &[Value]) -> usize {
    let mut chars = 0usize;
    for m in messages {
        chars += content_chars(m.get("content").unwrap_or(&Value::Null));
    }
    chars / 4
}

fn content_chars(content: &Value) -> usize {
    match content {
        Value::String(s) => s.len(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|b| {
                // tool_use input, text, or tool_result content string.
                b.get("content").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0)
                    + b.get("text").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0)
                    + b.get("input").map(|v| v.to_string().len()).unwrap_or(0)
            })
            .sum(),
        _ => 0,
    }
}

/// Returns true if a message is a user turn carrying tool_result blocks (an
/// "observation" turn produced by the loop after dispatching tools).
fn is_observation_turn(m: &Value) -> bool {
    m.get("role").and_then(|v| v.as_str()) == Some("user")
        && m.get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| blocks.iter().any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_result")))
            .unwrap_or(false)
}

/// Mechanically compress the transcript: cap every observation to head+tail, and
/// collapse all but the last `keep_recent` observation turns to a one-line gist.
/// The first message (the task/seed) and assistant reasoning turns pass through.
pub(crate) fn compress_messages(messages: &[Value], opts: &CompressOpts) -> Vec<Value> {
    // Index the observation turns so we can keep only the most recent K full.
    let obs_indices: Vec<usize> =
        messages.iter().enumerate().filter(|(_, m)| is_observation_turn(m)).map(|(i, _)| i).collect();
    let keep_from = obs_indices.len().saturating_sub(opts.keep_recent);
    let recent_obs: std::collections::HashSet<usize> =
        obs_indices.iter().skip(keep_from).copied().collect();

    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if !is_observation_turn(m) {
                // Cap non-observations if they are not very recent (keep last 2).
                if i < messages.len().saturating_sub(2) {
                    return cap_message(m.clone(), opts);
                }
                return m.clone();
            }
            let blocks = m.get("content").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            if recent_obs.contains(&i) {
                // Recent: keep, but cap every block's payload.
                let capped: Vec<Value> = blocks
                    .into_iter()
                    .map(|b| cap_block(b, opts.obs_head, opts.obs_tail))
                    .collect();
                json!({ "role": "user", "content": capped })
            } else {
                // Old: collapse tool_results to gists, cap everything else.
                let gists: Vec<Value> = blocks
                    .into_iter()
                    .map(|b| {
                        if b.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                            gist_tool_result(b)
                        } else {
                            cap_block(b, opts.obs_head, opts.obs_tail)
                        }
                    })
                    .collect();
                json!({ "role": "user", "content": gists })
            }
        })
        .collect()
}

fn cap_block(mut b: Value, head: usize, tail: usize) -> Value {
    if let Some(s) = b.get("content").and_then(|v| v.as_str()) {
        let capped = cap_str(s, head, tail);
        if let Some(obj) = b.as_object_mut() {
            obj.insert("content".to_string(), json!(capped));
        }
    }
    if let Some(s) = b.get("text").and_then(|v| v.as_str()) {
        let capped = cap_str(s, head, tail);
        if let Some(obj) = b.as_object_mut() {
            obj.insert("text".to_string(), json!(capped));
        }
    }
    b
}

fn cap_message(mut m: Value, opts: &CompressOpts) -> Value {
    if let Some(content) = m.get_mut("content") {
        match content {
            Value::String(s) => {
                *content = json!(cap_str(s, opts.obs_head, opts.obs_tail));
            }
            Value::Array(blocks) => {
                let capped: Vec<Value> = blocks
                    .iter()
                    .map(|b| cap_block(b.clone(), opts.obs_head, opts.obs_tail))
                    .collect();
                *content = json!(capped);
            }
            _ => {}
        }
    }
    m
}

fn gist_tool_result(b: Value) -> Value {
    if b.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
        return b;
    }
    let id = b.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
    let bytes = b.get("content").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
    let is_err = b.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
    json!({
        "type": "tool_result",
        "tool_use_id": id,
        "content": format!("[earlier observation elided — {} bytes{}]", bytes, if is_err { ", was error" } else { "" }),
        "is_error": is_err,
    })
}

/// The loop's context budget: the one threshold both compression layers share
/// (ADR-2611050000, amendment). Above it, after the mechanical pass, the loop
/// makes its one summarization call; `hexa-infer` never compresses below it.
pub const LOOP_BUDGET_TOKENS: usize = hexa_infer::ports::COMPRESSION_THRESHOLD_TOKENS as usize;

/// What one step of the loop sends, and how it was made.
#[derive(Debug, Clone)]
pub struct PreparedStep {
    pub messages: Vec<Value>,
    /// The transcript's estimate before compression.
    pub original_tokens: usize,
    /// The estimate of what is sent.
    pub compressed_tokens: usize,
    /// 0 or 1: the loop's summarization call, if it made one.
    pub summarization_calls: usize,
}

impl PreparedStep {
    /// The request's `compression` object
    /// ([`hexa_infer::ports::COMPRESSION_KEY`]): it tells `complete_raw` this
    /// transcript is already compressed, so no second layer runs, and carries
    /// the figures its telemetry reports.
    pub fn envelope(&self) -> Value {
        json!({
            "already_compressed": true,
            "layer": if self.summarization_calls == 0 { "mechanical" } else { "mechanical+summary" },
            "original_tokens": self.original_tokens,
            "compressed_tokens": self.compressed_tokens,
            "summarization_calls": self.summarization_calls,
        })
    }
}

/// One step's compression: the mechanical pass, then — only when the result is
/// still over `budget` — one call to `summarize`. `FnOnce`, so a step cannot
/// make a second summarization call.
pub async fn prepare_step<F, Fut>(messages: &[Value], opts: &CompressOpts, budget: usize, summarize: F) -> PreparedStep
where
    F: FnOnce(Vec<Value>) -> Fut,
    Fut: std::future::Future<Output = Vec<Value>>,
{
    let original_tokens = estimate_tokens(messages);
    let mut sent = compress_messages(messages, opts);
    let mut summarization_calls = 0;
    if estimate_tokens(&sent) > budget {
        sent = summarize(sent).await;
        summarization_calls = 1;
    }
    PreparedStep { compressed_tokens: estimate_tokens(&sent), messages: sent, original_tokens, summarization_calls }
}

// `str::floor_char_boundary`/`ceil_char_boundary` are unstable; provide local ones.
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_str_keeps_ends_and_elides_middle() {
        let s = "A".repeat(5000);
        let capped = cap_str(&s, 100, 100);
        assert!(capped.len() < s.len());
        assert!(capped.starts_with(&"A".repeat(100)));
        assert!(capped.contains("elided"));
        assert!(capped.ends_with(&"A".repeat(100)));
    }

    #[test]
    fn cap_str_noop_when_small() {
        let s = "short";
        assert_eq!(cap_str(s, 1024, 1024), "short");
    }

    #[test]
    fn cap_str_respects_char_boundaries() {
        // Multibyte chars at the cut points must not panic / split a char.
        let s = format!("{}{}", "é".repeat(2000), "ü".repeat(2000));
        let capped = cap_str(&s, 101, 101); // 101 is mid-2-byte-char
        assert!(capped.contains("elided"));
    }

    #[test]
    fn estimate_tokens_counts_blocks() {
        let msgs = vec![
            json!({"role":"user","content":"hello world"}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"A".repeat(400)}]}),
        ];
        // (11 + 400) / 4 ≈ 102
        let t = estimate_tokens(&msgs);
        assert!((100..=105).contains(&t), "got {}", t);
    }

    #[test]
    fn rolling_window_gists_old_observations() {
        // 6 observation turns, keep_recent=2 → first 4 become gists, last 2 capped.
        let big = "X".repeat(5000);
        let mut msgs = vec![json!({"role":"user","content":"task seed"})];
        for i in 0..6 {
            msgs.push(json!({"role":"assistant","content":[{"type":"text","text":format!("step {}",i)}]}));
            msgs.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":format!("t{}",i),"content":big.clone()}]}));
        }
        let opts = CompressOpts { obs_head: 200, obs_tail: 200, keep_recent: 2 };
        let out = compress_messages(&msgs, &opts);

        let obs: Vec<&Value> = out.iter().filter(|m| is_observation_turn(m)).collect();
        assert_eq!(obs.len(), 6);
        // First 4 are gists (short), last 2 are capped (head+tail+marker, still < big).
        for m in &obs[..4] {
            let c = m["content"][0]["content"].as_str().unwrap();
            assert!(c.contains("earlier observation elided"), "old should be gist: {}", c);
        }
        for m in &obs[4..] {
            let c = m["content"][0]["content"].as_str().unwrap();
            assert!(c.contains("elided") && c.len() < big.len(), "recent should be capped");
            assert!(!c.contains("earlier observation"), "recent should not be gisted");
        }
        // Seed + assistant turns pass through untouched.
        assert_eq!(out[0]["content"].as_str().unwrap(), "task seed");
    }

    #[test]
    fn compression_reduces_token_estimate() {
        let big = "Y".repeat(8000);
        let mut msgs = vec![json!({"role":"user","content":"seed"})];
        for i in 0..5 {
            msgs.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":format!("t{}",i),"content":big.clone()}]}));
        }
        let before = estimate_tokens(&msgs);
        let out = compress_messages(&msgs, &CompressOpts::default());
        let after = estimate_tokens(&out);
        assert!(after < before / 2, "compression should roughly halve+: {} -> {}", before, after);
    }

    // ── One budget, one owner (ADR-2611050000, amendment) ──────────────

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A ReAct transcript after `steps` tool calls of ~6k chars each. Each
    /// call is a `propose_edit` carrying ~4k chars of new text: tool inputs
    /// are not capped by the mechanical pass, so a long run outgrows it.
    fn react_transcript(steps: usize) -> Vec<Value> {
        let mut m = vec![json!({"role": "user", "content": "TASK: fix src/lib.rs"})];
        for i in 0..steps {
            m.push(json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": format!("t{i}"), "name": "propose_edit", "input": {"new": "e".repeat(4_000)}}]}));
            m.push(json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": format!("t{i}"), "content": format!("hit {i} {}", "z".repeat(6_000))}]}));
        }
        m
    }

    #[test]
    fn the_loop_budget_is_the_shared_threshold() {
        assert_eq!(LOOP_BUDGET_TOKENS as u64, hexa_infer::ports::COMPRESSION_THRESHOLD_TOKENS);
    }

    #[tokio::test]
    async fn under_budget_no_summarization_call_is_made() {
        let calls = AtomicUsize::new(0);
        let step = prepare_step(&react_transcript(2), &CompressOpts::default(), LOOP_BUDGET_TOKENS, |m| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move { m }
        })
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(step.summarization_calls, 0);
        assert_eq!(step.envelope()["already_compressed"], true);
        assert_eq!(step.envelope()["layer"], "mechanical");
    }

    #[tokio::test]
    async fn over_budget_exactly_one_summarization_call_is_made() {
        let calls = AtomicUsize::new(0);
        // A budget the mechanical pass cannot meet.
        let step = prepare_step(&react_transcript(10), &CompressOpts::default(), 500, |m: Vec<Value>| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move { vec![m[0].clone(), json!({"role": "user", "content": "summary"})] }
        })
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(step.summarization_calls, 1);
        let e = step.envelope();
        assert_eq!(e["summarization_calls"], 1);
        assert_eq!(e["layer"], "mechanical+summary");
        assert_eq!(e["original_tokens"].as_u64().unwrap() as usize, estimate_tokens(&react_transcript(10)));
        assert_eq!(e["compressed_tokens"].as_u64().unwrap() as usize, estimate_tokens(&step.messages));
    }

    /// A backend that counts calls and answers "ok".
    struct CountingBackend(AtomicUsize);

    #[async_trait::async_trait]
    impl hexa_infer::ports::Backends for CountingBackend {
        async fn complete(
            &self,
            request: hexa_core::ports::inference::InferenceRequest,
        ) -> Result<hexa_core::ports::inference::InferenceResponse, hexa_core::ports::inference::InferenceError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(hexa_core::ports::inference::InferenceResponse {
                content: vec![hexa_core::domain::messages::ContentBlock::Text { text: "ok".into() }],
                model_used: request.model,
                stop_reason: hexa_core::domain::messages::StopReason::EndTurn,
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                latency_ms: 0,
                telemetry: Default::default(),
            })
        }
        fn record_spend(&self, _: &str, _: u64, _: u64) {}
    }

    /// A compressor that counts the calls a second layer would make.
    struct CountingCompressor(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl hexa_infer::ports::PromptCompressor for CountingCompressor {
        async fn compress(
            &self,
            _: &str,
            messages: Vec<hexa_core::domain::messages::Message>,
        ) -> Result<(Vec<hexa_core::domain::messages::Message>, std::collections::HashMap<String, String>), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok((messages, Default::default()))
        }
    }

    /// The loop's step, end to end through `complete_raw_with`: however long
    /// the transcript grows, a step costs at most one summarization call —
    /// the loop's own — and the inference layer does not compress again.
    #[tokio::test]
    async fn a_long_react_transcript_costs_at_most_one_summarization_call_per_step() {
        let backend = CountingBackend(AtomicUsize::new(0));
        let second_layer = Arc::new(AtomicUsize::new(0));
        let compressor = CountingCompressor(second_layer.clone());
        let mut over_budget_steps = 0;
        for steps in 1..=40 {
            let transcript = react_transcript(steps);
            let loop_calls = AtomicUsize::new(0);
            let step = prepare_step(&transcript, &CompressOpts::default(), LOOP_BUDGET_TOKENS, |m: Vec<Value>| {
                loop_calls.fetch_add(1, Ordering::SeqCst);
                async move { m }
            })
            .await;
            if estimate_tokens(&compress_messages(&transcript, &CompressOpts::default())) > LOOP_BUDGET_TOKENS {
                over_budget_steps += 1;
            }
            let req = json!({
                "model": "m", "max_tokens": 16, "messages": step.messages,
                hexa_infer::ports::COMPRESSION_KEY: step.envelope(),
            });
            let before = second_layer.load(Ordering::SeqCst);
            let out = hexa_infer::complete::complete_raw_with(&backend, &compressor, &req).await.unwrap();
            let per_step = loop_calls.load(Ordering::SeqCst) + second_layer.load(Ordering::SeqCst) - before;
            assert!(per_step <= 1, "step {steps} made {per_step} summarization calls");
            assert_eq!(out["telemetry"]["summarization_calls"], loop_calls.load(Ordering::SeqCst).to_string());
        }
        assert!(over_budget_steps > 0, "vacuous: no step went over the loop budget");
        assert_eq!(second_layer.load(Ordering::SeqCst), 0, "the inference layer compressed a loop request");
        assert_eq!(backend.0.load(Ordering::SeqCst), 40, "one model call per step");
    }
}
