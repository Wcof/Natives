//! Agent Engine — single execution authority for a Run.
//!
//! Flow: prepare → provider stream → (tool loop) → terminal event.
//! Tools and credentials are injected via seams so the daemon can supply
//! capability-gateway + credential broker without circular deps.
//!
//! Module layout (see `mod.rs`): provider/tool contracts live in
//! [`super::provider`] / [`super::tool_runtime`], message conversion in
//! [`super::conversion`], and the error type in [`super::error`]. This file
//! keeps the [`AgentEngine`] state machine and its entry points only.
//!
//! W9: the run loop moved to `engine_run`, event helpers to `engine_events`,
//! input drain to `engine_input`, safe points to `engine_safe_point`, tool
//! execution to `engine_tools`, and compaction to `engine_compaction`.

use super::error::EngineError;
use super::provider::*;
use super::tool_runtime::{
    EngineToolRuntime, NoopToolProgressSink, ToolProgressSink, ToolSchema,
};

use crate::context::ContextStats;
use crate::event_seq::EventSequencer;
use crate::hooks::{HookEvent, HookRegistry, HookRequest};
use crate::live_event::LiveEventBus;
use serde_json::json;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// Live run handle.
pub struct AgentEngine {
    pub events: EventSequencer,
    /// Ephemeral live event sink (memory-only, bounded broadcast).
    /// High-frequency deltas go here, never to the durable store.
    pub live: LiveEventBus,
    pub(crate) cancel: CancellationToken,
    pub(crate) hooks: HookRegistry,
    /// Optional session coordinator for interjection / safe-point drain.
    pub(crate) session_harness: Option<Arc<crate::session_coordinator::SessionCoordinator>>,
    /// Optional soft char budget override for history compaction.
    pub(crate) history_compact_chars: Option<usize>,
    /// Optional max chars kept per tool output after compaction.
    pub(crate) tool_output_max_chars: Option<usize>,
    /// Ask the model for a structured summary when compacting (default on).
    pub(crate) model_compaction: bool,
    pub(crate) summary_attempts: AtomicU32,
    pub(crate) summary_failures: AtomicU32,
    pub(crate) progress_sink: Arc<dyn ToolProgressSink>,
    pub(crate) input_receiver: Option<Arc<dyn crate::EngineInputReceiver>>,
    pub(crate) safe_point_receiver: Option<Arc<dyn crate::EngineSafePointReceiver>>,
    pub(crate) provider_context_window: Option<u64>,
    /// PERF-001: running context budget stats maintained by the engine loop.
    /// `observe_transcript` appends only the per-round transcript growth; a
    /// compaction pass bumps the revision and resets the counters to the kept
    /// transcript size.
    pub(crate) context_stats: Mutex<ContextStats>,
}

impl AgentEngine {
    pub fn new(events: EventSequencer) -> Self {
        Self {
            events,
            live: LiveEventBus::new(),
            cancel: CancellationToken::new(),
            hooks: HookRegistry::new(),
            session_harness: None,
            history_compact_chars: None,
            tool_output_max_chars: None,
            model_compaction: true,
            summary_attempts: AtomicU32::new(0),
            summary_failures: AtomicU32::new(0),
            progress_sink: Arc::new(NoopToolProgressSink),
            input_receiver: None,
            safe_point_receiver: None,
            provider_context_window: None,
            context_stats: Mutex::new(ContextStats::new()),
        }
    }

    /// Construct with an explicit live event bus.
    ///
    /// `AgentEngine::new` creates an internal default [`LiveEventBus`]; this
    /// constructor lets a daemon share one live sink across engines or inject a
    /// custom bounded broadcast.
    pub fn with_live(events: EventSequencer, live: LiveEventBus) -> Self {
        Self {
            events,
            live,
            cancel: CancellationToken::new(),
            hooks: HookRegistry::new(),
            session_harness: None,
            history_compact_chars: None,
            tool_output_max_chars: None,
            model_compaction: true,
            summary_attempts: AtomicU32::new(0),
            summary_failures: AtomicU32::new(0),
            progress_sink: Arc::new(NoopToolProgressSink),
            input_receiver: None,
            safe_point_receiver: None,
            provider_context_window: None,
            context_stats: Mutex::new(ContextStats::new()),
        }
    }

    /// Disable the summarization round trip and keep compaction mechanical.
    /// Useful for cost-sensitive or offline runs; failure already degrades here.
    pub fn with_model_compaction(mut self, enabled: bool) -> Self {
        self.model_compaction = enabled;
        self
    }

    /// Use a registry-owned cancel token (task-03). Prefer over the engine-local root.
    pub fn with_cancel_token(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = hooks.with_events(self.events.clone());
        self
    }

