//! A semantic compressor that reduces conversation history using recursive summarization.
//!
//! This adapter implements the [`PromptCompressor`] port. It runs only when
//! `complete_raw_with` finds a request over the shared budget
//! ([`COMPRESSION_THRESHOLD_TOKENS`]) that its caller did not already compress
//! (ADR-2611050000, amendment).
//!
//! **What it never summarizes.** The system prompt is not a message and never
//! reaches it. Of the messages it is given, these are *pinned* and pass byte
//! for byte: the most recent plain user turn (the task), the last
//! [`CompressorConfig::keep_recent_tool_results`] tool results, every result
//! of a file-read tool ([`CompressorConfig::verbatim_tools`]) — the model may
//! have to quote that text exactly in a `replace_string` edit — and the last
//! [`CompressorConfig::recent_window`] messages. A pinned result keeps its
//! `tool_use` and a pinned `tool_use` its result, so no call is orphaned.
//!
//! **What it summarizes.** Every other message, into one summary placed where
//! the first of them stood, by recursive rounds of up to four chunk summaries
//! on the cheap model. The rounds stop when the summary fits
//! [`CompressorConfig::budget`], when a round fails to shrink it, or at
//! [`CompressorConfig::max_summarization_calls`]; whatever is still over is
//! capped head-and-tail. So the calls are bounded and the recursion ends.
//!
//! **When it cannot.** No cheap model resolved (the wiring reads tier `t1`),
//! or the summarizer failed: the unpinned messages are capped head-and-tail
//! instead — structure kept, no call made — and the telemetry says so in
//! `compression_note`. It never returns an error for that.

use async_trait::async_trait;
use hexa_core::ports::inference::{ContentBlock, Message, Role};
use hexa_core::ports::inference::InferenceRequest;
use crate::ports::{estimate_message_tokens, Backends, PromptCompressor, COMPRESSION_THRESHOLD_TOKENS};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Configuration for the [`SemanticCompressor`].
#[derive(Debug, Clone)]
pub struct CompressorConfig {
    /// The model summaries go to, as the tier configuration resolved it.
    /// `None` when nothing resolves: then nothing is summarized, and the
    /// unpinned turns are capped mechanically.
    pub cheap_model: Option<String>,
    /// The last messages always kept as they are.
    pub recent_window: usize,
    /// The last tool results always kept as they are. 4 by default, the
    /// ReAct loop's own rolling window.
    pub keep_recent_tool_results: usize,
    /// Tools whose results are file text the model may edit: always kept.
    pub verbatim_tools: Vec<String>,
    /// The token budget the summary of the unpinned turns is brought under.
    pub budget: u64,
    /// The most summarization calls one compression may make.
    pub max_summarization_calls: usize,
    /// The estimate above which this compressor is asked to run. Never
    /// below [`COMPRESSION_THRESHOLD_TOKENS`].
    pub threshold: u64,
    /// Bytes kept from each end of a capped text.
    pub cap_head: usize,
    pub cap_tail: usize,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            cheap_model: None,
            recent_window: 2,
            keep_recent_tool_results: 4,
            verbatim_tools: vec!["repo_read".to_string(), "read_file".to_string()],
            budget: 2000,
            max_summarization_calls: 8,
            threshold: COMPRESSION_THRESHOLD_TOKENS,
            cap_head: 1024,
            cap_tail: 1024,
        }
    }
}

pub struct SemanticCompressor {
    backends: Arc<dyn Backends>,
    config: CompressorConfig,
}

/// Bytes of one chunk sent in one summarization call, at most.
const MAX_CHUNK_CHARS: usize = 48_000;

impl SemanticCompressor {
    pub fn new(backends: Arc<dyn Backends>, config: CompressorConfig) -> Self {
        Self { backends, config }
    }

