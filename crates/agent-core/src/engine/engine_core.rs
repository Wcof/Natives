//! Agent Engine — single execution authority for a Run.
//!
//! Flow: prepare → provider stream → (tool loop) → terminal event.
//! Tools and credentials are injected via seams so the daemon can supply
//! capability-gateway + credential broker without circular deps.

use crate::compaction::{
    apply_model_summary, choose_summary_split, compact_messages as compact_tool_history,
    render_transcript_for_summary, repair_dangling_tool_calls, CompactResult,
    SUMMARY_SYSTEM_PROMPT,
};
use crate::context::ContextStats;
use crate::doom_loop::DoomLoopDetector;
use crate::event_seq::EventSequencer;
use crate::hooks::{HookDecision, HookEvent, HookRegistry, HookRequest};
use crate::live_event::LiveEventBus;
use assistant_protocol::v2::RunEventKind;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// Soft budget for in-engine history characters before tool-output compaction.
const HISTORY_COMPACT_CHARS: usize = 48_000;
const TOOL_OUTPUT_MAX_CHARS: usize = 4_000;

/// Messages kept verbatim at the tail when a model summary replaces the prefix.
const SUMMARY_KEEP_TAIL_MESSAGES: usize = 6;
/// Below this many summarizable messages a provider round trip is not worth it.
const SUMMARY_MIN_PREFIX_MESSAGES: usize = 4;
/// Upper bound on the transcript handed to the summarizer (cost boundary).
const SUMMARY_TRANSCRIPT_MAX_CHARS: usize = 60_000;
/// Per-message truncation inside that transcript.
const SUMMARY_MESSAGE_MAX_CHARS: usize = 2_000;
/// Wall clock ceiling for one summarization round trip.
const SUMMARY_TIMEOUT_MS: u64 = 60_000;
/// Total model summarizations attempted by one engine, successful or not.
const SUMMARY_MAX_ATTEMPTS: u32 = 8;
/// After this many failures the engine stops paying for summarization and
/// stays on mechanical compaction for the rest of the run.
const SUMMARY_MAX_FAILURES: u32 = 2;

/// Tool call after PreToolUse hooks, ready for (possibly parallel) execution.
#[derive(Debug, Clone)]
struct PreparedToolCall {
    id: String,
    name: String,
    args: String,
    input: Value,
    rejected: Option<ToolExecutionResult>,
    parallel_safe: bool,
    conflict_key: Option<String>,
}

/// Tool call after execution (or hook denial), in original order.
#[derive(Debug, Clone)]
struct ExecutedToolCall {
    id: String,
    name: String,
    args: String,
    result: Option<ToolExecutionResult>,
    /// Stable ToolResult message id committed in `ToolCallCompleted` so the
    /// daemon's event→SQLite replay reuses it instead of inventing a new one.
    result_message_id: Option<String>,
}

fn is_long_running_tool_result(result: &ToolExecutionResult) -> bool {
    result
        .output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "running" | "pending"))
}

/// Tool execution seam used by the engine.
#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)] // public trait: signature is frozen for implementors
pub trait EngineToolRuntime: Send + Sync {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema>;

    /// Capability metadata is supplied by the Gateway. Unknown tools are
    /// intentionally sequential so the core fails closed.
    async fn list_tool_capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }
    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
    ) -> ToolExecutionResult;

    /// Execute with a stable Core ToolCall id so gateway output and ledger
    /// records cannot drift to a second per-handler UUID.
    async fn execute_tool_with_call_id(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        _call_id: Option<&str>,
    ) -> ToolExecutionResult {
        self.execute_tool_with_call_id_and_progress(
            name,
            input,
            cancel,
            _call_id,
            None,
            None,
            Arc::new(NoopToolProgressSink),
        )
        .await
    }

    /// Optional call-identity-aware execution hook for runtimes that can emit
    /// live handler progress. The default keeps lightweight runtimes compatible.
    async fn execute_tool_with_call_id_and_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        _call_id: Option<&str>,
        _turn_id: Option<&str>,
        _message_id: Option<&str>,
        _progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        self.execute_tool(name, input, cancel).await
    }

    async fn execute_tool_with_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        _progress: &dyn ToolProgressSink,
    ) -> ToolExecutionResult {
        self.execute_tool(name, input, cancel).await
    }

    /// Progress variant carrying Core's stable ToolCall identity.  The older
    /// method remains as a compatibility hook for lightweight runtimes.
    async fn execute_tool_with_progress_for_call(
        &self,
        call_id: &str,
        _turn_id: Option<&str>,
        _message_id: Option<&str>,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        let _ = call_id;
        self.execute_tool_with_progress(name, input, cancel, progress.as_ref())
            .await
    }

    /// Optional batch entry for same-turn `task` tool calls.
    ///
    /// Default falls back to sequential `execute_tool("task", ...)`.
    /// Native Runtime overrides this to emit **one** `subagent_assignment`
    /// interaction for the whole batch, then start children by `call_id`.
    async fn execute_task_batch(
        &self,
        tasks: Vec<(String, Value)>,
        cancel: &CancellationToken,
    ) -> Vec<ToolExecutionResult> {
        let mut out = Vec::with_capacity(tasks.len());
        for (_call_id, input) in tasks {
            if cancel.is_cancelled() {
                out.push(ToolExecutionResult {
                    output: json!({"error": "cancelled"}),
                    is_error: true,
                    duration_ms: 0,
                });
                continue;
            }
            out.push(self.execute_tool("task", input, cancel).await);
        }
        out
    }

    /// Progress-aware batch seam for subagent tools. The default preserves
    /// compatibility with lightweight runtimes; production runtimes can keep
    /// the sink alive while child Runs emit their own events.
    async fn execute_task_batch_with_progress(
        &self,
        tasks: Vec<(String, Value)>,
        cancel: &CancellationToken,
        _progress: Arc<dyn ToolProgressSink>,
        _turn_id: Option<&str>,
        _message_id: Option<&str>,
    ) -> Vec<ToolExecutionResult> {
        self.execute_task_batch(tasks, cancel).await
    }

    /// Called when a handler returned but the authoritative completion fact
    /// could not be persisted. Production runtimes record this as `uncertain`
    /// so resume code cannot replay an unknown side effect. Returns Err when
    /// the uncertain fact itself could not be persisted — the caller must then
    /// fail the run with a `recovery_blocked` terminal so a later resume can
    /// never assume a side effect it cannot prove.
    async fn mark_tool_call_uncertain(
        &self,
        _call_id: &str,
        _name: &str,
        _turn_id: Option<&str>,
        _input: &Value,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionMode {
    ParallelSafe,
    Sequential,
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCapability {
    pub name: String,
    pub schema: Value,
    pub execution_mode: ToolExecutionMode,
    pub side_effect: ToolSideEffect,
    pub conflict_key: Option<String>,
}

/// Provider-neutral safety classification advertised by the Gateway.
/// Core never infers this from a tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSideEffect {
    ReadOnly,
    Write,
    Destructive,
    Network,
    Process,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub output: Value,
    pub is_error: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolProgressUpdate {
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub stream: String,
    pub text: String,
    pub final_update: bool,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
    pub progress_sequence: u64,
}

#[async_trait::async_trait]
pub trait ToolProgressSink: Send + Sync {
    async fn publish(&self, update: ToolProgressUpdate);

    async fn mark_tool_call_settled(&self, _tool_call_id: &str) {}
}

#[derive(Debug, Default)]
pub struct NoopToolProgressSink;

#[async_trait::async_trait]
impl ToolProgressSink for NoopToolProgressSink {
    async fn publish(&self, _update: ToolProgressUpdate) {}
}

/// Provider stream seam (maps onto provider-adapters without hard dep).
#[async_trait::async_trait]
pub trait EngineProvider: Send + Sync {
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError>;

    /// Context-aware provider call. The default preserves existing lightweight
    /// providers while production providers can bind credentials to the real
    /// Run identity and Attempt number.
    async fn stream_with_context(
        &self,
        _context: EngineProviderContext,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        self.stream(model, messages, tools, system_prompt, cancel)
            .await
    }

    async fn stream_turn(
        &self,
        request: ProviderTurnRequest,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        self.stream_with_context(
            request.context,
            &request.model,
            agent_messages_to_engine_messages(&request.messages),
            &request.tools,
            request.system_prompt.as_deref(),
            cancel,
        )
        .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineProviderContext {
    pub run_id: String,
    pub attempt: u32,
}

#[derive(Debug, Clone)]
pub struct ProviderTurnRequest {
    pub context: EngineProviderContext,
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<crate::AgentMessage>,
    pub tools: Vec<ToolSchema>,
}

pub type ProviderTurnEvent = EngineProviderEvent;

pub type EngineProviderEventStream =
    Pin<Box<dyn Stream<Item = EngineProviderEvent> + Send + 'static>>;

/// One message on the wire between the engine and a provider.
///
/// # Why `content` stays a `String`
///
/// Text is what every step of the loop reads and writes — doom-loop
/// fingerprints, compaction, transcripts, hook payloads. Turning `content` into
/// a block list would have rewritten all of them for the sake of one extra
/// modality. Non-text parts therefore ride in their own typed field instead:
/// `content` remains the text fast path, and [`Self::images`] carries what text
/// cannot. Adding a modality later means adding a field, not reshaping this one.
///
/// The rule that makes this honest: every layer below must either encode
/// `images` or say out loud that it could not (see
/// `provider_adapters::ImageSource::degraded_note`). Silently dropping them is
/// the bug this field exists to close.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineMessage {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    /// Tool name for `role: tool` results (needed by Gemini functionResponse).
    pub tool_name: Option<String>,
    pub tool_calls: Option<Vec<EngineToolCall>>,
    /// Images attached to this message. Empty for the overwhelming majority of
    /// messages, which is why it is a plain `Vec` rather than an `Option`.
    pub images: Vec<EngineImage>,
}

impl EngineMessage {
    /// Text-only message — the shape almost every call site wants.
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        EngineMessage {
            role: role.into(),
            content: content.into(),
            ..Default::default()
        }
    }

    /// Attach images to a message.
    pub fn with_images(mut self, images: Vec<EngineImage>) -> Self {
        self.images = images;
        self
    }
}

/// An image attached to an [`EngineMessage`].
///
/// Deliberately mirrors `provider_adapters::ImageSource` without depending on
/// it — `agent-core` has no provider dependency, and the daemon owns the
/// translation (see `production::engine_message_to_history`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineImage {
    /// A `data:` URI carrying inline base64 bytes, or a reference the provider
    /// resolves itself (`https://`, `gs://`, a Google File API URI).
    pub url: String,
    /// MIME type when the URL does not carry one. Required by several providers
    /// for inline data; they degrade loudly rather than guess when it is absent.
    pub media_type: Option<String>,
    /// Provider-specific fidelity hint (`"low"` / `"high"` / `"auto"`).
    pub detail: Option<String>,
}

impl EngineImage {
    pub fn new(url: impl Into<String>) -> Self {
        EngineImage {
            url: url.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub enum EngineProviderEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: Option<u64>,
        /// Prompt-cache writes reported by the provider, when it reports them.
        ///
        /// `None` (not reported) is deliberately distinct from `Some(0)` (the
        /// provider reported no cache activity) — the daemon persists the two
        /// differently, and collapsing them would make an unsupported provider
        /// indistinguishable from a cache miss.
        cache_creation_tokens: Option<u64>,
        /// Prompt-cache reads reported by the provider, when it reports them.
        cache_read_tokens: Option<u64>,
    },
    /// Legacy completion used by fixture providers; treated as a reliable stop.
    Completed,
    CompletedWithReason {
        reason: ProviderStopReason,
    },
    Error {
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStopReason {
    Stop,
    ToolUse,
    Length,
    Cancelled,
    Error,
    Unknown(String),
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
    #[error("{message}")]
    Provider {
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    },
    #[error("cancelled")]
    Cancelled,
    /// The detector's verdict travels with the error. "doom loop detected" on
    /// its own tells a user nothing they can act on; the reason names the
    /// signal, the cycle length and the repeating steps.
    #[error("doom loop detected: {0}")]
    DoomLoop(crate::doom_loop::DoomLoopReason),
    #[error("max steps exceeded")]
    MaxSteps,
    /// The authoritative completion fact AND the uncertain ledger recording
    /// both failed to persist. The run must terminate with a `recovery_blocked`
    /// code so a later resume never assumes a side effect it cannot prove.
    #[error("recovery_blocked: {0}")]
    RecoveryBlocked(String),
    /// An observation-only post hook (PostToolUse / PostToolUseFailure /
    /// PostCompact) returned a decision that cannot be honoured after the
    /// observed outcome was committed. The refusal is surfaced loudly instead
    /// of being silently ignored (T03).
    #[error("hook_refused: {0}")]
    HookRefused(String),
}

impl EngineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Provider { code, .. } => code,
            Self::Cancelled => "cancelled",
            Self::DoomLoop(_) => "doom_loop",
            Self::MaxSteps => "max_steps",
            Self::Message(_) => "provider",
            Self::RecoveryBlocked(_) => "recovery_blocked",
            Self::HookRefused(_) => "hook_refused",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Provider {
                retryable: true,
                ..
            }
        )
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Provider { category, .. } if category == "RateLimit")
    }

    /// Provider-neutral context-overflow classification (TASK-011 / C03-I03).
    /// Adapters map overflow to `ProviderErrorCategory::ContextLengthExceeded`;
    /// the engine uses this to trigger a bounded one-shot compaction retry,
    /// never an unbounded retry loop.
    pub fn is_context_overflow(&self) -> bool {
        matches!(
            self,
            Self::Provider { category, .. }
                if category == "ContextLengthExceeded"
                    || category == "ContextWindowExceeded"
        )
    }

    /// Delay the provider asked us to wait, if it named one.
    ///
    /// This is the value the retry loop feeds into [`provider_backoff_ms`]; it
    /// used to be carried on the error and never read, which is how a 429 could
    /// be retried three times inside two seconds.
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::Provider { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        }
    }
}

