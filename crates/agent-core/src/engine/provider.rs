//! Provider stream contract for the engine loop.
//!
//! Extracted from `engine_core.rs`. Owns the provider-side types
//! ([`EngineProvider`], [`EngineMessage`], [`EngineProviderEvent`],
//! [`ProviderStopReason`], ...) and the run configuration.
//!
//! The [`EngineProvider::stream_turn`] default implementation converts agent
//! messages into engine messages via [`super::conversion`], which keeps
//! lightweight providers source-compatible.

use futures_util::Stream;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

use super::conversion::agent_messages_to_engine_messages;
use super::error::EngineError;
use super::tool_runtime::ToolSchema;

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