    /// Which messages pass verbatim (see the module docs).
    fn pinned(&self, messages: &[Message]) -> Vec<bool> {
        let n = messages.len();
        let mut pin = vec![false; n];
        let has_result = |m: &Message| m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }));

        // The most recent plain user turn.
        if let Some(i) = messages.iter().rposition(|m| {
            m.role == Role::User && !has_result(m) && m.content.iter().any(|b| matches!(b, ContentBlock::Text { .. }))
        }) {
            pin[i] = true;
        }
        // The last messages.
        for p in pin.iter_mut().skip(n.saturating_sub(self.config.recent_window)) {
            *p = true;
        }
        // The last N tool results.
        let result_turns: Vec<usize> = (0..n).filter(|&i| has_result(&messages[i])).collect();
        for &i in result_turns.iter().rev().take(self.config.keep_recent_tool_results) {
            pin[i] = true;
        }
        // Every file read.
        let reads: HashSet<&str> = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, .. } if self.config.verbatim_tools.iter().any(|t| t == name) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        for (i, m) in messages.iter().enumerate() {
            if m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { tool_use_id, .. } if reads.contains(tool_use_id.as_str()))) {
                pin[i] = true;
            }
        }
        // Calls and results travel together, to a fixed point.
        let ids = |m: &Message| -> Vec<String> {
            m.content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                    _ => None,
                })
                .collect()
        };
        loop {
            let live: HashSet<String> = (0..n).filter(|&i| pin[i]).flat_map(|i| ids(&messages[i])).collect();
            let mut changed = false;
            for i in 0..n {
                if !pin[i] && ids(&messages[i]).iter().any(|id| live.contains(id)) {
                    pin[i] = true;
                    changed = true;
                }
            }
            if !changed {
                return pin;
            }
        }
    }

    /// Summarize `messages` under the budget. Returns the summary (or the
    /// error that stopped it), the calls made, and a note when the call bound
    /// or a round that did not shrink ended it early.
    async fn recursive_summarize(&self, model: &str, messages: &[Message]) -> (Result<String, String>, usize, Option<String>) {
        let mut calls = 0usize;
        let mut current: Vec<String> = messages.iter().map(render).collect();
        let max_chars = (self.config.budget as usize) * 4;
        loop {
            let content = current.join("\n");
            if estimate_text(&content) <= self.config.budget {
                return (Ok(content), calls, None);
            }
            if calls >= self.config.max_summarization_calls {
                let note = format!("summarization stopped at its bound of {} calls; the rest was capped", self.config.max_summarization_calls);
                return (Ok(cap_str(&content, max_chars / 2, max_chars / 2)), calls, Some(note));
            }
            let chunk_size = current.len().div_ceil(4).max(1);
            let mut next = Vec::new();
            for chunk in current.chunks(chunk_size) {
                let text = chunk.join("\n");
                if calls >= self.config.max_summarization_calls {
                    next.push(cap_str(&text, self.config.cap_head, self.config.cap_tail));
                    continue;
                }
                calls += 1;
                match self.summarize_chunk(model, &text).await {
                    Ok(s) => next.push(s),
                    Err(e) => return (Err(e), calls, None),
                }
            }
            let shrunk = next.iter().map(String::len).sum::<usize>() < content.len();
            if !shrunk {
                let joined = next.join("\n");
                let note = "a summarization round did not shrink the text; the rest was capped".to_string();
                return (Ok(cap_str(&joined, max_chars / 2, max_chars / 2)), calls, Some(note));
            }
            current = next;
        }
    }

    async fn summarize_chunk(&self, model: &str, content: &str) -> Result<String, String> {
        let prompt = format!(
            "Summarize the following conversation history concisely, preserving all key semantic signals, facts, and decisions. \
            The summary will be used to provide context for a future conversation.\n\nHistory:\n{}",
            cap_str(content, MAX_CHUNK_CHARS / 2, MAX_CHUNK_CHARS / 2)
        );

        let request = InferenceRequest {
            model: model.to_string(),
            system_prompt: "You are a helpful assistant that creates concise, high-signal summaries of conversation history.".to_string(),
            messages: vec![Message::user(&prompt)],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.0,
            thinking_budget: None,
            cache_control: false,
            priority: hexa_core::ports::inference::Priority::Normal,
            grammar: None,
        };

        let response = self.backends.complete(request).await
            .map_err(|e| format!("summarization on {model} failed: {e}"))?;
        self.backends.record_spend(model, response.input_tokens, response.output_tokens);

        Ok(response.content.iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""))
    }

    /// The unpinned messages capped head-and-tail, structure kept.
    fn mechanical(&self, messages: Vec<Message>, pinned: &[bool]) -> Vec<Message> {
        let (h, t) = (self.config.cap_head, self.config.cap_tail);
        messages
            .into_iter()
            .zip(pinned)
            .map(|(mut m, &keep)| {
                if !keep {
                    for b in &mut m.content {
                        match b {
                            ContentBlock::Text { text } => *text = cap_str(text, h, t),
                            ContentBlock::ToolResult { content, .. } => *content = cap_str(content, h, t),
                            ContentBlock::ToolUse { .. } => {}
                        }
                    }
                }
                m
            })
            .collect()
    }
}

