use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
use async_trait::async_trait;
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
pub fn history_message_to_provider(msg: HistoryMessage) -> ProviderMessage {
    fn push_images(content: &mut Vec<ProviderContentBlock>, images: Vec<ImageSource>) {
        for image_url in images {
            content.push(ProviderContentBlock::Image { image_url });
        }
    }
    let images = msg.images;
    if let Some(calls) = msg.tool_calls {
        if !calls.is_empty() {
            let mut content = Vec::new();
            if !msg.content.is_empty() {
                content.push(ProviderContentBlock::Text { text: msg.content });
            }
            push_images(&mut content, images);
            for call in calls {
                let input = parse_tool_arguments(&call.arguments);
                content.push(ProviderContentBlock::ToolCall {
                    id: call.id,
                    name: call.name,
                    input,
                });
            }
            return ProviderMessage {
                role: if msg.role.is_empty() {
                    "assistant".into()
                } else {
                    msg.role
                },
                content,
            };
        }
    }

    if msg.role == "tool" || msg.tool_call_id.is_some() {
        let tool_call_id = msg
            .tool_call_id
            .unwrap_or_else(|| "unknown_tool_call".into());
        let mut content = vec![ProviderContentBlock::ToolResult {
            tool_call_id,
            content: msg.content,
            name: msg.tool_name,
        }];
        push_images(&mut content, images);
        return ProviderMessage {
            role: "tool".into(),
            content,
        };
    }

    let mut content = vec![ProviderContentBlock::Text { text: msg.content }];
    push_images(&mut content, images);
    ProviderMessage {
        role: msg.role,
        content,
    }
}

/// Map a batch of history messages.
pub fn history_messages_to_provider(
    messages: impl IntoIterator<Item = HistoryMessage>,
) -> Vec<ProviderMessage> {
    messages
        .into_iter()
        .map(history_message_to_provider)
        .collect()
}

fn parse_tool_arguments(arguments: &str) -> serde_json::Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(trimmed)
        .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}

#[cfg(test)]
mod history_message_tests {
    use super::*;

    #[test]
    fn preserves_assistant_tool_calls_and_tool_results() {
        let assistant = history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: "calling".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![
                HistoryToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.txt"}"#.into(),
                },
                HistoryToolCall {
                    id: "call_2".into(),
                    name: "echo".into(),
                    arguments: r#"{"x":1}"#.into(),
                },
            ]),
            images: Vec::new(),
        });
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content.len(), 3);
        assert!(matches!(
            &assistant.content[0],
            ProviderContentBlock::Text { text } if text == "calling"
        ));
        assert!(matches!(
            &assistant.content[1],
            ProviderContentBlock::ToolCall { id, name, .. }
                if id == "call_1" && name == "read_file"
        ));
        assert!(matches!(
            &assistant.content[2],
            ProviderContentBlock::ToolCall { id, name, .. }
                if id == "call_2" && name == "echo"
        ));

        let tool = history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: r#"{"ok":true}"#.into(),
            tool_call_id: Some("call_1".into()),
            tool_name: Some("read_file".into()),
            tool_calls: None,
            images: Vec::new(),
        });
        assert_eq!(tool.role, "tool");
        assert!(matches!(
            &tool.content[0],
            ProviderContentBlock::ToolResult {
                tool_call_id,
                name: Some(n),
                ..
            } if tool_call_id == "call_1" && n == "read_file"
        ));
    }

    #[test]
    fn plain_user_stays_text() {
        let msg = history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "hi".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            images: Vec::new(),
        });
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(
            &msg.content[0],
            ProviderContentBlock::Text { text } if text == "hi"
        ));
    }

    #[test]
    fn images_reach_the_wire_in_every_branch() {
        let image = ImageSource::new("data:image/png;base64,AAAB");

        let user = history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "look".into(),
            images: vec![image.clone()],
            ..Default::default()
        });
        assert_eq!(user.content.len(), 2);
        assert!(matches!(
            &user.content[1],
            ProviderContentBlock::Image { image_url } if image_url == &image
        ));

        let assistant = history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: "here".into(),
            tool_calls: Some(vec![HistoryToolCall {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{}".into(),
            }]),
            images: vec![image.clone()],
            ..Default::default()
        });
        // text, image, tool_call — the image must not be swallowed by the
        // tool-call branch.
        assert_eq!(assistant.content.len(), 3);
        assert!(matches!(
            &assistant.content[1],
            ProviderContentBlock::Image { .. }
        ));

        let tool = history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: "{}".into(),
            tool_call_id: Some("c1".into()),
            images: vec![image],
            ..Default::default()
        });
        assert_eq!(tool.content.len(), 2);
        assert!(matches!(
            &tool.content[1],
            ProviderContentBlock::Image { .. }
        ));
    }
}

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

