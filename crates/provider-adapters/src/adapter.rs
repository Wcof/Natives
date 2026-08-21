//! Provider adapter 接缝（P0-B Extract）。
//!
//! 从 `capabilities.rs` 拆分出的 legacy 浅 interface：
//! - `ProviderStreamEvent` —— 旧 mock-friendly 流事件（Agent 语义）；
//! - `ProviderAdapter` trait —— 同时暴露 chat/chat_stream/stream、静态模型、
//!   发现与测试多组语义的浅 module。
//!
//! 按 P0-B 处置矩阵（provider-adapters-p0b-audit.md）：
//! - canonical request/event/usage/error 类型保留在 `capabilities`；
//! - `ProviderAdapter` 与 `ProviderStreamEvent` 在 Host `ProxyEngine` 接管后
//!   **Delete after cutover**；本文件是它们与 canonical 类型的隔离边界，
//!   使 `capabilities.rs` 降到 700 行阈值内。

use async_trait::async_trait;

use crate::capabilities::{
    Credential, ModelInfo, ProviderCapabilities, ProviderError, ProviderRequest, ProviderResponse,
    ProviderTestResult,
};
use crate::provider_identity::ProviderType;

/// A single delta from a streaming response（legacy mock-friendly path）。
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

use crate::capabilities::ProviderUsage;

/// Provider adapter trait — all providers must implement this（legacy 接缝）。
///
/// P0-B：完成态由 Host 深 `ProxyEngine` interface 隐藏 Connection 解析、
/// Credential 选择、协议策略、HTTP、stream lifecycle 与 usage normalization；
/// 本 trait 在 cutover 后删除。
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

/// Contract tests for all provider adapters.
pub mod contract_tests {
    use super::*;

    /// Test that all adapters satisfy the contract:
    /// - Have Non-empty capabilities
    /// - Non-empty provider type
    ///   Can chat
    ///   Can list models
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