#[async_trait]
impl PromptCompressor for SemanticCompressor {
    async fn compress(&self, _model: &str, messages: Vec<Message>) -> Result<(Vec<Message>, HashMap<String, String>), String> {
        let original = estimate_message_tokens(&messages);
        let pinned = self.pinned(&messages);
        let Some(first) = pinned.iter().position(|p| !p) else {
            return Ok((messages, telemetry(original, original, "none", 0, Some("every turn is pinned verbatim".into()))));
        };

        let Some(cheap) = self.config.cheap_model.as_deref() else {
            let out = self.mechanical(messages, &pinned);
            let k = estimate_message_tokens(&out);
            let note = "no cheap model resolves (inference.tier_models.t1 is not configured); older turns were capped, not summarized";
            return Ok((out, telemetry(original, k, "mechanical", 0, Some(note.into()))));
        };

        let older: Vec<Message> = messages.iter().zip(&pinned).filter(|(_, p)| !**p).map(|(m, _)| m.clone()).collect();
        let (summary, calls, note) = self.recursive_summarize(cheap, &older).await;
        let summary = match summary {
            Ok(s) => s,
            Err(e) => {
                let out = self.mechanical(messages, &pinned);
                let k = estimate_message_tokens(&out);
                let note = format!("{e}; older turns were capped, not summarized");
                return Ok((out, telemetry(original, k, "mechanical", calls, Some(note))));
            }
        };

        let mut out = Vec::with_capacity(messages.len());
        for (i, (m, keep)) in messages.into_iter().zip(&pinned).enumerate() {
            if i == first && !summary.is_empty() {
                out.push(Message::user(&format!("[Earlier conversation, summarized]\n{summary}")));
            }
            if *keep {
                out.push(m);
            }
        }
        let k = estimate_message_tokens(&out);
        Ok((out, telemetry(original, k, "semantic", calls, note)))
    }

    fn threshold_tokens(&self, _model: &str) -> u64 {
        self.config.threshold.max(COMPRESSION_THRESHOLD_TOKENS)
    }
}

/// The telemetry of one compression, under the keys `hexa-cli`'s
/// `telemetry.json` names, plus the layer, the calls, and any note.
pub fn telemetry(original: u64, compressed: u64, layer: &str, calls: usize, note: Option<String>) -> HashMap<String, String> {
    let mut t = HashMap::new();
    let ratio = original as f64 / compressed.max(1) as f64;
    t.insert("compression_ratio".to_string(), format!("{ratio:.1}x"));
    t.insert("tokens_saved".to_string(), tokens_label(original.saturating_sub(compressed)));
    t.insert("original_tokens".to_string(), original.to_string());
    t.insert("compressed_tokens".to_string(), compressed.to_string());
    t.insert("compression_layer".to_string(), layer.to_string());
    t.insert("summarization_calls".to_string(), calls.to_string());
    if let Some(n) = note {
        t.insert("compression_note".to_string(), n);
    }
    t
}

/// `950`, `1.2k`: a count small enough to read, never rounded to `0k`.
fn tokens_label(n: u64) -> String {
    if n < 1000 { n.to_string() } else { format!("{:.1}k", n as f64 / 1000.0) }
}

fn render(m: &Message) -> String {
    let role = match m.role {
        Role::User => "User",
        Role::Assistant => "Assistant",
    };
    let body: Vec<String> = m
        .content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::ToolUse { name, input, .. } => format!("[called {name} {input}]"),
            ContentBlock::ToolResult { content, .. } => format!("[result] {content}"),
        })
        .collect();
    format!("{role}: {}", body.join("\n"))
}

fn estimate_text(text: &str) -> u64 {
    text.len() as u64 / 4
}