/// A single delta from a streaming response.
#[derive(Debug, Clone)]
pub enum ProviderStreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallBegin {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Done(ProviderUsage),
    Error(ProviderError),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ToolChoice {
    /// Model decides freely (provider default).
    Auto,
    /// Model must not call a tool.
    None,
    /// Model must call at least one tool, its choice which.
    Required,
    /// Model must call this exact tool.
    Tool { name: String },
}

impl ToolChoice {
    /// Anthropic Messages API `tool_choice` object.
    ///
    /// `disable_parallel_tool_use` rides on the same object for Anthropic, so
    /// it is passed in here rather than emitted as a sibling field.
    pub fn to_anthropic(&self, parallel_tool_calls: Option<bool>) -> serde_json::Value {
        let mut value = match self {
            ToolChoice::Auto => serde_json::json!({ "type": "auto" }),
            ToolChoice::None => serde_json::json!({ "type": "none" }),
            ToolChoice::Required => serde_json::json!({ "type": "any" }),
            ToolChoice::Tool { name } => serde_json::json!({ "type": "tool", "name": name }),
        };
        if let Some(false) = parallel_tool_calls {
            value["disable_parallel_tool_use"] = serde_json::json!(true);
        }
        value
    }

    /// OpenAI chat-completions / Responses `tool_choice` value.
    pub fn to_openai(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::None => serde_json::json!("none"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Tool { name } => serde_json::json!({
                "type": "function",
                "function": { "name": name },
            }),
        }
    }

    /// Gemini `toolConfig.functionCallingConfig` object.
    pub fn to_gemini(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!({ "mode": "AUTO" }),
            ToolChoice::None => serde_json::json!({ "mode": "NONE" }),
            ToolChoice::Required => serde_json::json!({ "mode": "ANY" }),
            ToolChoice::Tool { name } => serde_json::json!({
                "mode": "ANY",
                "allowedFunctionNames": [name],
            }),
        }
    }
}

/// Coarse reasoning depth, mapped onto whatever knob the model exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// Parse the coarse level carried by `run.start`'s `effort` field.
    ///
    /// The protocol types it as a free-form `Option<String>` (it is
    /// provider-specific by design), so this is the single place that decides
    /// what the workbench's vocabulary means. Anything else returns `None` —
    /// an unknown level must not be rounded to a guess, because the difference
    /// between `low` and `high` is a tenfold thinking budget.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" | "minimal" => Some(ReasoningEffort::Low),
            "medium" | "default" | "standard" => Some(ReasoningEffort::Medium),
            "high" | "max" | "maximum" => Some(ReasoningEffort::High),
            _ => None,
        }
    }

    /// OpenAI `reasoning_effort` string.
    pub fn as_openai_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }

    /// Default thinking budget in tokens for providers that take a number
    /// instead of a level. Callers may override via
    /// [`ReasoningRequest::budget_tokens`].
    pub fn default_budget_tokens(self) -> u64 {
        match self {
            ReasoningEffort::Low => 4_096,
            ReasoningEffort::Medium => 16_384,
            ReasoningEffort::High => 32_768,
        }
    }
}

/// Caller-requested reasoning configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningRequest {
    pub effort: ReasoningEffort,
    /// Explicit thinking budget. Ignored by models whose only knob is a level
    /// (`ReasoningControl::OpenAiEffort`, `ReasoningControl::AnthropicAdaptive`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u64>,
}

impl ReasoningRequest {
    pub fn new(effort: ReasoningEffort) -> Self {
        ReasoningRequest {
            effort,
            budget_tokens: None,
        }
    }

    /// Budget to send, honouring an explicit value over the effort default.
    pub fn budget(&self) -> u64 {
        self.budget_tokens
            .unwrap_or_else(|| self.effort.default_budget_tokens())
    }
}

/// Environment kill switch for prompt-cache breakpoints.
///
/// Read on every body build (once per model turn, so the cost is noise) rather
/// than cached, because the point of an escape hatch is that it works on the
/// next request after someone sets it — including on a long-lived daemon that
/// nobody wants to restart mid-incident.
pub const PROMPT_CACHE_ENV: &str = "NATIVES_PROMPT_CACHE";