/// Configuration for one engine run.
#[derive(Debug, Clone)]
pub struct EngineRunConfig {
    pub run_id: String,
    pub conversation_id: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<EngineMessage>,
    pub user_content: String,
    pub max_steps: u32,
}

/// Live run handle.
pub struct AgentEngine {
    pub events: EventSequencer,
    /// Ephemeral live event sink (memory-only, bounded broadcast).
    /// High-frequency deltas go here, never to the durable store.
    pub live: LiveEventBus,
    cancel: CancellationToken,
    hooks: HookRegistry,
    /// Optional session coordinator for interjection / safe-point drain.
    session_harness: Option<Arc<crate::session_coordinator::SessionCoordinator>>,
    /// Optional soft char budget override for history compaction.
    history_compact_chars: Option<usize>,
    /// Optional max chars kept per tool output after compaction.
    tool_output_max_chars: Option<usize>,
    /// Ask the model for a structured summary when compacting (default on).
    model_compaction: bool,
    summary_attempts: AtomicU32,
    summary_failures: AtomicU32,
    progress_sink: Arc<dyn ToolProgressSink>,
    input_receiver: Option<Arc<dyn crate::EngineInputReceiver>>,
    safe_point_receiver: Option<Arc<dyn crate::EngineSafePointReceiver>>,
    provider_context_window: Option<u64>,
    /// PERF-001: running context budget stats maintained by the engine loop.
    /// `observe_transcript` appends only the per-round transcript growth; a
    /// compaction pass bumps the revision and resets the counters to the kept
    /// transcript size.
    context_stats: Mutex<ContextStats>,
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

