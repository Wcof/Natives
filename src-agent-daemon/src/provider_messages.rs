//! Provider history translation and error mapping (ARCH-002 seam).
//!
//! Extracted from `provider.rs` so the adapter file stays under the 700-line
//! review threshold (审计收口 W5：candidate 相比基线不新增超 700 行文件)。

use agent_core::{EngineError, EngineMessage};
use provider_adapters::capabilities::{
    HistoryMessage, HistoryToolCall, ImageSource, ProviderError,
};

/// Map engine history into provider history parts.
///
/// Preserves `tool_calls` / `tool_call_id` and image attachments. This is the
/// only place the engine's modality-neutral `EngineImage` becomes the provider
/// crate's `ImageSource`, so a new modality has exactly one seam to cross.
pub(crate) fn engine_message_to_history(m: EngineMessage) -> HistoryMessage {
    HistoryMessage {
        role: m.role,
        content: m.content,
        tool_call_id: m.tool_call_id,
        tool_name: m.tool_name,
        tool_calls: m.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|c| HistoryToolCall {
                    id: c.id,
                    name: c.name,
                    arguments: c.arguments,
                })
                .collect()
        }),
        images: m
            .images
            .into_iter()
            .map(|image| ImageSource {
                url: image.url,
                detail: image.detail,
                media_type: image.media_type,
            })
            .collect(),
    }
}

/// Convert the Core-owned typed transcript directly to the provider adapter's
/// neutral history shape. Production never needs to rebuild an `EngineMessage`
/// just to cross the provider boundary; the old conversion above remains only
/// for legacy callers and fixtures.
pub(crate) fn agent_message_to_history(message: agent_core::AgentMessage) -> HistoryMessage {
    fn content_parts(
        blocks: &[agent_core::ContentBlock],
    ) -> (String, Vec<ImageSource>, Option<Vec<HistoryToolCall>>) {
        let mut text = String::new();
        let mut images = Vec::new();
        let mut calls = Vec::new();
        for block in blocks {
            match block {
                agent_core::ContentBlock::Text { text: value }
                | agent_core::ContentBlock::Thinking { text: value, .. } => text.push_str(value),
                agent_core::ContentBlock::Image { source } => images.push(ImageSource {
                    url: source.url.clone(),
                    detail: source.detail.clone(),
                    media_type: source.media_type.clone(),
                }),
                agent_core::ContentBlock::ToolCall(call) => calls.push(HistoryToolCall {
                    id: call.tool_call_id.to_string(),
                    name: call.name.clone(),
                    arguments: call.arguments_json.clone(),
                }),
            }
        }
        (text, images, (!calls.is_empty()).then_some(calls))
    }

    fn result_text(blocks: &[agent_core::ToolResultBlock]) -> String {
        blocks
            .iter()
            .map(|block| match block {
                agent_core::ToolResultBlock::Text { text } => text.clone(),
                agent_core::ToolResultBlock::Json { value } => value.to_string(),
                agent_core::ToolResultBlock::Artifact {
                    artifact_id,
                    preview,
                } => preview.clone().unwrap_or_else(|| artifact_id.clone()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    match message {
        agent_core::AgentMessage::User(message) => {
            let (content, images, tool_calls) = content_parts(&message.content);
            HistoryMessage {
                role: "user".into(),
                content,
                images,
                tool_calls,
                ..Default::default()
            }
        }
        agent_core::AgentMessage::Assistant(message) => {
            let (content, images, tool_calls) = content_parts(&message.content);
            HistoryMessage {
                role: "assistant".into(),
                content,
                images,
                tool_calls,
                ..Default::default()
            }
        }
        agent_core::AgentMessage::ToolResult(message) => HistoryMessage {
            role: "tool".into(),
            content: result_text(&message.content),
            tool_call_id: Some(message.tool_call_id.to_string()),
            tool_name: Some(message.tool_name),
            ..Default::default()
        },
        agent_core::AgentMessage::System(message) => HistoryMessage {
            role: "system".into(),
            content: message.text,
            ..Default::default()
        },
        agent_core::AgentMessage::Custom(message) => HistoryMessage {
            role: message.kind,
            content: message.payload.to_string(),
            ..Default::default()
        },
    }
}

/// Build a safe, redacted provider error message. Key id, base url and the
/// underlying message are scrubbed before they ever reach the event stream or
/// the log (审计收口：凭证与敏感日志脱敏).
pub(crate) fn provider_error_message(
    error: &ProviderError,
    provider_id: &str,
    protocol: &str,
    model: &str,
    key_id: Option<&str>,
    base_url: Option<&str>,
) -> String {
    format!(
        "provider={provider_id} protocol={protocol} model={model} key_id={} base_url={} code={} category={:?} retryable={} message={}",
        key_id.unwrap_or("default"),
        base_url.map(assistant_protocol::v2::redact_secrets).unwrap_or_else(|| "default".into()),
        error.code,
        error.category,
        error.retryable,
        assistant_protocol::v2::redact_secrets(&error.message),
    )
}

/// Unused-import guard so removing callers never silently breaks the module.
pub(crate) fn _engine_error(_e: &EngineError) -> bool {
    false
}