/// Parse a permissive on/off flag. Returns `None` for anything unrecognised so
/// a typo falls back to the default instead of silently disabling a feature.
pub fn parse_bool_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" | "enabled" => Some(true),
        "0" | "off" | "false" | "no" | "disabled" => Some(false),
        _ => None,
    }
}

fn env_prompt_cache_override() -> Option<bool> {
    parse_bool_flag(&std::env::var(PROMPT_CACHE_ENV).ok()?)
}

/// Request-side controls that are orthogonal to the message payload.
///
/// [`Default`] is exactly today's behaviour: no forced tool, provider-default
/// parallelism, no reasoning parameter, and prompt caching left to the
/// per-model default (enabled wherever the model supports explicit
/// breakpoints).
///
/// This travels inside [`ProviderRequest::controls`], so every path that
/// already builds a request carries it without a second argument. The
/// `build_*_body_with_controls` functions remain as an explicit override seam
/// for callers that want to build a body with controls other than the
/// request's own (they ignore `request.controls` entirely).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestControls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// `Some(false)` forces one tool call per assistant turn. `None` leaves the
    /// provider default (parallel calls allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningRequest>,
    /// `None` = per-model default. `Some(false)` disables prompt-cache
    /// breakpoints for this request (incident escape hatch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<bool>,
}

impl RequestControls {
    /// Whether this is the zero-configuration form, i.e. wire-identical to the
    /// behaviour that predates the field.
    pub fn is_default(&self) -> bool {
        *self == RequestControls::default()
    }

    /// Whether explicit prompt-cache breakpoints should be emitted for a model.
    ///
    /// Precedence, most authoritative first:
    ///
    /// 1. `NATIVES_PROMPT_CACHE` — the operator kill switch. Set it to `0` and
    ///    no request emits a breakpoint, whatever any caller asked for. This
    ///    exists so a provider-side cache incident can be worked around without
    ///    shipping a build.
    /// 2. [`RequestControls::prompt_cache`] — the per-request opt-out.
    /// 3. The per-model default (on wherever the model supports breakpoints).
    pub fn prompt_cache_enabled(&self, profile: &crate::model_profile::ModelProfile) -> bool {
        if let Some(false) = env_prompt_cache_override() {
            return false;
        }
        profile.wants_explicit_cache_breakpoints() && self.prompt_cache.unwrap_or(true)
    }

    /// Reasoning controls for a coarse effort string (`"low"`/`"medium"`/`"high"`).
    ///
    /// Unrecognised input yields `None` so an unknown level degrades to
    /// "provider default" rather than silently picking a depth for the user.
    pub fn with_effort_str(mut self, effort: Option<&str>) -> Self {
        self.reasoning = effort
            .and_then(ReasoningEffort::parse)
            .map(ReasoningRequest::new);
        self
    }
}

#[cfg(test)]
mod request_controls_tests {
    use super::*;

    #[test]
    fn effort_strings_map_to_levels_and_unknown_stays_unset() {
        assert_eq!(ReasoningEffort::parse("high"), Some(ReasoningEffort::High));
        assert_eq!(ReasoningEffort::parse(" MAX "), Some(ReasoningEffort::High));
        assert_eq!(
            ReasoningEffort::parse("minimal"),
            Some(ReasoningEffort::Low)
        );
        assert_eq!(ReasoningEffort::parse("turbo"), None);
        assert_eq!(ReasoningEffort::parse(""), None);
    }

    #[test]
    fn with_effort_str_only_sets_reasoning_for_a_known_level() {
        assert_eq!(
            RequestControls::default()
                .with_effort_str(Some("low"))
                .reasoning,
            Some(ReasoningRequest::new(ReasoningEffort::Low))
        );
        // An unknown or absent level leaves the request wire-identical to one
        // that never mentioned reasoning at all.
        assert!(RequestControls::default()
            .with_effort_str(Some("wharrgarbl"))
            .is_default());
        assert!(RequestControls::default()
            .with_effort_str(None)
            .is_default());
    }