    async fn drain_inputs(
        &self,
        kind: crate::PendingInputKind,
        mode: crate::DrainMode,
        point: crate::InputSafePoint,
        messages: &mut Vec<crate::AgentMessage>,
        turn_id: Option<&str>,
    ) -> Result<bool, EngineError> {
        let Some(receiver) = &self.input_receiver else {
            return Ok(false);
        };
        let pending = receiver
            .drain(kind, mode, point)
            .await
            .map_err(|error| EngineError::Message(format!("input drain failed: {error}")))?;
        if pending.is_empty() {
            return Ok(false);
        }
        for input in pending {
            receiver
                .ack(&input, turn_id)
                .await
                .map_err(|error| EngineError::Message(format!("input ack failed: {error}")))?;
            let label = match input.kind {
                crate::PendingInputKind::Steering => "steering",
                crate::PendingInputKind::FollowUp => "follow_up",
            };
            let content = if input.content.starts_with(&format!("[{label}]\n")) {
                input.content.clone()
            } else {
                format!("[{label}]\n{}", input.content)
            };
            messages.push(crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::from(format!("queue:{}", input.id)),
                content: vec![crate::ContentBlock::Text { text: content }],
            }));
        }
        Ok(true)
    }

    fn append_critical(&self, run_id: &str, event: RunEventKind) -> Result<(), EngineError> {
        self.events
            .append_checked(run_id, event)
            .map(|_| ())
            .map_err(EngineError::Message)
    }

    fn close_failed_turn(
        &self,
        run_id: &str,
        turn_id: &crate::TurnId,
        message_id: &crate::MessageId,
        stop_reason: &str,
        text: &str,
        reasoning: &str,
    ) -> Result<(), EngineError> {
        let mut content = Vec::new();
        if !reasoning.is_empty() {
            content.push(crate::ContentBlock::Thinking {
                text: reasoning.to_string(),
                signature: None,
            });
        }
        if !text.is_empty() {
            content.push(crate::ContentBlock::Text {
                text: text.to_string(),
            });
        }
        self.append_critical(
            run_id,
            RunEventKind::MessageCompleted {
                turn_id: turn_id.to_string(),
                message_id: message_id.to_string(),
                role: "assistant".into(),
                content: Some(json!({
                    "message_id": message_id.to_string(),
                    "role": "assistant",
                    "content": content,
                })),
            },
        )?;
        self.append_critical(
            run_id,
            RunEventKind::TurnCompleted {
                turn_id: turn_id.to_string(),
                stop_reason: stop_reason.to_string(),
                input_tokens: 0,
                output_tokens: 0,
            },
        )
    }

    /// Wait out a provider backoff before the next generation attempt.
    ///
    /// Returns `false` when the run was cancelled mid-wait; the caller must
    /// unwind instead of retrying. A 60s `Retry-After` that ignored cancel
    /// would make Stop feel broken, so the wait races the run's cancel token.
    async fn sleep_provider_backoff(&self, attempt: u32, retry_after_ms: Option<u64>) -> bool {
        let delay = provider_backoff_ms(attempt, retry_after_ms);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                !self.cancel.is_cancelled()
            }
            _ = self.cancel.cancelled() => false,
        }
    }

    /// Apply coordinator action at a safe point: inject interjection into messages.
    async fn apply_safe_point(
        &self,
        conversation_id: &str,
        point: crate::session_coordinator::SafePoint,
        messages: &mut Vec<crate::AgentMessage>,
    ) -> Result<(), EngineError> {
        let input_point = match point {
            crate::session_coordinator::SafePoint::AfterTool
            | crate::session_coordinator::SafePoint::BeforeTool
            | crate::session_coordinator::SafePoint::AfterPermissionResolved => {
                crate::InputSafePoint::AfterToolBatch
            }
            crate::session_coordinator::SafePoint::ProviderBatchBoundary => {
                crate::InputSafePoint::BeforeProvider
            }
        };
        let content = if let Some(receiver) = &self.safe_point_receiver {
            receiver.on_safe_point(input_point).await.map_err(|error| {
                EngineError::Message(format!("safe-point persistence failed: {error}"))
            })?
        } else if let Some(harness) = &self.session_harness {
            match harness.on_safe_point(conversation_id, point) {
                crate::session_coordinator::CoordinatorAction::InjectInterjection { content } => {
                    Some(content)
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(content) = content {
            messages.push(crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::new(),
                content: vec![crate::ContentBlock::Text {
                    text: format!("[interjection]\n{content}"),
                }],
            }));
        }
        Ok(())
    }

    /// Execute a full agent loop against the given seams.
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
                    input: serde_json::json!({
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
                input: serde_json::json!({
                    "success": result.is_ok(),
                }),
            })
            .await;
        result
    }

    async fn run_inner(
        &self,
        mut config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
        tool_schemas: Vec<ToolSchema>,
        typed_transcript: Option<Vec<crate::AgentMessage>>,
    ) -> Result<crate::EngineOutcome, EngineError> {
        use crate::EngineOutcome;
        let run_id_owned = config.run_id.clone();
        let run_id = &run_id_owned;
        let tool_capabilities: BTreeMap<String, ToolCapability> = tools
            .list_tool_capabilities()
            .await
            .into_iter()
            .map(|capability| (capability.name.clone(), capability))
            .collect();
        // Lifecycle status is owned by RunManager::commit_transition.
        // Engine only emits domain events and returns EngineOutcome.
        let session_start = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::SessionStart,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "conversation_id": config.conversation_id }),
            })
            .await;
        let mut injected_hook_messages = apply_prompt_hook_responses(&mut config, session_start)?;
        let prompt_submit = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::UserPromptSubmit,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "content": config.user_content }),
            })
            .await;
        injected_hook_messages.extend(apply_prompt_hook_responses(&mut config, prompt_submit)?);

        // History is prior turns; always ensure the current user prompt appears
        // exactly once. Production supplies typed history directly. Legacy
        // callers cross the conversion boundary once here and never re-enter it.
        let mut typed_messages = if let Some(messages) = typed_transcript {
            messages
        } else {
            let initial_messages = if config.messages.is_empty() {
                vec![EngineMessage {
                    role: "user".into(),
                    content: config.user_content.clone(),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                    images: Vec::new(),
                }]
            } else {
                let mut msgs = config.messages.clone();
                let already_has_current = msgs.last().is_some_and(|m| {
                    m.role == "user" && m.content.trim() == config.user_content.trim()
                });
                if !already_has_current && !config.user_content.trim().is_empty() {
                    msgs.push(EngineMessage {
                        role: "user".into(),
                        content: config.user_content.clone(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        images: Vec::new(),
                    });
                }
                msgs
            };
            engine_messages_to_agent_messages(&initial_messages)
        };
        if !config.user_content.trim().is_empty()
            && !typed_messages.last().is_some_and(|message| {
                matches!(
                    message,
                    crate::AgentMessage::User(value)
                        if value.content.iter().any(|block| matches!(
                            block,
                            crate::ContentBlock::Text { text } if text.trim() == config.user_content.trim()
                        ))
                )
            })
        {
            typed_messages.push(crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::new(),
                content: vec![crate::ContentBlock::Text {
                    text: config.user_content.clone(),
                }],
            }));
        }
        // Hook Inject must reach the typed transcript (and therefore the next
        // ProviderTurnRequest), not a side-channel EngineMessage list. Injected
        // content becomes leading system context: deterministic position, safe
        // for safety instructions, and identical for legacy and typed callers.
        if !injected_hook_messages.is_empty() {
            typed_messages.splice(
                0..0,
                injected_hook_messages.into_iter().map(|text| {
                    crate::AgentMessage::System(crate::SystemMessage {
                        message_id: crate::MessageId::new(),
                        text,
                    })
                }),
            );
        }
        let mut doom = DoomLoopDetector::new();
        let mut step = 0u32;

        loop {
            if self.cancel.is_cancelled() {
                return Ok(EngineOutcome::Cancelled);
            }
            step += 1;
            if step > config.max_steps {
                return Err(EngineError::MaxSteps);
            }

            let turn_id = crate::TurnId::new();
            let assistant_message_id = crate::MessageId::new();
            self.append_critical(
                run_id,
                RunEventKind::TurnStarted {
                    turn_id: turn_id.to_string(),
                },
            )?;
            self.append_critical(
                run_id,
                RunEventKind::MessageStarted {
                    turn_id: turn_id.to_string(),
                    message_id: assistant_message_id.to_string(),
                    role: "assistant".into(),
                },
            )?;

            const MAX_PROVIDER_ATTEMPTS: u32 = 3;
            let mut attempt = 1u32;
            // One-shot overflow policy (C03-I03): each user input may be
            // compacted at most once in response to a context-overflow error;
            // a second overflow fails without looping.
            let mut overflow_compacted = false;
            let (text_acc, reasoning_acc, tool_acc, stop_reason) = 'attempts: loop {
                self.events.append(
                    run_id,
                    RunEventKind::GenerationAttemptStarted {
                        attempt,
                        max_attempts: MAX_PROVIDER_ATTEMPTS,
                    },
                );

                let provider_events = match provider
                    .stream_turn(
                        ProviderTurnRequest {
                            context: EngineProviderContext {
                                run_id: run_id.to_string(),
                                attempt,
                            },
                            model: config.model.clone(),
                            system_prompt: config.system_prompt.clone(),
                            messages: typed_messages.clone(),
                            tools: tool_schemas.clone(),
                        },
                        self.cancel.clone(),
                    )
                    .await
                {
                    Ok(stream) => stream,
                    Err(EngineError::Cancelled) => {
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            "",
                            "",
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    Err(_e) if self.cancel.is_cancelled() => {
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            "",
                            "",
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    Err(e) if e.retryable() && attempt < MAX_PROVIDER_ATTEMPTS => {
                        // Rate limits used to skip the backoff entirely and
                        // retry immediately, which is the one case where the
                        // provider explicitly told us not to.
                        let delay = provider_backoff_ms(attempt, e.retry_after_ms());
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: true,
                                retrying: true,
                                retry_in_ms: Some(delay),
                            },
                        );
                        if !self
                            .sleep_provider_backoff(attempt, e.retry_after_ms())
                            .await
                        {
                            self.close_failed_turn(
                                run_id,
                                &turn_id,
                                &assistant_message_id,
                                "cancelled",
                                "",
                                "",
                            )?;
                            return Ok(EngineOutcome::Cancelled);
                        }
                        attempt += 1;
                        continue 'attempts;
                    }
                    Err(e) if e.is_context_overflow() => {
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: false,
                                retrying: !overflow_compacted,
                                retry_in_ms: None,
                            },
                        );
                        if overflow_compacted {
                            // One-shot policy: the input was already compacted
                            // once and the provider still rejects the context.
                            // Fail immediately — never an unbounded retry loop.
                            self.close_failed_turn(
                                run_id,
                                &turn_id,
                                &assistant_message_id,
                                "error",
                                "",
                                "",
                            )?;
                            return Err(e);
                        }
                        overflow_compacted = true;
                        attempt += 1;
                        let compacted = self
                            .maybe_compact_typed_history(
                                run_id,
                                &turn_id,
                                &config.model,
                                provider,
                                typed_messages,
                            )
                            .await?;
                        typed_messages = compacted;
                        continue 'attempts;
                    }
                    Err(e) => {
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: e.retryable(),
                                retrying: false,
                                retry_in_ms: None,
                            },
                        );
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "error",
                            "",
                            "",
                        )?;
                        return Err(e);
                    }
                };

                let mut text_acc = String::new();
                let mut reasoning_acc = String::new();
                let mut tool_acc: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
                let mut saw_generation_delta = false;
                let mut completed_reason: Option<ProviderStopReason> = None;
                tokio::pin!(provider_events);

                while let Some(event) = provider_events.next().await {
                    if self.cancel.is_cancelled() {
                        if !tool_acc.is_empty() {
                            completed_reason = Some(ProviderStopReason::Cancelled);
                            break;
                        }
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            &text_acc,
                            &reasoning_acc,
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    match event {
                        EngineProviderEvent::TextDelta(t) => {
                            saw_generation_delta = true;
                            text_acc.push_str(&t);
                            // Live delta: ephemeral bus only, never durable.
                            // The cumulative MessageDelta emit is deleted
                            // (contract: live-durable-event-contract.md §Message).
                            self.live
                                .append(run_id, RunEventKind::TextDelta { text: t });
                        }
                        EngineProviderEvent::ReasoningDelta(t) => {
                            saw_generation_delta = true;
                            reasoning_acc.push_str(&t);
                            // Live delta: ephemeral bus only, never durable.
                            self.live
                                .append(run_id, RunEventKind::ReasoningDelta { text: t });
                        }
                        EngineProviderEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_delta,
                        } => {
                            saw_generation_delta = true;
                            let entry = tool_acc
                                .entry(index)
                                .or_insert_with(|| (String::new(), String::new(), String::new()));
                            if let Some(id) = id.clone() {
                                if !id.is_empty() {
                                    entry.0 = id;
                                }
                            }
                            if let Some(name) = name.clone() {
                                if !name.is_empty() {
                                    entry.1 = name;
                                }
                            }
                            entry.2.push_str(&arguments_delta);
                            // Live delta: ephemeral bus only, never durable.
                            self.live.append(
                                run_id,
                                RunEventKind::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                },
                            );
                        }
                        EngineProviderEvent::Usage {
                            input_tokens,
                            output_tokens,
                            reasoning_tokens,
                            cache_creation_tokens,
                            cache_read_tokens,
                        } => {
                            self.events.append(
                                run_id,
                                RunEventKind::UsageUpdated {
                                    input_tokens,
                                    output_tokens,
                                    reasoning_tokens,
                                    cache_creation_tokens,
                                    cache_read_tokens,
                                },
                            );
                        }
                        EngineProviderEvent::Error {
                            message,
                            code,
                            retryable,
                            category,
                            retry_after_ms,
                        } => {
                            if !saw_generation_delta && retryable && attempt < MAX_PROVIDER_ATTEMPTS
                            {
                                // Same fix as the connect path above: honour the
                                // provider's own delay instead of special-casing
                                // RateLimit into a zero-wait retry.
                                let delay = provider_backoff_ms(attempt, retry_after_ms);
                                self.events.append(
                                    run_id,
                                    RunEventKind::GenerationAttemptFailed {
                                        attempt,
                                        code: code.clone(),
                                        retryable,
                                        retrying: true,
                                        retry_in_ms: Some(delay),
                                    },
                                );
                                if !self.sleep_provider_backoff(attempt, retry_after_ms).await {
                                    self.close_failed_turn(
                                        run_id,
                                        &turn_id,
                                        &assistant_message_id,
                                        "cancelled",
                                        &text_acc,
                                        &reasoning_acc,
                                    )?;
                                    return Ok(EngineOutcome::Cancelled);
                                }
                                attempt += 1;
                                continue 'attempts;
                            }
                            if saw_generation_delta {
                                self.events.append(
                                    run_id,
                                    RunEventKind::GenerationAttemptDiscarded {
                                        attempt,
                                        reason: code.clone(),
                                    },
                                );
                            }
                            self.events.append(
                                run_id,
                                RunEventKind::GenerationAttemptFailed {
                                    attempt,
                                    code: code.clone(),
                                    retryable,
                                    retrying: false,
                                    retry_in_ms: None,
                                },
                            );
                            if !tool_acc.is_empty() {
                                completed_reason = Some(ProviderStopReason::Error);
                                break;
                            }
                            self.close_failed_turn(
                                run_id,
                                &turn_id,
                                &assistant_message_id,
                                "error",
                                &text_acc,
                                &reasoning_acc,
                            )?;
                            return Err(EngineError::Provider {
                                message,
                                code,
                                retryable,
                                category,
                                retry_after_ms,
                            });
                        }
                        EngineProviderEvent::CompletedWithReason { reason } => {
                            completed_reason = Some(reason);
                        }
                        EngineProviderEvent::Completed => {
                            completed_reason = Some(ProviderStopReason::Stop);
                        }
                    }
                }

                if self.cancel.is_cancelled() && tool_acc.is_empty() {
                    self.close_failed_turn(
                        run_id,
                        &turn_id,
                        &assistant_message_id,
                        "cancelled",
                        &text_acc,
                        &reasoning_acc,
                    )?;
                    return Ok(EngineOutcome::Cancelled);
                }

                if completed_reason.is_none() && !tool_acc.is_empty() {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptDiscarded {
                            attempt,
                            reason: "INCOMPLETE_TOOL_CALL".into(),
                        },
                    );
                    break (
                        text_acc,
                        reasoning_acc,
                        tool_acc,
                        Some(ProviderStopReason::Unknown("INCOMPLETE_TOOL_CALL".into())),
                    );
                }

                if !saw_generation_delta && attempt < 2 {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptFailed {
                            attempt,
                            code: "EMPTY_RESPONSE".into(),
                            retryable: true,
                            retrying: true,
                            retry_in_ms: Some(provider_backoff_ms(attempt, None)),
                        },
                    );
                    if !self.sleep_provider_backoff(attempt, None).await {
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            &text_acc,
                            &reasoning_acc,
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    attempt += 1;
                    continue 'attempts;
                }
                if !saw_generation_delta {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptFailed {
                            attempt,
                            code: "EMPTY_RESPONSE".into(),
                            retryable: false,
                            retrying: false,
                            retry_in_ms: None,
                        },
                    );
                    self.close_failed_turn(
                        run_id,
                        &turn_id,
                        &assistant_message_id,
                        "error",
                        &text_acc,
                        &reasoning_acc,
                    )?;
                    return Err(EngineError::Provider {
                        message: "provider returned empty response".into(),
                        code: "EMPTY_RESPONSE".into(),
                        retryable: false,
                        category: "unknown".into(),
                        retry_after_ms: None,
                    });
                }

                self.append_critical(run_id, RunEventKind::GenerationAttemptCommitted { attempt })?;
                break (text_acc, reasoning_acc, tool_acc, completed_reason);
            };

            if !text_acc.is_empty() {
                doom.observe_text(&text_acc);
            }
            if let Some(reason) = doom.diagnose() {
                self.close_failed_turn(
                    run_id,
                    &turn_id,
                    &assistant_message_id,
                    "error",
                    &text_acc,
                    &reasoning_acc,
                )?;
                return Err(EngineError::DoomLoop(reason));
            }

            if tool_acc.is_empty() {
                // Commit the assistant message before consuming a follow-up.
                // Otherwise the next provider request would lose the response
                // that caused the safe-point transition.
                let mut assistant_content = Vec::new();
                if !reasoning_acc.is_empty() {
                    assistant_content.push(crate::ContentBlock::Thinking {
                        text: reasoning_acc.clone(),
                        signature: None,
                    });
                }
                if !text_acc.is_empty() {
                    assistant_content.push(crate::ContentBlock::Text {
                        text: text_acc.clone(),
                    });
                }
                typed_messages.push(crate::AgentMessage::Assistant(crate::AssistantMessage {
                    message_id: assistant_message_id.clone(),
                    content: assistant_content.clone(),
                    stop_reason: Some(
                        match stop_reason.clone().unwrap_or(ProviderStopReason::Stop) {
                            ProviderStopReason::Stop => crate::StopReason::Stop,
                            ProviderStopReason::ToolUse => crate::StopReason::ToolUse,
                            ProviderStopReason::Length => crate::StopReason::Length,
                            ProviderStopReason::Cancelled => crate::StopReason::Cancelled,
                            ProviderStopReason::Error => crate::StopReason::Error,
                            ProviderStopReason::Unknown(raw) => crate::StopReason::Provider(raw),
                        },
                    ),
                }));
                // Close the response before consuming a follow-up. The next
                // provider turn must never begin with an uncommitted prior
                // MessageStarted/TurnStarted pair.
                let stop_label = stop_reason_label(stop_reason.as_ref());
                self.append_critical(
                    run_id,
                    RunEventKind::MessageCompleted {
                        turn_id: turn_id.to_string(),
                        message_id: assistant_message_id.to_string(),
                        role: "assistant".into(),
                        content: Some(json!({
                            "message_id": assistant_message_id.to_string(),
                            "role": "assistant",
                            "content": assistant_content,
                        })),
                    },
                )?;
                self.append_critical(
                    run_id,
                    RunEventKind::TurnCompleted {
                        turn_id: turn_id.to_string(),
                        stop_reason: stop_label,
                        input_tokens: 0,
                        output_tokens: 0,
                    },
                )?;
                let follow_up_consumed = match self
                    .drain_inputs(
                        crate::PendingInputKind::FollowUp,
                        crate::DrainMode::All,
                        crate::InputSafePoint::BeforeRunEnd,
                        &mut typed_messages,
                        Some(turn_id.0.as_str()),
                    )
                    .await
                {
                    Ok(consumed) => consumed,
                    Err(error) => return Err(error),
                };
                if follow_up_consumed {
                    // Live-lane signal (STREAM-CONTRACT-V2): high-frequency
                    // Progress is ephemeral — never write it to the durable
                    // EventSequencer.
                    self.live.append(
                        run_id,
                        RunEventKind::Progress {
                            message: "follow_up_consumed".into(),
                            percentage: None,
                        },
                    );
                    continue;
                }
                // No tools — complete. Status commit is RunManager's job.
                let stop = self
                    .hooks
                    .dispatch(HookRequest {
                        event: HookEvent::Stop,
                        run_id: run_id.to_string(),
                        tool_name: None,
                        input: serde_json::json!({ "reason": "stop" }),
                    })
                    .await;
                if let Err(reason) = HookRegistry::aggregate_allow(&stop) {
                    let _ = self
                        .hooks
                        .dispatch(HookRequest {
                            event: HookEvent::StopFailure,
                            run_id: run_id.to_string(),
                            tool_name: None,
                            input: serde_json::json!({ "reason": reason }),
                        })
                        .await;
                    return Err(EngineError::Message(format!("stop hook denied: {reason}")));
                }
                return Ok(match stop_reason.as_ref() {
                    Some(ProviderStopReason::Cancelled) => EngineOutcome::Cancelled,
                    Some(ProviderStopReason::Error) => EngineOutcome::failed(
                        "PROVIDER_STOP_ERROR",
                        "provider ended the response with an error stop reason",
                        false,
                    ),
                    Some(ProviderStopReason::Unknown(raw)) => EngineOutcome::failed(
                        "UNKNOWN_PROVIDER_STOP_REASON",
                        format!("provider ended with unknown stop reason: {raw}"),
                        false,
                    ),
                    Some(reason) => EngineOutcome::completed(stop_reason_label(Some(reason))),
                    None => EngineOutcome::completed("unknown"),
                });
            }

            // Execute tools and continue loop.
            // Phase 2: parallel_safe readonly tools may run concurrently (max 4);
            // write / process / network stay serial. Results are filled in call order.
            // No user input is injected while a provider response is being
            // prepared; the next safe point is after the complete tool batch.
            let mut prepared: Vec<PreparedToolCall> = Vec::new();
            // A tool call is executable only when the provider explicitly says
            // that the turn ended for tool use. Every other stop reason is a
            // fail-closed boundary, including provider errors/cancellation and
            // legacy streams that omit a final event.
            let fail_closed_reason = match stop_reason.as_ref() {
                Some(ProviderStopReason::ToolUse) => None,
                Some(ProviderStopReason::Length) => Some((
                    "TRUNCATED_TOOL_CALL",
                    "provider output was truncated before the tool call could be executed",
                )),
                Some(ProviderStopReason::Cancelled) => Some((
                    "CANCELLED_TOOL_CALL",
                    "provider cancelled before the tool call could be executed",
                )),
                Some(ProviderStopReason::Error) => Some((
                    "PROVIDER_ERROR_TOOL_CALL",
                    "provider ended with an error before the tool call could be executed",
                )),
                Some(ProviderStopReason::Stop) => Some((
                    "INVALID_TOOL_CALL_STOP_REASON",
                    "provider stopped without authorizing tool execution",
                )),
                Some(ProviderStopReason::Unknown(_)) | None => Some((
                    "UNKNOWN_PROVIDER_STOP_REASON",
                    "provider did not provide a reliable stop reason; tool call was not executed",
                )),
            };
            for (_index, (id, name, args)) in tool_acc {
                let id = if id.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    id
                };
                let (mut input, mut rejected) = match serde_json::from_str(&args) {
                    Ok(value) => (value, None),
                    Err(error) => (
                        json!({}),
                        Some(ToolExecutionResult {
                            output: json!({
                                "error_code": "INVALID_TOOL_ARGUMENTS",
                                "error": format!("tool arguments are not valid JSON: {error}"),
                            }),
                            is_error: true,
                            duration_ms: 0,
                        }),
                    ),
                };
                if let Some((code, message)) = fail_closed_reason {
                    rejected = Some(ToolExecutionResult {
                        output: json!({"error_code": code, "error": message}),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
                doom.observe_tool(&name, &tool_args_fingerprint(&args));
                if let Some(reason) = doom.diagnose() {
                    return Err(EngineError::DoomLoop(reason));
                }

                // PreToolUse hooks may deny or modify arguments (always serial).
                let pre = self
                    .hooks
                    .dispatch(HookRequest {
                        event: HookEvent::PreToolUse,
                        run_id: run_id.to_string(),
                        tool_name: Some(name.clone()),
                        input: input.clone(),
                    })
                    .await;
                let mut deny_reason: Option<String> = None;
                for response in pre {
                    match response.decision {
                        HookDecision::Deny { reason } => {
                            deny_reason = Some(reason);
                        }
                        HookDecision::Modify { payload } => {
                            input = payload;
                        }
                        _ => {}
                    }
                }
                if let Some(reason) = deny_reason {
                    rejected = Some(ToolExecutionResult {
                        output: json!({
                            "error_code": "HOOK_DENIED",
                            "error": reason,
                            "denied_by_hook": true
                        }),
                        is_error: true,
                        duration_ms: 0,
                    });
                }

                self.append_critical(
                    run_id,
                    RunEventKind::ToolCallRequested {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                )?;
                let capability = tool_capabilities.get(&name);
                self.append_critical(
                    run_id,
                    RunEventKind::ToolCallPrepared {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        execution_mode: capability
                            .map(|value| format!("{:?}", value.execution_mode))
                            .unwrap_or_else(|| "Sequential".into()),
                        side_effect: capability
                            .map(|value| format!("{:?}", value.side_effect))
                            .unwrap_or_else(|| "Destructive".into()),
                    },
                )?;
                let parallel_safe = matches!(
                    capability.map(|value| value.execution_mode),
                    Some(ToolExecutionMode::ParallelSafe)
                );
                prepared.push(PreparedToolCall {
                    id,
                    name,
                    args,
                    input,
                    rejected,
                    parallel_safe,
                    conflict_key: capability.and_then(|value| value.conflict_key.clone()),
                });
            }

            let executed = self
                .execute_prepared_tools(
                    run_id,
                    turn_id.0.as_str(),
                    assistant_message_id.0.as_str(),
                    tools,
                    prepared,
                )
                .await?;
            let persistence_failed = executed.iter().any(|item| {
                item.result.as_ref().is_some_and(|result| {
                    result.output.get("error_code").and_then(Value::as_str)
                        == Some("PERSISTENCE_FAILED")
                })
            });

            let mut typed_tool_calls = Vec::new();
            let mut typed_tool_results = Vec::new();
            for item in executed {
                typed_tool_calls.push(crate::ContentBlock::ToolCall(crate::ToolCall {
                    tool_call_id: crate::ToolCallId::from(item.id.clone()),
                    name: item.name.clone(),
                    arguments_json: item.args.clone(),
                }));
                if let Some(result) = item.result {
                    let code = result
                        .output
                        .get("error_code")
                        .or_else(|| result.output.get("code"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let output = result.output;
                    let result_block = output
                        .get("artifact_id")
                        .and_then(Value::as_str)
                        .map(|artifact_id| crate::ToolResultBlock::Artifact {
                            artifact_id: artifact_id.to_string(),
                            preview: output
                                .get("preview")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        })
                        .unwrap_or(crate::ToolResultBlock::Json { value: output });
                    typed_tool_results.push(crate::AgentMessage::ToolResult(
                        crate::ToolResultMessage {
                            message_id: item
                                .result_message_id
                                .map(crate::MessageId::from)
                                .unwrap_or_else(crate::MessageId::new),
                            tool_call_id: crate::ToolCallId::from(item.id),
                            tool_name: item.name,
                            content: vec![result_block],
                            is_error: result.is_error,
                            code,
                        },
                    ));
                }
            }

            let assistant_content = {
                let mut content = Vec::new();
                if !reasoning_acc.is_empty() {
                    content.push(crate::ContentBlock::Thinking {
                        text: reasoning_acc.clone(),
                        signature: None,
                    });
                }
                if !text_acc.is_empty() {
                    content.push(crate::ContentBlock::Text {
                        text: text_acc.clone(),
                    });
                }
                content.extend(typed_tool_calls);
                content
            };
            typed_messages.push(crate::AgentMessage::Assistant(crate::AssistantMessage {
                message_id: assistant_message_id.clone(),
                content: assistant_content.clone(),
                stop_reason: Some(core_stop_reason(stop_reason.as_ref())),
            }));
            typed_messages.extend(typed_tool_results);

            self.append_critical(
                run_id,
                RunEventKind::MessageCompleted {
                    turn_id: turn_id.to_string(),
                    message_id: assistant_message_id.to_string(),
                    role: "assistant".into(),
                    content: Some(json!({
                        "message_id": assistant_message_id.to_string(),
                        "role": "assistant",
                        "content": assistant_content,
                    })),
                },
            )?;
            self.append_critical(
                run_id,
                RunEventKind::TurnCompleted {
                    turn_id: turn_id.to_string(),
                    stop_reason: stop_reason_label(stop_reason.as_ref()),
                    input_tokens: 0,
                    output_tokens: 0,
                },
            )?;

            if persistence_failed {
                self.cancel.cancel();
                return Err(EngineError::Message(
                    "critical tool persistence failed; run stopped fail-closed".into(),
                ));
            }

            // Safe point: the tool batch and its assistant/tool-result
            // transcript are fully committed before steering is leased. A
            // queue failure therefore cannot leave an open turn behind.
            self.apply_safe_point(
                &config.conversation_id,
                crate::session_coordinator::SafePoint::AfterTool,
                &mut typed_messages,
            )
            .await?;
            self.drain_inputs(
                crate::PendingInputKind::Steering,
                crate::DrainMode::All,
                crate::InputSafePoint::AfterToolBatch,
                &mut typed_messages,
                Some(turn_id.0.as_str()),
            )
            .await?;

            // Compact large tool outputs + repair dangling tool_call_ids before
            // the next provider turn (no isolated tool calls).
            // Safe point: ProviderBatchBoundary — between tool batch and next provider turn.
            typed_messages = self
                .maybe_compact_typed_history(
                    run_id,
                    &turn_id,
                    &config.model,
                    provider,
                    typed_messages,
                )
                .await?;
            self.apply_safe_point(
                &config.conversation_id,
                crate::session_coordinator::SafePoint::ProviderBatchBoundary,
                &mut typed_messages,
            )
            .await?;
        }
    }

    /// Execute prepared tool calls with capability-driven batching (max
    /// concurrency 4). Non-parallel tools and rejected calls stay serial;
    /// results keep original source order. The Core does not special-case tool
    /// names when deciding concurrency.
    async fn execute_prepared_tools(
        &self,
        run_id: &str,
        turn_id: &str,
        message_id: &str,
        tools: &dyn EngineToolRuntime,
        prepared: Vec<PreparedToolCall>,
    ) -> Result<Vec<ExecutedToolCall>, EngineError> {
        use crate::session_coordinator::PARALLEL_SAFE_MAX_CONCURRENCY;
        use futures_util::stream::{self, StreamExt};

        let mut out: Vec<ExecutedToolCall> = Vec::with_capacity(prepared.len());
        // A batch is parallel only when every non-rejected call is explicitly
        // advertised ParallelSafe. One sequential/exclusive call serializes
        // the whole batch, preserving deterministic side-effect order.
        let mut conflict_keys = std::collections::HashSet::new();
        let batch_parallel = prepared.iter().all(|call| {
            if call.rejected.is_some() {
                return true;
            }
            if !call.parallel_safe {
                return false;
            }
            call.conflict_key
                .as_ref()
                .map(|key| conflict_keys.insert(key.clone()))
                .unwrap_or(true)
        });
        let mut i = 0;
        while i < prepared.len() {
            if let Some(result) = prepared[i].rejected.clone() {
                let keeps_progress = is_long_running_tool_result(&result);
                let result_message_id = crate::MessageId::new().to_string();
                out.push(ExecutedToolCall {
                    id: prepared[i].id.clone(),
                    name: prepared[i].name.clone(),
                    args: prepared[i].args.clone(),
                    result: Some(result.clone()),
                    result_message_id: Some(result_message_id.clone()),
                });
                if let Err(error) = self.append_critical(
                    run_id,
                    RunEventKind::ToolCallCompleted {
                        id: prepared[i].id.clone(),
                        name: prepared[i].name.clone(),
                        output: result.output,
                        is_error: true,
                        duration_ms: 0,
                        result_message_id: Some(result_message_id),
                    },
                ) {
                    let uncertain = tools
                        .mark_tool_call_uncertain(
                            &prepared[i].id,
                            &prepared[i].name,
                            Some(turn_id),
                            &serde_json::from_str(&prepared[i].args)
                                .unwrap_or(Value::String(prepared[i].args.clone())),
                        )
                        .await;
                    self.cancel.cancel();
                    self.progress_sink
                        .mark_tool_call_settled(&prepared[i].id)
                        .await;
                    if let Err(uncertain_error) = uncertain {
                        return Err(EngineError::RecoveryBlocked(format!(
                            "completion fact AND uncertain ledger recording both failed: completion={error}; uncertain={uncertain_error}"
                        )));
                    }
                    return Err(error);
                }
                if !keeps_progress {
                    self.progress_sink
                        .mark_tool_call_settled(&prepared[i].id)
                        .await;
                }
                i += 1;
                continue;
            }

            // Gather a contiguous parallel_safe run (cap concurrency).
            if batch_parallel && prepared[i].parallel_safe {
                let mut batch = Vec::new();
                while i < prepared.len()
                    && prepared[i].parallel_safe
                    && prepared[i].rejected.is_none()
                    && batch.len() < PARALLEL_SAFE_MAX_CONCURRENCY
                {
                    batch.push(prepared[i].clone());
                    i += 1;
                }

                let cancel = self.cancel.clone();
                let results: Vec<(usize, ToolExecutionResult)> =
                    stream::iter(batch.iter().cloned().enumerate().map(|(idx, call)| {
                        let cancel = cancel.clone();
                        async move {
                            let result = tools
                                .execute_tool_with_progress_for_call(
                                    &call.id,
                                    Some(turn_id),
                                    Some(message_id),
                                    &call.name,
                                    call.input.clone(),
                                    &cancel,
                                    self.progress_sink.clone(),
                                )
                                .await;
                            (idx, result)
                        }
                    }))
                    .buffer_unordered(PARALLEL_SAFE_MAX_CONCURRENCY)
                    .collect()
                    .await;

                let mut by_idx: Vec<Option<ToolExecutionResult>> =
                    (0..batch.len()).map(|_| None).collect();
                for (idx, result) in results {
                    if idx < by_idx.len() {
                        by_idx[idx] = Some(result);
                    }
                }

                for (idx, call) in batch.into_iter().enumerate() {
                    let result = by_idx[idx].take().unwrap_or(ToolExecutionResult {
                        output: json!({"error": "missing parallel result"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                    let post_event = if result.is_error {
                        HookEvent::PostToolUseFailure
                    } else {
                        HookEvent::PostToolUse
                    };
                    self.hooks
                        .observe(HookRequest {
                            event: post_event,
                            run_id: run_id.to_string(),
                            tool_name: Some(call.name.clone()),
                            input: json!({ "input": call.input, "output": result.output }),
                        })
                        .await
                        .map_err(|refusal| match refusal {
                            crate::hooks::ObserveResult::Failed { reason } => {
                                EngineError::HookRefused(reason)
                            }
                            crate::hooks::ObserveResult::Observe => {
                                EngineError::HookRefused("post-hook observation failed".into())
                            }
                        })?;
                    let result_message_id = crate::MessageId::new().to_string();
                    if let Err(error) = self.append_critical(
                        run_id,
                        RunEventKind::ToolCallCompleted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            output: result.output.clone(),
                            is_error: result.is_error,
                            duration_ms: result.duration_ms,
                            result_message_id: Some(result_message_id.clone()),
                        },
                    ) {
                        let uncertain = tools
                            .mark_tool_call_uncertain(
                                &call.id,
                                &call.name,
                                Some(turn_id),
                                &call.input,
                            )
                            .await;
                        self.cancel.cancel();
                        self.progress_sink.mark_tool_call_settled(&call.id).await;
                        if let Err(uncertain_error) = uncertain {
                            return Err(EngineError::RecoveryBlocked(format!(
                                "completion fact AND uncertain ledger recording both failed: completion={error}; uncertain={uncertain_error}"
                            )));
                        }
                        return Err(error);
                    }
                    if !is_long_running_tool_result(&result) {
                        self.progress_sink.mark_tool_call_settled(&call.id).await;
                    }
                    out.push(ExecutedToolCall {
                        id: call.id,
                        name: call.name,
                        args: call.args,
                        result: Some(result),
                        result_message_id: Some(result_message_id),
                    });
                }
                continue;
            }

            // Serial path for write / process / network.
            let call = &prepared[i];
            let result = tools
                .execute_tool_with_progress_for_call(
                    &call.id,
                    Some(turn_id),
                    Some(message_id),
                    &call.name,
                    call.input.clone(),
                    &self.cancel,
                    self.progress_sink.clone(),
                )
                .await;
            let post_event = if result.is_error {
                HookEvent::PostToolUseFailure
            } else {
                HookEvent::PostToolUse
            };
            self.hooks
                .observe(HookRequest {
                    event: post_event,
                    run_id: run_id.to_string(),
                    tool_name: Some(call.name.clone()),
                    input: json!({ "input": call.input, "output": result.output }),
                })
                .await
                .map_err(|refusal| match refusal {
                    crate::hooks::ObserveResult::Failed { reason } => {
                        EngineError::HookRefused(reason)
                    }
                    crate::hooks::ObserveResult::Observe => {
                        EngineError::HookRefused("post-hook observation failed".into())
                    }
                })?;
            let result_message_id = crate::MessageId::new().to_string();
            if let Err(error) = self.append_critical(
                run_id,
                RunEventKind::ToolCallCompleted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    output: result.output.clone(),
                    is_error: result.is_error,
                    duration_ms: result.duration_ms,
                    result_message_id: Some(result_message_id.clone()),
                },
            ) {
                let uncertain = tools
                    .mark_tool_call_uncertain(&call.id, &call.name, Some(turn_id), &call.input)
                    .await;
                self.cancel.cancel();
                self.progress_sink.mark_tool_call_settled(&call.id).await;
                if let Err(uncertain_error) = uncertain {
                    return Err(EngineError::RecoveryBlocked(format!(
                        "completion fact AND uncertain ledger recording both failed: completion={error}; uncertain={uncertain_error}"
                    )));
                }
                return Err(error);
            }
            if !is_long_running_tool_result(&result) {
                self.progress_sink.mark_tool_call_settled(&call.id).await;
            }
            out.push(ExecutedToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
                result: Some(result),
                result_message_id: Some(result_message_id),
            });
            i += 1;
        }
        Ok(out)
    }

    /// Compact the typed Core transcript at the provider-neutral JSON boundary.
    ///
    /// Over budget the engine first asks the model for a structured summary of
    /// the old prefix (see [`crate::compaction::SUMMARY_SYSTEM_PROMPT`]) and
    /// keeps only `[summary] + recent tail`. Every failure path — provider
    /// error, timeout, cancellation, empty answer, budget exhausted — falls
    /// back to mechanical compaction. Compaction never fails a Run.
    async fn maybe_compact_typed_history(
        &self,
        run_id: &str,
        turn_id: &crate::TurnId,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<crate::AgentMessage>,
    ) -> Result<Vec<crate::AgentMessage>, EngineError> {
        let values = agent_messages_to_values(&messages);
        let compacted = self
            .maybe_compact_values(run_id, model, provider, values)
            .await?;
        if compacted != agent_messages_to_values(&messages) {
            let summary_message_id = compacted.iter().find_map(|message| {
                let content = message.get("content").and_then(Value::as_str)?;
                (message.get("role").and_then(Value::as_str) == Some("system")
                    && content.starts_with(crate::compaction::SUMMARY_MARKER))
                .then(|| message.get("message_id").and_then(Value::as_str))
                .flatten()
                .map(str::to_string)
            });
            self.append_critical(
                run_id,
                RunEventKind::ContextSnapshotCommitted {
                    snapshot_id: uuid::Uuid::new_v4().to_string(),
                    turn_id: Some(turn_id.to_string()),
                    source_revision: self.events.last_sequence(run_id),
                    input_message_ids: messages.iter().map(agent_message_id).collect(),
                    summary_message_id,
                    replaced_range: Some(format!("0..{}", messages.len())),
                    algorithm_version: "typed-compaction-v1".into(),
                    provider_context_window: self.provider_context_window,
                    artifact_reference: None,
                    snapshot_json: Value::Array(compacted.clone()),
                },
            )?;
        }
        Ok(values_to_agent_messages(&compacted))
    }

    async fn maybe_compact_values(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<Value>,
    ) -> Result<Vec<Value>, EngineError> {
        let history_limit = self.history_compact_chars.unwrap_or(HISTORY_COMPACT_CHARS);
        let tool_limit = self.tool_output_max_chars.unwrap_or(TOOL_OUTPUT_MAX_CHARS);
        // PERF-001: observe the transcript with a cheap structured char count
        // (a bounded `Value` tree walk — no string allocation) and append ONLY
        // this round's growth to the running budget. The pre-fix path
        // re-serialized the whole transcript to a JSON string every
        // provider/tool round AND re-accumulated that full size into the
        // estimate, so an uncompacted multi-turn run drifted arbitrarily large
        // while still paying O(n) serialization per round.
        let before_chars = values_chars(&messages);
        self.context_stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe_transcript(before_chars);
        if before_chars < history_limit {
            // Still repair dangling pairs cheaply.
            let (fixed, repaired) = repair_dangling_tool_calls(&messages);
            if repaired == 0 {
                return Ok(messages);
            }
            return Ok(fixed);
        }

        let pre_compact = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::PreCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "before_chars": before_chars,
                    "messages": messages.len(),
                    "model_compaction": self.model_compaction,
                }),
            })
            .await;
        if HookRegistry::aggregate_allow(&pre_compact).is_err() {
            // PreCompact is a pre-event: a Deny aborts compaction (the
            // transcript is returned untouched) — a deterministic, honest
            // outcome, unlike the post-event observation points.
            return Ok(messages);
        }

        let result = match self
            .try_model_summary(run_id, model, provider, &messages, tool_limit)
            .await
        {
            Some(summarized) => summarized,
            None => compact_tool_history(&messages, tool_limit),
        };
        let mode = if result.summarized_messages > 0 {
            "model"
        } else {
            "mechanical"
        };
        // Same cheap structured measure as the observe above, so the reset
        // baseline and the next observation use one consistent char metric.
        let after_chars = values_chars(&result.messages);
        // P1-05/PERF-001: a real compaction rewrote the transcript — bump the
        // revision and re-seed the running counters from the kept size so
        // stale bytes do not accumulate (P1-06 invariant).
        {
            let mut stats = self.context_stats.lock().unwrap_or_else(|e| e.into_inner());
            stats.note_compaction();
            stats.reset(after_chars);
        }

        self.events.append(
            run_id,
            RunEventKind::ContextCompressed {
                before_tokens: (before_chars as u64 / 4).max(1),
                after_tokens: (after_chars as u64 / 4).max(1),
                summary: result.summary.clone(),
            },
        );

        // PostCompact is an observation point: the compaction already applied
        // and a hook cannot rewind it. A hook that tries to alter the outcome
        // (Deny/Modify/Inject) is refused loudly rather than silently ignored.
        self.hooks
            .observe(HookRequest {
                event: HookEvent::PostCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "mode": mode,
                    "before_chars": before_chars,
                    "after_chars": after_chars,
                    "dropped_tool_outputs": result.dropped_tool_outputs,
                    "repaired_dangling": result.repaired_dangling,
                    "summarized_messages": result.summarized_messages,
                    "summary": result.summary,
                }),
            })
            .await
            .map_err(|refusal| match refusal {
                crate::hooks::ObserveResult::Failed { reason } => EngineError::HookRefused(reason),
                crate::hooks::ObserveResult::Observe => {
                    EngineError::HookRefused("post-hook observation failed".into())
                }
            })?;

        Ok(result.messages)
    }

    /// Model-backed compaction: `Some` only when a usable summary came back.
    ///
    /// Returning `None` is the documented degradation path and the caller
    /// answers it with mechanical compaction.
    async fn try_model_summary(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        values: &[Value],
        tool_limit: usize,
    ) -> Option<CompactResult> {
        if !self.model_compaction || self.cancel.is_cancelled() {
            return None;
        }
        // Cost boundary: bounded attempts per engine, and a failure streak
        // permanently drops the run back to mechanical compaction.
        if self.summary_attempts.load(AtomicOrdering::SeqCst) >= SUMMARY_MAX_ATTEMPTS
            || self.summary_failures.load(AtomicOrdering::SeqCst) >= SUMMARY_MAX_FAILURES
        {
            return None;
        }
        let split = choose_summary_split(values, SUMMARY_KEEP_TAIL_MESSAGES);
        if split < SUMMARY_MIN_PREFIX_MESSAGES {
            // Too little history to be worth a round trip.
            return None;
        }

        self.summary_attempts.fetch_add(1, AtomicOrdering::SeqCst);
        let transcript = render_transcript_for_summary(
            &values[..split],
            SUMMARY_TRANSCRIPT_MAX_CHARS,
            SUMMARY_MESSAGE_MAX_CHARS,
        );
        let request = vec![crate::AgentMessage::User(crate::UserMessage {
            message_id: crate::MessageId::new(),
            content: vec![crate::ContentBlock::Text { text: transcript }],
        })];

        match self
            .stream_summary_text(run_id, model, provider, request)
            .await
        {
            Ok(summary) if !summary.trim().is_empty() => {
                Some(apply_model_summary(values, split, &summary, tool_limit))
            }
            _ => {
                self.summary_failures.fetch_add(1, AtomicOrdering::SeqCst);
                None
            }
        }
    }

    /// One isolated provider round trip that yields plain summary text.
    ///
    /// Isolated in three ways: no tools (so it cannot start a tool loop), a
    /// throwaway message vector (so it never touches the run history), and no
    /// event emission (so the summary does not surface as assistant output).
    /// Bounded by the run cancel token and a wall-clock timeout.
    async fn stream_summary_text(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<crate::AgentMessage>,
    ) -> Result<String, String> {
        let cancel = self.cancel.clone();
        let collect = async {
            let mut stream = provider
                .stream_turn(
                    ProviderTurnRequest {
                        context: EngineProviderContext {
                            run_id: run_id.to_string(),
                            attempt: 0,
                        },
                        model: model.to_string(),
                        system_prompt: Some(SUMMARY_SYSTEM_PROMPT.to_string()),
                        messages,
                        tools: Vec::new(),
                    },
                    cancel.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut text = String::new();
            while let Some(event) = stream.next().await {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                match event {
                    EngineProviderEvent::TextDelta(delta) => text.push_str(&delta),
                    EngineProviderEvent::Error { message, .. } => return Err(message),
                    _ => {}
                }
            }
            Ok(text)
        };

        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err("cancelled".to_string()),
            outcome = tokio::time::timeout(
                std::time::Duration::from_millis(SUMMARY_TIMEOUT_MS),
                collect,
            ) => outcome.unwrap_or_else(|_| Err("summary request timed out".to_string())),
        }
    }
}

