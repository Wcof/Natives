//! Anthropic Messages API adapter — real HTTP streaming.

use async_trait::async_trait;
use crate::capabilities::*;
use crate::stream::{
    split_sse_lines, sse_data_payload, AnthropicSseParser, ProviderEvent,
};
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

pub struct AnthropicAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl AnthropicAdapter {
    pub fn new() -> Self {
        AnthropicAdapter {
            api_key: None,
            base_url: "https://api.anthropic.com".to_string(),
            client: Client::new(),
        }
    }
    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
}
impl Default for AnthropicAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn build_messages_body(request: &ProviderRequest) -> serde_json::Value {
    let mut messages = Vec::new();
    // Anthropic requires tool_result blocks to live in a user message, possibly batched.
    let mut pending_tool_results: Vec<serde_json::Value> = Vec::new();

    let flush_tool_results = |messages: &mut Vec<serde_json::Value>, pending: &mut Vec<serde_json::Value>| {
        if !pending.is_empty() {
            messages.push(serde_json::json!({
                "role": "user",
                "content": std::mem::take(pending),
            }));
        }
    };

    for message in &request.messages {
        let mut content_blocks = Vec::new();
        let mut is_tool_result_only = true;
        for block in &message.content {
            match block {
                ProviderContentBlock::Text { text } => {
                    if !text.is_empty() {
                        is_tool_result_only = false;
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }
                ProviderContentBlock::ToolCall { id, name, input } => {
                    is_tool_result_only = false;
                    content_blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                ProviderContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    pending_tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                    }));
                }
                ProviderContentBlock::Image { .. } => {
                    is_tool_result_only = false;
                }
            }
        }

        if message.role == "tool" || (is_tool_result_only && !pending_tool_results.is_empty() && content_blocks.is_empty()) {
            // Keep accumulating tool_result blocks; flushed before next non-tool message.
            continue;
        }

        flush_tool_results(&mut messages, &mut pending_tool_results);

        if content_blocks.is_empty() {
            continue;
        }
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        messages.push(serde_json::json!({
            "role": role,
            "content": content_blocks,
        }));
    }
    flush_tool_results(&mut messages, &mut pending_tool_results);

    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "stream": true,
    });
    if let Some(system) = &request.system_prompt {
        body["system"] = serde_json::json!(system);
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools
                .iter()
                .map(|t| serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                }))
                .collect::<Vec<_>>());
        }
    }
    body
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Anthropic,
            features: vec![
                "streaming".into(),
                "tool_calls".into(),
                "reasoning".into(),
                "system_prompt".into(),
            ],
            max_context_window: 200_000,
            streaming: true,
            tool_calls: true,
            structured_output: false,
            image_input: true,
            file_input: false,
            reasoning: true,
            system_prompt: true,
            function_calling: true,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        if self.api_key.is_none() {
            return Err(ProviderError {
                code: "missing_key".into(),
                message: "Anthropic API key required (offline mock removed)".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
            });
        }
        let mut text = String::new();
        let mut usage = ProviderUsage::default();
        let stream = self
            .stream(
                request,
                Credential {
                    api_key: self.api_key.clone().unwrap_or_default(),
                    base_url: Some(self.base_url.clone()),
                    key_id: None,
                    provider_type: Some("anthropic".into()),
                },
            )
            .await?;
        tokio::pin!(stream);
        while let Some(ev) = stream.next().await {
            match ev {
                ProviderEvent::TextDelta(t) => text.push_str(&t),
                ProviderEvent::Usage(u) => usage = u,
                ProviderEvent::Error(e) => return Err(e),
                _ => {}
            }
        }
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(text)],
            usage,
        })
    }
    async fn chat_stream(
        &self,
        _request: ProviderRequest,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError>
    {
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for Anthropic; offline mock removed".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
        })
    }
    async fn stream(
        &self,
        request: ProviderRequest,
        credential: Credential,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
        ProviderError,
    > {
        let key = if !credential.api_key.is_empty() {
            credential.api_key
        } else {
            self.api_key.clone().ok_or_else(|| ProviderError {
                code: "missing_key".into(),
                message: "Anthropic API key required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
            })?
        };
        let base = credential
            .base_url
            .unwrap_or_else(|| self.base_url.clone());
        let url = format!("{}/v1/messages", base.trim_end_matches('/'));
        let body = build_messages_body(&request);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError {
                code: "network".into(),
                message: e.to_string(),
                category: ProviderErrorCategory::Network,
                retryable: true,
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(crate::http_stream::map_http_status(status, &text));
        }

        let byte_stream = response.bytes_stream();
        let stream = async_stream::stream! {
            let mut parser = AnthropicSseParser::new();
            let mut buffer = String::new();
            let mut saw_completed = false;
            tokio::pin!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        for line in split_sse_lines(&mut buffer) {
                            if let Some(data) = sse_data_payload(&line) {
                                for event in parser.push_data_line(data) {
                                    if matches!(event, ProviderEvent::Completed) {
                                        saw_completed = true;
                                    }
                                    yield event;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        yield ProviderEvent::Error(ProviderError {
                            code: "stream_error".into(),
                            message: err.to_string(),
                            category: ProviderErrorCategory::Network,
                            retryable: true,
                        });
                        return;
                    }
                }
            }
            if !saw_completed {
                yield ProviderEvent::Completed;
            }
        };
        Ok(Box::pin(stream))
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "claude-sonnet-4-20250514".into(),
                display_name: Some("Claude Sonnet 4".into()),
                context_window: 200_000,
                max_output: 16_384,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: false,
                    reasoning: true,
                    tool_calling: true,
                    structured_output: false,
                    function_calling: true,
                    system_prompt: true,
                },
            },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: self.api_key.is_some(),
            latency_ms: None,
            message: "Anthropic adapter ready".into(),
        })
    }
}

#[cfg(test)]
mod tool_message_tests {
    use super::*;

    #[test]
    fn anthropic_body_uses_tool_use_and_tool_result_blocks() {
        let body = build_messages_body(&ProviderRequest {
            model: "claude-sonnet-4".into(),
            messages: vec![
                ProviderMessage {
                    role: "user".into(),
                    content: vec![ProviderContentBlock::Text {
                        text: "read it".into(),
                    }],
                },
                ProviderMessage {
                    role: "assistant".into(),
                    content: vec![ProviderContentBlock::ToolCall {
                        id: "toolu_1".into(),
                        name: "read_file".into(),
                        input: serde_json::json!({"path": "x"}),
                    }],
                },
                ProviderMessage {
                    role: "tool".into(),
                    content: vec![ProviderContentBlock::ToolResult {
                        tool_call_id: "toolu_1".into(),
                        content: "file data".into(),
                        name: Some("read_file".into()),
                    }],
                },
            ],
            system_prompt: Some("sys".into()),
            tools: None,
            max_tokens: Some(256),
            temperature: None,
            stream: true,
            structured_output: None,
        });

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[1]["content"][0]["id"], "toolu_1");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(messages[2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(messages[2]["content"][0]["content"], "file data");
    }
}