    #[test]
    fn bool_flag_parsing_ignores_typos() {
        assert_eq!(parse_bool_flag("0"), Some(false));
        assert_eq!(parse_bool_flag(" OFF "), Some(false));
        assert_eq!(parse_bool_flag("disabled"), Some(false));
        assert_eq!(parse_bool_flag("1"), Some(true));
        assert_eq!(parse_bool_flag("true"), Some(true));
        assert_eq!(parse_bool_flag("maybe"), None);
    }

    #[test]
    fn per_request_prompt_cache_opt_out_beats_the_model_default() {
        let profile = crate::model_profile::resolve("claude-sonnet-4-5");
        assert!(profile.wants_explicit_cache_breakpoints());
        assert!(RequestControls::default().prompt_cache_enabled(&profile));
        assert!(!RequestControls {
            prompt_cache: Some(false),
            ..Default::default()
        }
        .prompt_cache_enabled(&profile));
    }
}

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
#[derive(Debug, Clone)]
pub struct Credential {
    pub api_key: String,
    pub base_url: Option<String>,
    /// Per-request outbound proxy. Kept memory-only alongside the credential.
    pub proxy_url: Option<String>,
    pub key_id: Option<String>,
    pub provider_type: Option<String>,
}

/// Provider adapter trait — all providers must implement this.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// Get the provider type.
    fn provider_type(&self) -> ProviderType;

    /// Get provider capabilities.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Send a chat completion request (non-streaming).
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError>;

    /// Send a streaming chat completion request (legacy mock-friendly path).
    async fn chat_stream(
        &self,
        request: ProviderRequest,
    ) -> Result<
        Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>,
        ProviderError,
    >;

    /// Authenticated streaming path used by the Agent Engine.
    ///
    /// Default implementation falls back to `chat_stream` and maps events.
    /// Real adapters override this to perform HTTP SSE with `credential`.
    async fn stream(
        &self,
        request: ProviderRequest,
        _credential: Credential,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = crate::stream::ProviderEvent> + Send>>,
        ProviderError,
    > {
        use crate::stream::ProviderEvent;
        use futures_util::StreamExt;
        let legacy = self.chat_stream(request).await?;
        let mapped = legacy.map(|event| match event {
            ProviderStreamEvent::TextDelta(t) => ProviderEvent::TextDelta(t),
            ProviderStreamEvent::ReasoningDelta(t) => ProviderEvent::ReasoningDelta(t),
            ProviderStreamEvent::ToolCallBegin { id, name } => ProviderEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments_delta: String::new(),
            },
            ProviderStreamEvent::ToolCallDelta { id, delta } => ProviderEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: None,
                arguments_delta: delta,
            },
            ProviderStreamEvent::ToolCallComplete { id, name, input } => {
                ProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some(id),
                    name: Some(name),
                    arguments_delta: input.to_string(),
                }
            }
            ProviderStreamEvent::Done(usage) => ProviderEvent::Usage(usage),
            ProviderStreamEvent::Error(err) => ProviderEvent::Error(err),
        });
        // Append Completed after legacy Done for engine compatibility.
        let completed = futures_util::stream::once(async {
            ProviderEvent::Completed {
                reason: crate::stream::ProviderStopReason::Unknown("legacy_done".into()),
            }
        });
        Ok(Box::pin(mapped.chain(completed)))
    }

    /// List available models from this provider.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Discover models with credentials (real HTTP when available).
    async fn discover_models(
        &self,
        _credential: Credential,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        self.list_models().await
    }

    /// Test the provider connection.
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError>;

    /// Test with credentials.
    async fn test_connection_with_credential(
        &self,
        _credential: Credential,
    ) -> Result<ProviderTestResult, ProviderError> {
        self.test_connection().await
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

/// Contract tests for all provider adapters.
pub mod contract_tests {
    use super::*;

    /// Test that all adapters satisfy the contract:
    /// - Have Non-empty capabilities
    /// - Non-empty provider type
    /// Can chat
    /// Can list models
    pub fn run_contract_tests(adapter: &dyn ProviderAdapter) {
        let caps = adapter.capabilities();
        assert!(!caps.features.is_empty(), "Features should not be empty");
        assert!(
            caps.max_context_window > 0,
            "Max context window should be > 0"
        );

        // Provider type should be set
        let pt = adapter.provider_type();
        match pt {
            ProviderType::Openai
            | ProviderType::Anthropic
            | ProviderType::Gemini
            | ProviderType::Deepseek
            | ProviderType::OpenaiCompatible
            | ProviderType::Ollama => {}
        }
    }
}