/// Apply SessionStart/UserPromptSubmit hook decisions.
///
/// Returns the messages a hook asked to inject. The engine places them in the
/// typed transcript (see [`AgentEngine::run_inner`]) so they reach the
/// `ProviderTurnRequest`; the legacy `config.messages` list is not a channel
/// for hook context and nothing injects into it any more.
fn apply_prompt_hook_responses(
    config: &mut EngineRunConfig,
    responses: Vec<crate::hooks::HookResponse>,
) -> Result<Vec<String>, EngineError> {
    let mut injected = Vec::new();
    for response in responses {
        match response.decision {
            HookDecision::Deny { reason } => {
                return Err(EngineError::Message(format!("hook denied: {reason}")));
            }
            HookDecision::Modify { payload } => {
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    config.user_content = content.to_string();
                }
            }
            HookDecision::Inject { messages } => {
                injected.extend(messages);
            }
            HookDecision::Allow => {}
        }
    }
    Ok(injected)
}

/// Leading characters of a tool's arguments kept verbatim in its doom-loop key.
///
/// Long enough to stay readable in a [`crate::doom_loop::DoomLoopReason`]
/// pattern, short enough that the key does not carry a whole file body.
const TOOL_FINGERPRINT_PREFIX_CHARS: usize = 80;