    pub fn with_session_harness(
        mut self,
        harness: Arc<crate::session_coordinator::SessionCoordinator>,
    ) -> Self {
        self.session_harness = Some(harness);
        self
    }

    /// Alias of [`Self::with_session_harness`] using the coordinator name.
    pub fn with_session_coordinator(
        self,
        coordinator: Arc<crate::session_coordinator::SessionCoordinator>,
    ) -> Self {
        self.with_session_harness(coordinator)
    }

    /// Override in-engine history compaction thresholds (Context Budget).
    pub fn with_context_budget(mut self, history_chars: usize, tool_output_chars: usize) -> Self {
        self.history_compact_chars = Some(history_chars.max(1_000));
        self.tool_output_max_chars = Some(tool_output_chars.max(256));
        self
    }

    pub fn with_progress_sink(mut self, sink: Arc<dyn ToolProgressSink>) -> Self {
        self.progress_sink = sink;
        self
    }

    pub fn with_input_receiver(mut self, receiver: Arc<dyn crate::EngineInputReceiver>) -> Self {
        self.input_receiver = Some(receiver);
        self
    }

    pub fn with_safe_point_receiver(
        mut self,
        receiver: Arc<dyn crate::EngineSafePointReceiver>,
    ) -> Self {
        self.safe_point_receiver = Some(receiver);
        self
    }

    pub fn with_provider_context_window(mut self, window: Option<u64>) -> Self {
        self.provider_context_window = window;
        self
    }

    /// Shared cancel token for this engine run (clone freely; cancel is cooperative).
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Compatibility alias for [`Self::cancel_token`].
    /// Prefer `cancel_token()` / `request_cancel()` / `is_cancelled()`.
    pub fn cancel_flag(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn request_cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Execute a full agent loop against the given seams.
    /// (drain_inputs / append_critical / close_failed_turn / sleep_provider_backoff /
    /// apply_safe_point moved to engine_input / engine_events / engine_safe_point — W9;
    /// run_inner moved to engine_run — W9.)
    pub async fn run(
        &self,
        config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
    ) -> Result<crate::EngineOutcome, EngineError> {
        let tool_schemas = tools.list_tool_schemas().await;
        self.run_with_tool_schemas(config, provider, tools, tool_schemas)
            .await
    }

    /// Execute with the exact Tool Plan frozen by Run-start authority.
    ///
    /// Production callers use this path so schema discovery happens once and
    /// the provider sees the same bounded schemas recorded as Run evidence.
    pub async fn run_with_tool_schemas(
        &self,
        config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
        tool_schemas: Vec<ToolSchema>,
    ) -> Result<crate::EngineOutcome, EngineError> {
        self.run_with_typed_transcript(config, provider, tools, tool_schemas, None)
            .await
    }

    /// Production entry point for a transcript that has already crossed the
    /// daemon's typed persistence boundary.  The legacy `EngineMessage` field
    /// remains available to fixture/compatibility callers, but production does
    /// not flatten typed blocks and immediately rebuild them.
    pub async fn run_with_typed_messages(
        &self,
        config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
        tool_schemas: Vec<ToolSchema>,
        messages: Vec<crate::AgentMessage>,
    ) -> Result<crate::EngineOutcome, EngineError> {
        self.run_with_typed_transcript(config, provider, tools, tool_schemas, Some(messages))
            .await
    }

    async fn run_with_typed_transcript(
        &self,
        config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
        tool_schemas: Vec<ToolSchema>,
        typed_transcript: Option<Vec<crate::AgentMessage>>,
    ) -> Result<crate::EngineOutcome, EngineError> {
        let run_id = config.run_id.clone();
        let result = self
            .run_inner(config, provider, tools, tool_schemas, typed_transcript)
            .await;
        if let Err(error) = &result {
            // Terminal telemetry: the run already failed; a hook decision here
            // cannot be honoured, and the outcome must not be re-decided by a
            // telemetry event. The frozen post-contract (T03) therefore treats
            // Error as observation-only and records rather than acts.
            let _ = self
                .hooks
                .dispatch(HookRequest {
                    event: HookEvent::Error,
                    run_id: run_id.clone(),
                    tool_name: None,
                    input: json!({
                        "code": error.code(),
                        "message": error.to_string(),
                    }),
                })
                .await;
        }
        // Terminal telemetry: the session is over, so a decision cannot alter
        // the committed outcome. Observation-only by the frozen post-contract
        // (T03); the dispatch result is intentionally dropped.
        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::SessionEnd,
                run_id,
                tool_name: None,
                input: json!({
                    "success": result.is_ok(),
                }),
            })
            .await;
        result
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