/// `s` clamped to `head` + `tail` bytes on char boundaries, the middle elided.
fn cap_str(s: &str, head: usize, tail: usize) -> String {
    if s.len() <= head + tail + 32 {
        return s.to_string();
    }
    let mut h = head.min(s.len());
    while !s.is_char_boundary(h) {
        h -= 1;
    }
    let mut t = s.len().saturating_sub(tail);
    while !s.is_char_boundary(t) {
        t += 1;
    }
    format!("{}\n…[elided {} bytes]…\n{}", &s[..h], t - h, &s[t..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexa_core::domain::messages::StopReason;
    use hexa_core::ports::inference::{InferenceError, InferenceResponse};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// What the fake summarizer answers.
    #[derive(Clone, Copy)]
    enum Reply {
        /// A short summary.
        Short,
        /// A reply longer than its input: summarizing never shrinks.
        Bloat,
        /// An error.
        Fail,
    }

    /// A [`Backends`] that records every request.
    struct Fake {
        calls: Mutex<Vec<InferenceRequest>>,
        reply: Reply,
    }

    impl Fake {
        fn new(reply: Reply) -> Arc<Fake> {
            Arc::new(Fake { calls: Mutex::new(Vec::new()), reply })
        }
        fn count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl Backends for Fake {
        async fn complete(&self, request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
            let input: usize = request.messages.iter().map(|m| m.text_content().len()).sum();
            self.calls.lock().unwrap().push(request.clone());
            let text = match self.reply {
                Reply::Short => "SUMMARY: earlier greps found the symbol in lib.rs.".to_string(),
                Reply::Bloat => "B".repeat(input * 2),
                Reply::Fail => return Err(InferenceError::ProviderUnavailable("down".into())),
            };
            Ok(InferenceResponse {
                content: vec![ContentBlock::Text { text }],
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

    fn config(cheap: Option<&str>) -> CompressorConfig {
        CompressorConfig {
            cheap_model: cheap.map(str::to_string),
            keep_recent_tool_results: 2,
            ..CompressorConfig::default()
        }
    }

    /// A ReAct-shaped transcript: a seed, then `rounds` of one tool call and
    /// its result. Every third call is a `repo_read` of a file the model may
    /// edit; the rest are greps.
    fn transcript(rounds: usize) -> Vec<Message> {
        let mut m = vec![Message::user("TASK: fix the bug in src/lib.rs")];
        for i in 0..rounds {
            let name = if i.is_multiple_of(3) { "repo_read" } else { "repo_grep" };
            m.push(Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Text { text: format!("step {i}") },
                    ContentBlock::ToolUse { id: format!("t{i}"), name: name.into(), input: serde_json::json!({"path": "src/lib.rs"}) },
                ],
            });
            m.push(Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: format!("t{i}"),
                    content: format!("{name} {i}: {}", "x".repeat(6_000)),
                    is_error: None,
                }],
            });
        }
        m
    }

    fn results(messages: &[Message]) -> Vec<(String, String)> {
        messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::ToolResult { tool_use_id, content, .. } => Some((tool_use_id.clone(), content.clone())),
                _ => None,
            })
            .collect()
    }

    fn uses(messages: &[Message]) -> Vec<String> {
        messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    /// The recent tool results, every file read, and the most recent user
    /// turn reach the model byte for byte; every kept result keeps its call.
    #[tokio::test]
    async fn recent_results_file_reads_and_the_user_turn_stay_verbatim() {
        let backends = Fake::new(Reply::Short);
        let input = transcript(12);
        let c = SemanticCompressor::new(backends.clone(), config(Some("cheap-tier-model")));
        let (out, t) = c.compress("main", input.clone()).await.unwrap();

        let before = results(&input);
        let after = results(&out);
        for (id, content) in &before {
            let n: usize = id[1..].parse().unwrap();
            let is_read = n.is_multiple_of(3);
            let is_recent = n >= 10; // keep_recent_tool_results = 2
            if is_read || is_recent {
                assert!(after.contains(&(id.clone(), content.clone())), "{id} was not kept verbatim");
            }
        }
        assert!(after.len() < before.len(), "nothing was summarized");
        assert!(after.iter().all(|(id, _)| !id.is_empty()));
        let kept_uses = uses(&out);
        for (id, _) in &after {
            assert!(kept_uses.contains(id), "result {id} lost its tool_use");
        }
        for id in &kept_uses {
            assert!(after.iter().any(|(r, _)| r == id), "tool_use {id} lost its result");
        }
        assert_eq!(out[0].text_content(), "TASK: fix the bug in src/lib.rs", "the user turn was rewritten");
        assert!(out.iter().any(|m| m.text_content().contains("SUMMARY")), "no summary in {out:?}");
        assert_eq!(t["compression_layer"], "semantic");
    }

    /// The cheap model is the resolved one, never a literal tier name.
    #[tokio::test]
    async fn summaries_go_to_the_resolved_cheap_model() {
        let backends = Fake::new(Reply::Short);
        let c = SemanticCompressor::new(backends.clone(), config(Some("cheap-tier-model")));
        c.compress("main", transcript(12)).await.unwrap();
        let calls = backends.calls.lock().unwrap();
        assert!(!calls.is_empty());
        assert!(calls.iter().all(|r| r.model == "cheap-tier-model"), "{:?}", calls.iter().map(|r| &r.model).collect::<Vec<_>>());
    }

    /// A summarizer whose output never shrinks cannot loop forever: the
    /// rounds stop at the call bound and the rest is capped mechanically.
    #[tokio::test]
    async fn recursion_terminates_within_the_call_bound() {
        let backends = Fake::new(Reply::Bloat);
        let cfg = CompressorConfig { max_summarization_calls: 5, ..config(Some("cheap")) };
        let c = SemanticCompressor::new(backends.clone(), cfg);
        let (_, t) = tokio::time::timeout(std::time::Duration::from_secs(5), c.compress("main", transcript(30)))
            .await
            .expect("recursion did not terminate")
            .unwrap();
        assert!(backends.count() <= 5, "made {} calls", backends.count());
        assert_eq!(t["summarization_calls"], backends.count().to_string());
    }

    /// No cheap model configured: no call is made, nothing fails, and the
    /// older turns are capped mechanically with a note saying why.
    #[tokio::test]
    async fn no_cheap_model_falls_back_to_mechanical_capping() {
        let backends = Fake::new(Reply::Short);
        let input = transcript(12);
        let c = SemanticCompressor::new(backends.clone(), config(None));
        let (out, t) = c.compress("main", input.clone()).await.expect("never an error");
        assert_eq!(backends.count(), 0, "a model name was sent without a resolved tier");
        assert_eq!(t["compression_layer"], "mechanical");
        assert_eq!(t["summarization_calls"], "0");
        assert!(t["compression_note"].contains("t1"), "{t:?}");
        assert!(estimate_message_tokens(&out) < estimate_message_tokens(&input));
        // Pinned content survives the fallback too.
        let after = results(&out);
        for (id, content) in results(&input) {
            let n: usize = id[1..].parse().unwrap();
            if n.is_multiple_of(3) || n >= 10 {
                assert!(after.contains(&(id.clone(), content)), "{id} was capped");
            }
        }
    }

    /// A summarizer that errors falls back the same way.
    #[tokio::test]
    async fn a_failing_summarizer_falls_back_without_an_error() {
        let backends = Fake::new(Reply::Fail);
        let c = SemanticCompressor::new(backends.clone(), config(Some("cheap")));
        let (_, t) = c.compress("main", transcript(12)).await.expect("never an error");
        assert_eq!(t["compression_layer"], "mechanical");
        assert!(t["compression_note"].contains("down"), "{t:?}");
    }

    /// The telemetry is what happened: real token counts, the ratio between
    /// them, the layer, and the number of calls the backend saw.
    #[tokio::test]
    async fn telemetry_reports_what_happened() {
        let backends = Fake::new(Reply::Short);
        let input = transcript(12);
        let c = SemanticCompressor::new(backends.clone(), config(Some("cheap")));
        let (out, t) = c.compress("main", input.clone()).await.unwrap();
        let (o, k) = (estimate_message_tokens(&input), estimate_message_tokens(&out));
        assert_eq!(t["original_tokens"], o.to_string());
        assert_eq!(t["compressed_tokens"], k.to_string());
        assert_eq!(t["compression_ratio"], format!("{:.1}x", o as f64 / k as f64));
        assert_eq!(t["summarization_calls"], backends.count().to_string());
        assert_eq!(t["compression_layer"], "semantic");
        assert!(t.contains_key("tokens_saved"));
    }

    /// A transcript with nothing it may summarize is returned as it came.
    #[tokio::test]
    async fn a_transcript_of_only_pinned_turns_is_untouched() {
        let backends = Fake::new(Reply::Short);
        let input = transcript(2);
        let c = SemanticCompressor::new(backends.clone(), config(Some("cheap")));
        let (out, t) = c.compress("main", input.clone()).await.unwrap();
        assert_eq!(backends.count(), 0);
        assert_eq!(results(&out), results(&input));
        assert_eq!(t["compression_layer"], "none");
    }
}