/// Identity of one tool invocation for doom-loop purposes.
///
/// A bare 80-character prefix is not an identity: two `edit` calls on the same
/// file whose argument JSON happens to agree for 80 characters and diverges at
/// the 400th would look identical, and three of them would abort a run that was
/// making progress. The prefix is kept for readability and a hash of the *full*
/// arguments is appended so distinct calls stay distinct.
fn tool_args_fingerprint(args: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut chars = args.chars();
    let prefix: String = chars.by_ref().take(TOOL_FINGERPRINT_PREFIX_CHARS).collect();
    if chars.next().is_none() {
        return prefix;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    args.hash(&mut hasher);
    format!("{prefix}#{:016x}", hasher.finish())
}

/// Ceiling on a provider-supplied `retry_after_ms`.
///
/// A provider (or a routing layer, see `routing::route_unavailable` with its
/// 60s hint) can name any delay it likes, and an absurd one would pin a run
/// open for as long as it wants. One minute is the longest wait that is still
/// plausibly worth doing inside a single generation attempt; past that the run
/// is better off failing so the caller can decide. The wait is cancellable
/// throughout, so the ceiling bounds patience, not responsiveness.
const MAX_PROVIDER_BACKOFF_MS: u64 = 60_000;

/// How long to wait before retrying a failed generation attempt.
///
/// The provider's own hint wins when it asks for *more* than the local
/// schedule — that is the whole point of `Retry-After`, and ignoring it is how
/// a 429 turns into three instant retries and a longer ban. It never shortens
/// the wait, and it never exceeds [`MAX_PROVIDER_BACKOFF_MS`].
fn provider_backoff_ms(attempt: u32, retry_after_ms: Option<u64>) -> u64 {
    let local = match attempt {
        1 => 500,
        2 => 1_000,
        _ => 2_000,
    };
    retry_after_ms
        .unwrap_or(0)
        .min(MAX_PROVIDER_BACKOFF_MS)
        .max(local)
}

fn stop_reason_label(reason: Option<&ProviderStopReason>) -> String {
    match reason {
        Some(ProviderStopReason::Stop) => "stop".into(),
        Some(ProviderStopReason::ToolUse) => "tool_use".into(),
        Some(ProviderStopReason::Length) => "length".into(),
        Some(ProviderStopReason::Cancelled) => "cancelled".into(),
        Some(ProviderStopReason::Error) => "error".into(),
        Some(ProviderStopReason::Unknown(raw)) => format!("unknown:{raw}"),
        None => "unknown".into(),
    }
}

fn core_stop_reason(reason: Option<&ProviderStopReason>) -> crate::StopReason {
    match reason.cloned().unwrap_or(ProviderStopReason::ToolUse) {
        ProviderStopReason::Stop => crate::StopReason::Stop,
        ProviderStopReason::ToolUse => crate::StopReason::ToolUse,
        ProviderStopReason::Length => crate::StopReason::Length,
        ProviderStopReason::Cancelled => crate::StopReason::Cancelled,
        ProviderStopReason::Error => crate::StopReason::Error,
        ProviderStopReason::Unknown(raw) => crate::StopReason::Provider(raw),
    }
}

#[allow(dead_code)]
fn engine_messages_to_values(messages: &[EngineMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), json!(m.role));
            obj.insert("content".into(), json!(m.content));
            if let Some(id) = &m.tool_call_id {
                obj.insert("tool_call_id".into(), json!(id));
            }
            if let Some(name) = &m.tool_name {
                obj.insert("name".into(), json!(name));
            }
            if let Some(calls) = &m.tool_calls {
                let arr: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": c.arguments,
                            }
                        })
                    })
                    .collect();
                obj.insert("tool_calls".into(), Value::Array(arr));
            }
            // Compaction round-trips history through JSON. Images have to make
            // the trip or they would vanish at the first compaction, which is
            // exactly the silent-drop failure this field was added to stop.
            if !m.images.is_empty() {
                let arr: Vec<Value> = m
                    .images
                    .iter()
                    .map(|img| {
                        let mut obj = serde_json::Map::new();
                        obj.insert("url".into(), json!(img.url));
                        if let Some(media_type) = &img.media_type {
                            obj.insert("media_type".into(), json!(media_type));
                        }
                        if let Some(detail) = &img.detail {
                            obj.insert("detail".into(), json!(detail));
                        }
                        Value::Object(obj)
                    })
                    .collect();
                obj.insert("images".into(), Value::Array(arr));
            }
            Value::Object(obj)
        })
        .collect()
}

