//! Offline fixture provider (extracted from `production.rs`, task-01 structure).
//!
//! Emits tool_call then text for integration tests — never fake "hello from adapter" success
//! content for tools.

use agent_core::{
    EngineError, EngineMessage, EngineProvider, EngineProviderEvent, EngineProviderEventStream,
    ToolSchema,
};
use tokio_util::sync::CancellationToken;

/// Fixture provider for offline tests — emits tool_call then text, never fake "hello from adapter" as success content for tools.
pub struct FixtureProvider {
    pub mode: FixtureMode,
}

#[derive(Clone, Copy)]
pub enum FixtureMode {
    TextOnly,
    ToolThenText,
    RequestPermissionPath,
}

#[async_trait::async_trait]
impl EngineProvider for FixtureProvider {
    async fn stream(
        &self,
        _model: &str,
        messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        // If last message is a tool result, complete with text.
        if messages.last().map(|m| m.role == "tool").unwrap_or(false) {
            return Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("tool path complete".into()),
                EngineProviderEvent::Completed,
            ])));
        }
        let events = match self.mode {
            FixtureMode::TextOnly => vec![
                EngineProviderEvent::TextDelta("fixture answer".into()),
                EngineProviderEvent::Completed,
            ],
            FixtureMode::ToolThenText => vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_1".into()),
                    name: Some("read_file".into()),
                    arguments_delta: r#"{"path":"Cargo.toml"}"#.into(),
                },
                EngineProviderEvent::CompletedWithReason {
                    reason: agent_core::ProviderStopReason::ToolUse,
                },
            ],
            // Side-effecting tool so PermissionManager ConfirmEach emits permission_requested.
            FixtureMode::RequestPermissionPath => vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_perm".into()),
                    name: Some("write_file".into()),
                    arguments_delta: r#"{"path":"/tmp/natives-perm-test.txt","content":"x"}"#
                        .into(),
                },
                EngineProviderEvent::CompletedWithReason {
                    reason: agent_core::ProviderStopReason::ToolUse,
                },
            ],
        };
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}
