use crate::controls::RequestControls;
use crate::provider_identity::{ModelCapabilities, ProviderType};
use serde::{Deserialize, Serialize};

/// Provider capabilities declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Provider type.
    pub provider_type: ProviderType,
    /// Supported features.
    pub features: Vec<String>,
    /// Maximum context window in tokens.
    pub max_context_window: u64,
    /// Whether the provider supports streaming.
    pub streaming: bool,
    /// Whether the provider supports tool calls.
    pub tool_calls: bool,
    /// Whether the provider supports structured output.
    pub structured_output: bool,
    /// Whether the provider supports image input.
    pub image_input: bool,
    /// Whether the provider supports file input.
    pub file_input: bool,
    /// Whether the provider supports reasoning/thinking.
    pub reasoning: bool,
    /// Whether the provider supports system prompts.
    pub system_prompt: bool,
    /// Whether the provider supports function calling.
    pub function_calling: bool,
}

/// A single chat message in the provider format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMessage {
    pub role: String,
    pub content: Vec<ProviderContentBlock>,
}

/// A content block in provider format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderContentBlock {
    Text {
        text: String,
    },
    Image {
        image_url: ImageSource,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
        /// Tool name when known (required by Gemini functionResponse; optional for OpenAI).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// One tool call from engine/history (arguments may be a JSON string or object text).
#[derive(Debug, Clone)]
pub struct HistoryToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Engine/history message parts used to build a structured [`ProviderMessage`].
///
/// Callers (daemon `RealProvider`, Tauri bridge) map their engine types into this
/// shape so `tool_calls` / `tool_call_id` are never flattened to plain text.
#[derive(Debug, Clone, Default)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_calls: Option<Vec<HistoryToolCall>>,
    /// Images attached to this message.
    ///
    /// Always emitted as [`ProviderContentBlock::Image`] blocks, whatever the
    /// branch — dropping them here would be invisible to every adapter below.
    pub images: Vec<ImageSource>,
}

/// Convert a history/engine message into provider wire blocks.
///
/// - Assistant with `tool_calls` → optional text + images + one `ToolCall` block per call
/// - Tool role (or `tool_call_id` set without assistant tool_calls) → `ToolResult` + images
/// - Otherwise → text + images
///
/// Images are appended in every branch. Whether a given wire format can carry
/// them is the adapter's call, and an adapter that cannot must say so in the
/// message body ([`ImageSource::degraded_note`]) rather than drop them.
/// History-message → provider-message conversion moved to
/// `capabilities_history` (W9); re-exported for compatibility.
pub use crate::capabilities_history::{history_message_to_provider, history_messages_to_provider};

/// Image source for provider requests.
///
/// `url` is either a `data:` URI carrying inline base64 bytes, or a reference
/// the provider has to resolve itself (an `http(s)` URL, a `gs://` object, a
/// Google File API URI). `media_type` only has to be filled in when the URL
/// cannot state it — a `data:` URI already carries its own MIME type and wins
/// over this field.
///
/// Every adapter is required to encode an image or say out loud that it could
/// not; see [`ImageSource::degraded_note`]. Silently dropping an image is a bug.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageSource {
    pub url: String,
    /// Provider-specific fidelity hint (`"low"` / `"high"` / `"auto"`).
    /// Only OpenAI reads it today; other adapters ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// MIME type when the URL does not carry one. Required by Anthropic
    /// base64 sources and by every Gemini image part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

/// How an [`ImageSource`] can be handed to a provider.
///
/// This is the single place that knows how to read a `data:` URI, so no adapter
/// has to re-parse one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePayload<'a> {
    /// Inline base64 bytes taken from a `data:...;base64,` URI.
    ///
    /// `media_type` is `None` when neither the URI nor [`ImageSource::media_type`]
    /// stated one — adapters that require a MIME type must degrade loudly
    /// instead of guessing.
    Base64 {
        media_type: Option<&'a str>,
        data: &'a str,
    },
    /// A reference the provider resolves itself. The scheme is left intact so
    /// each adapter can decide what it accepts.
    Remote {
        url: &'a str,
        media_type: Option<&'a str>,
    },
}

impl ImageSource {
    /// Image referenced by URL or `data:` URI, with no extra hints.
    pub fn new(url: impl Into<String>) -> Self {
        ImageSource {
            url: url.into(),
            detail: None,
            media_type: None,
        }
    }