/// Cheap estimate of a transcript's serialized length WITHOUT allocating a
/// string (PERF-001). Walks the `Value` tree summing string lengths and key
/// lengths; structural overhead is folded in per node but no bytes are
/// formatted or escaped. This is what the engine observes every
/// provider/tool round instead of `messages.iter().map(|m|
/// m.to_string().len()).sum()` — same order of growth, no per-round heap
/// churn from JSON serialization.
fn values_chars(messages: &[Value]) -> usize {
    messages.iter().map(value_chars).sum()
}

fn value_chars(value: &Value) -> usize {
    match value {
        Value::Null => 4,
        Value::Bool(boolean) => {
            if *boolean {
                4
            } else {
                5
            }
        }
        Value::Number(number) => number.to_string().len(),
        Value::String(text) => text.len(),
        Value::Array(items) => items.iter().map(value_chars).sum(),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| key.len() + value_chars(value))
            .sum(),
    }
}

fn agent_messages_to_values(messages: &[crate::AgentMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| match message {
            crate::AgentMessage::User(message) => json!({
                "role": "user",
                "message_id": message.message_id,
                "content": message
                    .content
                    .iter()
                    .map(content_block_to_value)
                    .collect::<Vec<_>>(),
            }),
            crate::AgentMessage::Assistant(message) => json!({
                "role": "assistant",
                "message_id": message.message_id,
                "content": message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::ContentBlock::Text { text }
                        | crate::ContentBlock::Thinking { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
                "blocks": message
                    .content
                    .iter()
                    .map(content_block_to_value)
                    .collect::<Vec<_>>(),
                "tool_calls": message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::ContentBlock::ToolCall(call) => Some(json!({
                            "id": call.tool_call_id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments_json,
                            }
                        })),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                "stop_reason": message.stop_reason.as_ref().map(ToString::to_string),
            }),
            crate::AgentMessage::ToolResult(message) => json!({
                "role": "tool",
                "message_id": message.message_id,
                "tool_call_id": message.tool_call_id,
                "name": message.tool_name,
                "content": tool_result_content(&message.content),
                "tool_result_blocks": message
                    .content
                    .iter()
                    .map(tool_result_block_to_value)
                    .collect::<Vec<_>>(),
                "is_error": message.is_error,
                "error_code": message.code,
            }),
            crate::AgentMessage::System(message) => json!({
                "role": "system",
                "message_id": message.message_id,
                "content": message.text,
            }),
            crate::AgentMessage::Custom(message) => json!({
                "role": "custom",
                "message_id": message.message_id,
                "kind": message.kind,
                "payload": message.payload,
            }),
        })
        .collect()
}

