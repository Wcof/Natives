//! 可替换 ProxyEngine 边界（ADR-0020 §4：协议转换由可替换 Engine 执行）。
//!
//! ProxyEngine 是深 module：隐藏 Connection 解析、Credential 选择、协议策略、
//! HTTP、stream lifecycle 与 usage normalization（provider-proxy-architecture
//! §9.2）。实现可以是 Host 内嵌（复用 provider-adapters codec）或受监督
//! sidecar；本 trait 是替换接缝。
//!
//! P0-A 已证明三协议 codec / SSE parser / transport / Key Pool / SecretStore
//! 可用；生产 Engine 实现按 ADR-0020 P0 Gate 通过后接入。

use async_trait::async_trait;

use crate::secrets::store::SecretRef;

/// 一次上游请求的已解析上下文（Host 组装，Engine 消费）。
#[derive(Debug, Clone)]
pub struct EngineCall {
    /// 目标协议：`anthropic_messages` | `openai_chat_completions` | `openai_responses`。
    pub protocol: String,
    pub base_url: String,
    /// Keychain secret_ref（Engine 经 SecretStore 读取，不接触明文 DB）。
    pub secret_ref: SecretRef,
    pub model: String,
    /// 序列化后的规范请求体（ProviderRequest JSON）。
    pub request_json: String,
}

/// 一次调用的结果（流式或完整）。生产 Engine 返回规范化事件流；
/// trait 层先用可测试的聚合结果表达生命周期。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOutcome {
    /// 调用成功，返回归一化 usage/stop reason。
    Completed {
        usage_input_tokens: u64,
        usage_output_tokens: u64,
        stop_reason: String,
    },
    /// 结构化失败（Engine 必须给出可分类错误，不得吞异常）。
    Failed {
        category: String,
        code: String,
        retryable: bool,
    },
    /// Engine 当前不可用（sidecar 未就绪 / 被监督器停摆）。
    Unavailable { reason: String },
}

/// 可替换 Engine 接缝（深 module，隐藏协议/传输/usage 归一化细节）。
#[async_trait]
pub trait ProxyEngine: Send + Sync {
    fn name(&self) -> &'static str;

    /// 执行一次上游调用。
    async fn call(&self, request: EngineCall) -> Result<EngineOutcome, EngineError>;
}

/// Engine 层错误：不携带 Secret / 上游原始响应体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    pub category: String,
    pub code: String,
    pub retryable: bool,
    pub message: String,
}

impl EngineError {
    pub fn new(
        category: impl Into<String>,
        code: impl Into<String>,
        retryable: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            category: category.into(),
            code: code.into(),
            retryable,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::store::SecretRef;
    use std::sync::Arc;

    /// 可替换 Engine 接缝的 mock 实现：证明 trait 可被替换、错误可分类。
    struct FakeEngine;

    #[async_trait]
    impl ProxyEngine for FakeEngine {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn call(&self, request: EngineCall) -> Result<EngineOutcome, EngineError> {
            if request.model == "fail" {
                return Err(EngineError::new("rate_limit", "429", true, "upstream 429"));
            }
            Ok(EngineOutcome::Completed {
                usage_input_tokens: 10,
                usage_output_tokens: 5,
                stop_reason: "stop".into(),
            })
        }
    }

    #[tokio::test]
    async fn engine_trait_is_replaceable_and_classifies_errors() {
        let engine: Arc<dyn ProxyEngine> = Arc::new(FakeEngine);
        let base = EngineCall {
            protocol: "openai_chat_completions".into(),
            base_url: "http://127.0.0.1:8080".into(),
            secret_ref: SecretRef::new("cred:provider:p1:k1"),
            model: "gpt-4o".into(),
            request_json: "{}".into(),
        };

        let ok = engine.call(base.clone()).await.unwrap();
        assert_eq!(
            ok,
            EngineOutcome::Completed {
                usage_input_tokens: 10,
                usage_output_tokens: 5,
                stop_reason: "stop".into()
            }
        );

        let failing = engine
            .call(EngineCall {
                model: "fail".into(),
                ..base
            })
            .await
            .unwrap_err();
        assert_eq!(failing.category, "rate_limit");
        assert!(failing.retryable);
    }
}