    /// Attach an explicit MIME type (used when `url` cannot state one).
    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Classify the reference for adapter encoding.
    pub fn payload(&self) -> ImagePayload<'_> {
        let declared = self.media_type.as_deref().filter(|s| !s.is_empty());
        if let Some(rest) = self.url.strip_prefix("data:") {
            if let Some((meta, data)) = rest.split_once(',') {
                // `data:[<mediatype>][;base64],<data>` — anything that is not
                // marked base64 is not bytes we can forward.
                if let Some(meta) = meta.strip_suffix(";base64") {
                    let inline = meta.split(';').next().filter(|s| !s.is_empty());
                    return ImagePayload::Base64 {
                        media_type: inline.or(declared),
                        data,
                    };
                }
            }
        }
        ImagePayload::Remote {
            url: self.url.as_str(),
            media_type: declared,
        }
    }

    /// Sentence an adapter emits in place of an image it cannot encode.
    ///
    /// The note lands in the message the model reads, so a dropped image is
    /// visible to both the model and the user instead of vanishing.
    pub fn degraded_note(&self, reason: &str) -> String {
        format!(
            "[image not sent to the model: {reason}; source={}]",
            self.short_ref()
        )
    }

    /// Short, log-safe rendering of the reference (never dumps base64 bytes).
    fn short_ref(&self) -> String {
        const MAX: usize = 120;
        if let Some(rest) = self.url.strip_prefix("data:") {
            if let Some((meta, data)) = rest.split_once(',') {
                return format!("data:{meta} ({} chars)", data.len());
            }
        }
        if self.url.chars().count() > MAX {
            let head: String = self.url.chars().take(MAX).collect();
            return format!("{head}…");
        }
        self.url.clone()
    }
}

#[cfg(test)]
mod image_source_tests {
    use super::*;

    #[test]
    fn data_uri_is_read_as_inline_base64() {
        let image = ImageSource::new("data:image/png;base64,AAAB");
        assert_eq!(
            image.payload(),
            ImagePayload::Base64 {
                media_type: Some("image/png"),
                data: "AAAB",
            }
        );
    }

    #[test]
    fn data_uri_without_media_type_falls_back_to_the_declared_one() {
        let image = ImageSource::new("data:;base64,AAAB").with_media_type("image/webp");
        assert_eq!(
            image.payload(),
            ImagePayload::Base64 {
                media_type: Some("image/webp"),
                data: "AAAB",
            }
        );
        let bare = ImageSource::new("data:;base64,AAAB");
        assert_eq!(
            bare.payload(),
            ImagePayload::Base64 {
                media_type: None,
                data: "AAAB",
            }
        );
    }

    #[test]
    fn non_base64_data_uri_is_not_treated_as_bytes() {
        let image = ImageSource::new("data:image/svg+xml,<svg/>");
        assert!(matches!(image.payload(), ImagePayload::Remote { .. }));
    }

    #[test]
    fn remote_url_keeps_its_scheme() {
        let image = ImageSource::new("https://example.test/a.png");
        assert_eq!(
            image.payload(),
            ImagePayload::Remote {
                url: "https://example.test/a.png",
                media_type: None,
            }
        );
    }

    #[test]
    fn degraded_note_never_dumps_base64_bytes() {
        let image = ImageSource::new("data:image/png;base64,QUJDREVGRw");
        let note = image.degraded_note("provider needs a media type");
        assert!(note.contains("provider needs a media type"), "{note}");
        assert!(!note.contains("QUJDREVGRw"), "{note}");
        assert!(note.contains("10 chars"), "{note}");
    }
}

/// A tool/function definition for provider requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Request to send to a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ProviderMessage>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<ProviderTool>>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub stream: bool,
    pub structured_output: Option<serde_json::Value>,
    /// Request-side controls that are orthogonal to the message payload.
    ///
    /// A caller that sets nothing here gets exactly the bytes it got before
    /// this field existed — [`RequestControls::default`] is today's behaviour,
    /// and the field is skipped entirely when serializing a default value.
    /// See `tests/request_body_golden.rs` for the byte-level guard.
    #[serde(default, skip_serializing_if = "RequestControls::is_default")]
    pub controls: RequestControls,
}

/// Non-streaming response from a provider.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub content: Vec<ProviderResponseBlock>,
    pub usage: ProviderUsage,
}