fn agent_message_id(message: &crate::AgentMessage) -> String {
    match message {
        crate::AgentMessage::User(value) => value.message_id.to_string(),
        crate::AgentMessage::Assistant(value) => value.message_id.to_string(),
        crate::AgentMessage::ToolResult(value) => value.message_id.to_string(),
        crate::AgentMessage::System(value) => value.message_id.to_string(),
        crate::AgentMessage::Custom(value) => value.message_id.to_string(),
    }
}

fn content_block_to_value(block: &crate::ContentBlock) -> Value {
    match block {
        crate::ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        crate::ContentBlock::Thinking { text, signature } => {
            json!({ "type": "thinking", "text": text, "signature": signature })
        }
        crate::ContentBlock::Image { source } => json!({ "type": "image", "source": source }),
        crate::ContentBlock::ToolCall(call) => json!({
            "type": "tool_call",
            "tool_call_id": call.tool_call_id,
            "name": call.name,
            "arguments": call.arguments_json,
        }),
    }
}

fn tool_result_block_to_value(block: &crate::ToolResultBlock) -> Value {
    match block {
        crate::ToolResultBlock::Text { text } => json!({ "type": "text", "text": text }),
        crate::ToolResultBlock::Json { value } => json!({ "type": "json", "value": value }),
        crate::ToolResultBlock::Artifact {
            artifact_id,
            preview,
        } => json!({ "type": "artifact", "artifact_id": artifact_id, "preview": preview }),
    }
}

#[allow(clippy::unnecessary_filter_map)] // every arm returns Some; rewrite is large and risky
fn values_to_agent_messages(values: &[Value]) -> Vec<crate::AgentMessage> {
    values
        .iter()
        .filter_map(|value| {
            let role = value.get("role").and_then(Value::as_str).unwrap_or("user");
            let message_id = value
                .get("message_id")
                .and_then(Value::as_str)
                .map(crate::MessageId::from)
                .unwrap_or_else(crate::MessageId::new);
            match role {
                "tool" => {
                    let blocks = value
                        .get("tool_result_blocks")
                        .and_then(Value::as_array)
                        .map(|blocks| {
                            blocks
                                .iter()
                                .filter_map(value_to_tool_result_block)
                                .collect::<Vec<_>>()
                        })
                        .filter(|blocks| !blocks.is_empty())
                        .unwrap_or_else(|| {
                            vec![crate::ToolResultBlock::Text {
                                text: value
                                    .get("content")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                            }]
                        });
                    Some(crate::AgentMessage::ToolResult(crate::ToolResultMessage {
                        message_id,
                        tool_call_id: crate::ToolCallId::from(
                            value
                                .get("tool_call_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        ),
                        tool_name: value
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        content: blocks,
                        is_error: value
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        code: value
                            .get("error_code")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    }))
                }
                "assistant" => Some(crate::AgentMessage::Assistant(crate::AssistantMessage {
                    message_id,
                    content: value
                        .get("blocks")
                        .and_then(Value::as_array)
                        .map(|blocks| blocks.iter().filter_map(value_to_content_block).collect())
                        .unwrap_or_else(|| {
                            value
                                .get("content")
                                .and_then(Value::as_str)
                                .filter(|text| !text.is_empty())
                                .map(|text| vec![crate::ContentBlock::Text { text: text.into() }])
                                .unwrap_or_default()
                        }),
                    stop_reason: value
                        .get("stop_reason")
                        .and_then(Value::as_str)
                        .map(stop_reason_from_label),
                })),
                "system" => Some(crate::AgentMessage::System(crate::SystemMessage {
                    message_id,
                    text: value
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })),
                "user" => Some(crate::AgentMessage::User(crate::UserMessage {
                    message_id,
                    content: value
                        .get("content")
                        .and_then(Value::as_array)
                        .map(|blocks| blocks.iter().filter_map(value_to_content_block).collect())
                        .unwrap_or_else(|| {
                            value
                                .get("content")
                                .and_then(Value::as_str)
                                .map(|text| vec![crate::ContentBlock::Text { text: text.into() }])
                                .unwrap_or_default()
                        }),
                })),
                "custom" => Some(crate::AgentMessage::Custom(crate::CustomMessage {
                    message_id,
                    kind: value
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("custom")
                        .to_string(),
                    payload: value.get("payload").cloned().unwrap_or(Value::Null),
                })),
                kind => Some(crate::AgentMessage::Custom(crate::CustomMessage {
                    message_id,
                    kind: kind.to_string(),
                    payload: value.get("content").cloned().unwrap_or(Value::Null),
                })),
            }
        })
        .collect()
}

/// Replay a persisted active-context snapshot without exposing the provider
/// wire representation to the daemon.
pub fn agent_messages_from_json(value: &Value) -> Vec<crate::AgentMessage> {
    value
        .as_array()
        .map_or_else(Vec::new, |items| values_to_agent_messages(items))
}

/// Strict active-context snapshot decoder.  The compatibility decoder above
/// intentionally tolerates old provider-shaped values; durable recovery must
/// not silently invent message IDs or drop malformed blocks.
pub fn try_agent_messages_from_json(value: &Value) -> Result<Vec<crate::AgentMessage>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| "active context snapshot must be an array".to_string())?;
    for (index, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| format!("snapshot message {index} is not an object"))?;
        let message_id = object
            .get("message_id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| format!("snapshot message {index} is missing message_id"))?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot message {message_id} is missing role"))?;
        match role {
            "assistant" => {
                if let Some(blocks) = object.get("blocks") {
                    validate_snapshot_content_blocks(message_id, blocks)?;
                } else if object.get("content").and_then(Value::as_str).is_none() {
                    return Err(format!("snapshot assistant {message_id} has no content"));
                }
            }
            "user" => {
                if let Some(blocks) = object.get("content") {
                    if !blocks.is_string() {
                        validate_snapshot_content_blocks(message_id, blocks)?;
                    }
                } else {
                    return Err(format!("snapshot user {message_id} has no content"));
                }
            }
            "system" => {
                if object.get("content").and_then(Value::as_str).is_none() {
                    return Err(format!("snapshot system {message_id} has no content"));
                }
            }
            "tool" => {
                let call_id = object
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| format!("snapshot tool {message_id} is missing tool_call_id"))?;
                if object
                    .get("name")
                    .and_then(Value::as_str)
                    .is_none_or(|name| name.trim().is_empty())
                {
                    return Err(format!("snapshot tool {call_id} is missing name"));
                }
                if let Some(blocks) = object.get("tool_result_blocks") {
                    validate_snapshot_tool_result_blocks(call_id, blocks)?;
                } else if object.get("content").and_then(Value::as_str).is_none() {
                    return Err(format!("snapshot tool {call_id} has no content"));
                }
            }
            "custom" => {
                if object
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind.trim().is_empty())
                {
                    return Err(format!("snapshot custom {message_id} is missing kind"));
                }
                if object.get("payload").is_none() {
                    return Err(format!("snapshot custom {message_id} has no payload"));
                }
            }
            other => {
                return Err(format!(
                    "snapshot message {message_id} has unknown role {other}"
                ))
            }
        }
    }
    Ok(values_to_agent_messages(items))
}

