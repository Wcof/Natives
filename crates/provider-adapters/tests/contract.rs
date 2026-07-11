//! Contract tests for provider adapters.
//!
//! Every registered adapter must satisfy the same contract suite.

use provider_adapters::capabilities::*;
use provider_adapters::capabilities::contract_tests::run_contract_tests;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};
use async_trait::async_trait;

/// Mock adapter for contract testing.
struct MockAdapter;

#[async_trait]
impl ProviderAdapter for MockAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Openai
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Openai,
            features: vec!["streaming".to_string(), "tool_calls".to_string()],
            max_context_window: 128_000,
            streaming: true,
            tool_calls: true,
            structured_output: true,
            image_input: true,
            file_input: true,
            reasoning: false,
            system_prompt: true,
            function_calling: true,
        }
    }

    async fn chat(&self, _request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text("Mock response".to_string())],
            usage: ProviderUsage {
                input_tokens: 10,
                output_tokens: 20,
                reasoning_tokens: None,
                cost_usd: Some(0.001),
            },
        })
    }

    async fn chat_stream(
        &self,
        _request: ProviderRequest,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Mock ".to_string()),
            ProviderStreamEvent::TextDelta("response".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4".to_string(),
                display_name: Some("GPT-4".to_string()),
                context_window: 8192,
                max_output: 4096,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: true,
                    reasoning: false,
                    tool_calling: true,
                    structured_output: true,
                    function_calling: true,
                    system_prompt: true,
                },
            },
        ])
    }

    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: true,
            latency_ms: Some(42),
            message: "Mock connection successful".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

#[test]
fn test_mock_adapter_contract() {
    let adapter = MockAdapter;
    run_contract_tests(&adapter);
}

#[test]
fn test_mock_adapter_capabilities() {
    let adapter = MockAdapter;
    let caps = adapter.capabilities();
    assert_eq!(caps.provider_type, ProviderType::Openai);
    assert!(caps.streaming);
    assert!(caps.tool_calls);
    assert!(caps.structured_output);
}

#[test]
fn test_mock_adapter_chat() {
    let adapter = MockAdapter;
    let request = ProviderRequest {
        model: "gpt-4".to_string(),
        messages: vec![],
        system_prompt: None,
        tools: None,
        max_tokens: Some(100),
        temperature: Some(0.7),
        stream: false,
        structured_output: None,
    };

    let result = futures::executor::block_on(adapter.chat(request)).unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.usage.input_tokens, 10);
}

#[test]
fn test_mock_adapter_list_models() {
    let adapter = MockAdapter;
    let models = futures::executor::block_on(adapter.list_models()).unwrap();
    assert!(!models.is_empty());
    assert_eq!(models[0].id, "gpt-4");
}

#[test]
fn test_mock_adapter_test_connection() {
    let adapter = MockAdapter;
    let result = futures::executor::block_on(adapter.test_connection()).unwrap();
    assert!(result.success);
    assert!(result.latency_ms.is_some());
}

#[test]
fn test_provider_error_serialization() {
    let error = ProviderError {
        code: "rate_limit_exceeded".to_string(),
        message: "Too many requests".to_string(),
        category: ProviderErrorCategory::RateLimit,
        retryable: true,
    };
    let json = serde_json::to_string(&error).unwrap();
    assert!(json.contains("rate_limit_exceeded"));
}

#[test]
fn test_provider_usage_default() {
    let usage = ProviderUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
}

#[test]
fn test_provider_capabilities_serialization() {
    let caps = ProviderCapabilities {
        provider_type: ProviderType::Openai,
        features: vec!["streaming".to_string()],
        max_context_window: 128000,
        streaming: true,
        tool_calls: true,
        structured_output: false,
        image_input: false,
        file_input: false,
        reasoning: false,
        system_prompt: true,
        function_calling: false,
    };
    let json = serde_json::to_value(&caps).unwrap();
    assert_eq!(json["provider_type"], "openai");
    assert!(json["streaming"].as_bool().unwrap());
}