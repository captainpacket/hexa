//! Codex inference adapter: `codex exec` behind the same `ProcessSpawner` as
//! `ClaudeCodeInferenceAdapter` (ADR-2611231735). A skeleton: it builds, and
//! every call says it is not built yet, so the sealed tests are red until it is.

use std::sync::Arc;

use async_trait::async_trait;

use hexa_core::ports::inference::{
    futures_stream, HealthStatus, IInferencePort, InferenceCapabilities, InferenceError, InferenceRequest,
    InferenceResponse, StreamChunk,
};

use super::claude_code::ProcessSpawner;

/// Serves a registered model through the `codex` CLI (ADR-2611231735 §1).
pub struct CodexInferenceAdapter {
    _model: Option<String>,
    _spawner: Option<Arc<dyn ProcessSpawner>>,
}

impl CodexInferenceAdapter {
    /// Production constructor: the real process spawner.
    pub fn new(model: Option<String>) -> Self {
        Self { _model: model, _spawner: None }
    }

    /// Test constructor: a caller-supplied spawner stands in for `codex`.
    pub fn with_spawner(model: Option<String>, spawner: Arc<dyn ProcessSpawner>) -> Self {
        Self { _model: model, _spawner: Some(spawner) }
    }
}

fn not_built() -> InferenceError {
    InferenceError::ProviderUnavailable("the codex adapter is not built yet (ADR-2611231735)".to_string())
}

#[async_trait]
impl IInferencePort for CodexInferenceAdapter {
    async fn complete(&self, _request: InferenceRequest) -> Result<InferenceResponse, InferenceError> {
        Err(not_built())
    }

    async fn stream(
        &self,
        _request: InferenceRequest,
    ) -> Result<Box<dyn futures_stream::Stream<Item = StreamChunk> + Send + Unpin>, InferenceError> {
        Err(not_built())
    }

    async fn health(&self) -> Result<HealthStatus, InferenceError> {
        Ok(HealthStatus::Unreachable { reason: "not built yet".to_string() })
    }

    fn capabilities(&self) -> InferenceCapabilities {
        InferenceCapabilities {
            models: vec![],
            supports_tool_use: true,
            supports_thinking: false,
            supports_caching: false,
            supports_streaming: false,
            max_context_tokens: 0,
            cost_per_mtok_input: 0.0,
            cost_per_mtok_output: 0.0,
        }
    }
}