fn validate_snapshot_content_blocks(message_id: &str, value: &Value) -> Result<(), String> {
    let blocks = value
        .as_array()
        .ok_or_else(|| format!("snapshot message {message_id} blocks are not an array"))?;
    for (index, block) in blocks.iter().enumerate() {
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot message {message_id} block {index} has no type"))?;
        match kind {
            "text" | "thinking" => {
                if block.get("text").and_then(Value::as_str).is_none() {
                    return Err(format!(
                        "snapshot message {message_id} block {index} has no text"
                    ));
                }
            }
            "image" => {
                let source = block
                    .get("source")
                    .ok_or_else(|| format!("snapshot message {message_id} image has no source"))?;
                serde_json::from_value::<crate::ImageSource>(source.clone())
                    .map_err(|e| format!("invalid snapshot image source: {e}"))?;
            }
            "tool_call" => {
                for field in ["tool_call_id", "name", "arguments"] {
                    if block
                        .get(field)
                        .and_then(Value::as_str)
                        .is_none_or(|value| value.trim().is_empty())
                    {
                        return Err(format!(
                            "snapshot message {message_id} tool call missing {field}"
                        ));
                    }
                }
                let arguments =
                    block
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!("snapshot message {message_id} tool call arguments are missing")
                        })?;
                serde_json::from_str::<Value>(arguments).map_err(|error| {
                    format!(
                        "snapshot message {message_id} tool call arguments are invalid JSON: {error}"
                    )
                })?;
            }
            other => {
                return Err(format!(
                    "snapshot message {message_id} has unknown block {other}"
                ))
            }
        }
    }
    Ok(())
}

fn validate_snapshot_tool_result_blocks(call_id: &str, value: &Value) -> Result<(), String> {
    let blocks = value
        .as_array()
        .ok_or_else(|| format!("snapshot tool {call_id} result blocks are not an array"))?;
    for (index, block) in blocks.iter().enumerate() {
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot tool {call_id} result {index} has no type"))?;
        match kind {
            "text" => {
                if block.get("text").and_then(Value::as_str).is_none() {
                    return Err(format!(
                        "snapshot tool {call_id} result {index} has no text"
                    ));
                }
            }
            "json" => {
                if block.get("value").is_none() {
                    return Err(format!(
                        "snapshot tool {call_id} result {index} has no value"
                    ));
                }
            }
            "artifact" => {
                if block
                    .get("artifact_id")
                    .and_then(Value::as_str)
                    .is_none_or(|id| id.trim().is_empty())
                {
                    return Err(format!(
                        "snapshot tool {call_id} result {index} has no artifact_id"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "snapshot tool {call_id} has unknown result block {other}"
                ))
            }
        }
    }
    Ok(())
}

fn value_to_content_block(value: &Value) -> Option<crate::ContentBlock> {
    match value.get("type").and_then(Value::as_str)? {
        "text" => Some(crate::ContentBlock::Text {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        }),
        "thinking" => Some(crate::ContentBlock::Thinking {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            signature: value
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "image" => serde_json::from_value(value.get("source")?.clone())
            .ok()
            .map(|source| crate::ContentBlock::Image { source }),
        "tool_call" => Some(crate::ContentBlock::ToolCall(crate::ToolCall {
            tool_call_id: crate::ToolCallId::from(
                value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            arguments_json: value
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        })),
        _ => None,
    }
}

fn value_to_tool_result_block(value: &Value) -> Option<crate::ToolResultBlock> {
    match value.get("type").and_then(Value::as_str)? {
        "text" => Some(crate::ToolResultBlock::Text {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        }),
        "json" => Some(crate::ToolResultBlock::Json {
            value: value.get("value").cloned().unwrap_or(Value::Null),
        }),
        "artifact" => Some(crate::ToolResultBlock::Artifact {
            artifact_id: value
                .get("artifact_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            preview: value
                .get("preview")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        _ => None,
    }
}

fn stop_reason_from_label(label: &str) -> crate::StopReason {
    match label {
        "stop" => crate::StopReason::Stop,
        "tool_use" => crate::StopReason::ToolUse,
        "length" => crate::StopReason::Length,
        "cancelled" => crate::StopReason::Cancelled,
        "error" => crate::StopReason::Error,
        value => crate::StopReason::Provider(value.to_string()),
    }
}

pub fn engine_messages_to_agent_messages(messages: &[EngineMessage]) -> Vec<crate::AgentMessage> {
    messages
        .iter()
        .map(|message| {
            let message_id = crate::MessageId::new();
            let content = {
                let mut blocks = Vec::new();
                if !message.content.is_empty() {
                    blocks.push(crate::ContentBlock::Text {
                        text: message.content.clone(),
                    });
                }
                blocks.extend(message.images.iter().cloned().map(|image| {
                    crate::ContentBlock::Image {
                        source: crate::ImageSource {
                            url: image.url,
                            media_type: image.media_type,
                            detail: image.detail,
                        },
                    }
                }));
                blocks
            };
            match message.role.as_str() {
                "assistant" => {
                    let mut blocks = content;
                    if let Some(calls) = &message.tool_calls {
                        blocks.extend(calls.iter().map(|call| {
                            crate::ContentBlock::ToolCall(crate::ToolCall {
                                tool_call_id: crate::ToolCallId::from(call.id.clone()),
                                name: call.name.clone(),
                                arguments_json: call.arguments.clone(),
                            })
                        }));
                    }
                    crate::AgentMessage::Assistant(crate::AssistantMessage {
                        message_id,
                        content: blocks,
                        stop_reason: None,
                    })
                }
                "tool" => crate::AgentMessage::ToolResult(crate::ToolResultMessage {
                    message_id,
                    tool_call_id: crate::ToolCallId::from(
                        message.tool_call_id.clone().unwrap_or_default(),
                    ),
                    tool_name: message.tool_name.clone().unwrap_or_default(),
                    content: vec![crate::ToolResultBlock::Json {
                        value: serde_json::from_str(&message.content)
                            .unwrap_or_else(|_| json!(message.content)),
                    }],
                    is_error: false,
                    code: None,
                }),
                "system" => crate::AgentMessage::System(crate::SystemMessage {
                    message_id,
                    text: message.content.clone(),
                }),
                "user" => crate::AgentMessage::User(crate::UserMessage {
                    message_id,
                    content,
                }),
                kind => crate::AgentMessage::Custom(crate::CustomMessage {
                    message_id,
                    kind: kind.to_string(),
                    payload: json!({
                        "role": message.role,
                        "content": message.content,
                    }),
                }),
            }
        })
        .collect()
}

pub fn agent_messages_to_engine_messages(messages: &[crate::AgentMessage]) -> Vec<EngineMessage> {
    messages
        .iter()
        .map(|message| match message {
            crate::AgentMessage::User(message) => {
                engine_message_from_blocks("user", &message.content, None, None)
            }
            crate::AgentMessage::Assistant(message) => {
                let (content, images, tool_calls) = blocks_to_engine_parts(&message.content);
                EngineMessage {
                    role: "assistant".into(),
                    content,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
                    images,
                }
            }
            crate::AgentMessage::ToolResult(message) => EngineMessage {
                role: "tool".into(),
                content: tool_result_content(&message.content),
                tool_call_id: Some(message.tool_call_id.to_string()),
                tool_name: Some(message.tool_name.clone()),
                tool_calls: None,
                images: Vec::new(),
            },
            crate::AgentMessage::System(message) => {
                EngineMessage::text("system", message.text.clone())
            }
            crate::AgentMessage::Custom(message) => {
                EngineMessage::text(message.kind.clone(), message.payload.to_string())
            }
        })
        .collect()
}

fn engine_message_from_blocks(
    role: &str,
    blocks: &[crate::ContentBlock],
    tool_call_id: Option<String>,
    tool_name: Option<String>,
) -> EngineMessage {
    let (content, images, tool_calls) = blocks_to_engine_parts(blocks);
    EngineMessage {
        role: role.into(),
        content,
        tool_call_id,
        tool_name,
        tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
        images,
    }
}

fn blocks_to_engine_parts(
    blocks: &[crate::ContentBlock],
) -> (String, Vec<EngineImage>, Vec<EngineToolCall>) {
    let mut text = String::new();
    let mut images = Vec::new();
    let mut calls = Vec::new();
    for block in blocks {
        match block {
            crate::ContentBlock::Text { text: value }
            | crate::ContentBlock::Thinking { text: value, .. } => text.push_str(value),
            crate::ContentBlock::Image { source } => images.push(EngineImage {
                url: source.url.clone(),
                media_type: source.media_type.clone(),
                detail: source.detail.clone(),
            }),
            crate::ContentBlock::ToolCall(call) => calls.push(EngineToolCall {
                id: call.tool_call_id.to_string(),
                name: call.name.clone(),
                arguments: call.arguments_json.clone(),
            }),
        }
    }
    (text, images, calls)
}

fn tool_result_content(blocks: &[crate::ToolResultBlock]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            crate::ToolResultBlock::Text { text } => text.clone(),
            crate::ToolResultBlock::Json { value } => value.to_string(),
            crate::ToolResultBlock::Artifact {
                artifact_id,
                preview,
            } => preview.clone().unwrap_or_else(|| artifact_id.clone()),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(dead_code)]
fn values_to_engine_messages(values: &[Value]) -> Vec<EngineMessage> {
    values
        .iter()
        .map(|v| {
            let role = v
                .get("role")
                .and_then(|x| x.as_str())
                .unwrap_or("user")
                .to_string();
            let content = v
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let tool_call_id = v
                .get("tool_call_id")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let tool_name = v.get("name").and_then(|x| x.as_str()).map(str::to_string);
            let tool_calls = v.get("tool_calls").and_then(|x| x.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let id = c.get("id")?.as_str()?.to_string();
                        let name = c
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = c
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        Some(EngineToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            });
            let images = v
                .get("images")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|img| {
                            Some(EngineImage {
                                url: img.get("url")?.as_str()?.to_string(),
                                media_type: img
                                    .get("media_type")
                                    .and_then(|x| x.as_str())
                                    .map(str::to_string),
                                detail: img
                                    .get("detail")
                                    .and_then(|x| x.as_str())
                                    .map(str::to_string),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            EngineMessage {
                role,
                content,
                tool_call_id,
                tool_name,
                tool_calls,
                images,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