/// A response block.
#[derive(Debug, Clone)]
pub enum ProviderResponseBlock {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Token usage information.
///
/// # Prompt-cache token contract
///
/// Providers disagree about whether cached prompt tokens are counted inside
/// their prompt-token field: Anthropic **excludes** them from `input_tokens`,
/// while OpenAI, DeepSeek and Gemini **include** them. Rather than leak that to
/// every caller, the stream parsers normalise on the Anthropic convention:
///
/// - `input_tokens` — prompt tokens that were **not** served from cache.
/// - `cache_read_tokens` — prompt tokens served from cache (billed at a
///   discount by every provider that reports them).
/// - `cache_creation_tokens` — prompt tokens **written** to the cache this
///   request (billed at a premium). Only Anthropic reports this separately;
///   automatic-prefix providers fold cache writes into `input_tokens`.
///
/// Total prompt size is therefore always
/// `input_tokens + cache_read_tokens + cache_creation_tokens`
/// (see [`ProviderUsage::total_prompt_tokens`]).
///
/// `None` means "the provider did not report this", which is distinct from
/// `Some(0)` ("reported, and it was zero"). Callers that persist cache metrics
/// must preserve that distinction.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    /// Prompt tokens written to the provider's prompt cache this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// Prompt tokens served from the provider's prompt cache this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl ProviderUsage {
    /// Full prompt size including cached and cache-written tokens.
    pub fn total_prompt_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cache_creation_tokens.unwrap_or(0))
            .saturating_add(self.cache_read_tokens.unwrap_or(0))
    }

    /// Whether the provider reported any prompt-cache activity at all.
    pub fn reported_cache(&self) -> bool {
        self.cache_creation_tokens.is_some() || self.cache_read_tokens.is_some()
    }

    /// Fold a later usage report into an earlier one.
    ///
    /// Anthropic sends the full prompt breakdown once on `message_start` and
    /// then a terminal `message_delta` that carries only the fields it knows;
    /// naively replacing the accumulator there loses the input and cache
    /// counts. Non-zero / `Some` values from `next` win, everything else is
    /// carried forward.
    pub fn merge_from(&mut self, next: &ProviderUsage) {
        if next.input_tokens > 0 {
            self.input_tokens = next.input_tokens;
        }
        if next.output_tokens > 0 {
            self.output_tokens = next.output_tokens;
        }
        if next.reasoning_tokens.is_some() {
            self.reasoning_tokens = next.reasoning_tokens;
        }
        if next.cache_creation_tokens.is_some() {
            self.cache_creation_tokens = next.cache_creation_tokens;
        }
        if next.cache_read_tokens.is_some() {
            self.cache_read_tokens = next.cache_read_tokens;
        }
        if next.cost_usd.is_some() {
            self.cost_usd = next.cost_usd;
        }
    }
}

/// Which tool (if any) the model is forced to call.
///
/// Wire encoding is provider-specific; see
/// [`ToolChoice::to_anthropic`] / [`ToolChoice::to_openai`].
/// Provider error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderError {
    pub code: String,
    pub message: String,
    pub category: ProviderErrorCategory,
    pub retryable: bool,
    #[serde(default)]
    pub retry_after_ms: Option<u64>,
}

/// Error category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    Auth,
    RateLimit,
    QuotaExceeded,
    ModelNotFound,
    ContextLengthExceeded,
    BadRequest,
    ServerError,
    Timeout,
    Network,
    Unknown,
}

/// Runtime credential material for a single request (never logged).
#[derive(Clone)]
pub struct Credential {
    pub api_key: String,
    pub base_url: Option<String>,
    /// Per-request outbound proxy. Kept memory-only alongside the credential.
    pub proxy_url: Option<String>,
    pub key_id: Option<String>,
    pub provider_type: Option<String>,
    /// Google Cloud project id for OAuth-backed Cloud Code providers
    /// (antigravity) — sent as `x-goog-cloud-target-resource`. Optional.
    pub project_id: Option<String>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url.as_ref().map(|_| "[REDACTED]"))
            .field("proxy_url", &self.proxy_url.as_ref().map(|_| "[REDACTED]"))
            .field("key_id", &self.key_id)
            .field("provider_type", &self.provider_type)
            .field("project_id", &self.project_id)
            .finish()
    }
}

#[cfg(test)]
mod credential_tests {
    use super::Credential;

    #[test]
    fn debug_redacts_runtime_secret_and_urls() {
        let credential = Credential {
            api_key: "secret-api-key".into(),
            base_url: Some("https://example.test?key=base-secret".into()),
            proxy_url: Some("http://proxy-user:proxy-secret@example.test".into()),
            key_id: Some("key-1".into()),
            provider_type: Some("gemini".into()),
            project_id: Some("project-1".into()),
        };

        let debug = format!("{credential:?}");
        assert!(!debug.contains("secret-api-key"));
        assert!(!debug.contains("base-secret"));
        assert!(!debug.contains("proxy-secret"));
        assert!(debug.contains("[REDACTED]"));
    }
}

/// Model information from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: Option<String>,
    pub context_window: u64,
    pub max_output: u64,
    pub capabilities: ModelCapabilities,
}

/// Provider connection test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTestResult {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub message: String,
}
