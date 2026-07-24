//! Gemini GenerateContent adapter — real HTTP streaming (SSE / JSON array).

use async_trait::async_trait;
use crate::capabilities::*;
use crate::stream::{
    parse_gemini_chunk, split_sse_lines, sse_data_payload, ProviderEvent,
};
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

pub struct GeminiAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl GeminiAdapter {
    pub fn new() -> Self {
        GeminiAdapter {
            api_key: None,
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            client: Client::new(),
        }
    }
    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn build_generate_body(request: &ProviderRequest) -> serde_json::Value {
    let mut contents = Vec::new();
    for message in &request.messages {
        let role = if message.role == "assistant" {
            "model"
        } else if message.role == "tool" {
            "user"
        } else {
            "user"
        };
        let mut parts = Vec::new();
        for block in &message.content {
            match block {
                ProviderContentBlock::Text { text } => {
                    if !text.is_empty() {
                        parts.push(serde_json::json!({ "text": text }));
                    }
                }
                ProviderContentBlock::ToolCall { name, input, .. } => {
                    parts.push(serde_json::json!({
                        "functionCall": {
                            "name": name,
                            "args": input,
                        }
                    }));
                }
                ProviderContentBlock::ToolResult {
                    content,
                    name,
                    tool_call_id,
                } => {
                    let fn_name = name
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| tool_call_id.clone());
                    let response = serde_json::from_str::<serde_json::Value>(content)
                        .unwrap_or_else(|_| serde_json::json!({ "result": content }));
                    parts.push(serde_json::json!({
                        "functionResponse": {
                            "name": fn_name,
                            "response": response,
                        }
                    }));
                }
                ProviderContentBlock::Image { .. } => {}
            }
        }
        if parts.is_empty() {
            continue;
        }
        contents.push(serde_json::json!({ "role": role, "parts": parts }));
    }

    let mut body = serde_json::json!({ "contents": contents });
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": system }]
            });
        }
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            let decls: Vec<_> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
        }
    }
    if let Some(max) = request.max_tokens {
        body["generationConfig"] = serde_json::json!({ "maxOutputTokens": max });
    }
    body
}

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Gemini
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Gemini,
            features: vec![
                "streaming".into(),
                "tool_calls".into(),
                "image_input".into(),
                "reasoning".into(),
                "system_prompt".into(),
            ],
            max_context_window: 1_048_576,
            streaming: true,
            tool_calls: true,
            structured_output: true,
            image_input: true,
            file_input: true,
            reasoning: true,
            system_prompt: true,
            function_calling: true,
        }
    }

    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        if self.api_key.is_none() {
            return Err(ProviderError {
                code: "missing_key".into(),
                message: "Gemini API key required (no mock tool path)".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
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
                    provider_type: Some("gemini".into()),
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
        // No mock "Hello from Gemini" tool-call path — require credentials via stream().
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for Gemini; offline mock removed".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
            retry_after_ms: None,
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
                message: "Gemini API key required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            })?
        };
        let base = credential
            .base_url
            .unwrap_or_else(|| self.base_url.clone());
        let model = if request.model.is_empty() {
            "gemini-2.0-flash".to_string()
        } else {
            request.model.clone()
        };
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            base.trim_end_matches('/'),
            model,
            key
        );
        let body = build_generate_body(&request);

        let response = self
            .client
            .post(&url)
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
                retry_after_ms: None,
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let text = response.text().await.unwrap_or_default();
            return Err(crate::http_stream::map_http_status(status, &text, Some(&headers)));
        }

        let byte_stream = response.bytes_stream();
        let stream = async_stream::stream! {
            let mut buffer = String::new();
            let mut saw_completed = false;
            tokio::pin!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        for line in split_sse_lines(&mut buffer) {
                            if let Some(data) = sse_data_payload(&line) {
                                if data == "[DONE]" {
                                    saw_completed = true;
                                    yield ProviderEvent::Completed;
                                    continue;
                                }
                                for event in parse_gemini_chunk(data) {
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
                            retry_after_ms: None,
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
                id: "gemini-2.0-flash".into(),
                display_name: Some("Gemini 2.0 Flash".into()),
                context_window: 1_048_576,
                max_output: 8_192,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: true,
                    reasoning: true,
                    tool_calling: true,
                    structured_output: true,
                    function_calling: true,
                    system_prompt: true,
                },
            },
            ModelInfo {
                id: "gemini-2.5-pro".into(),
                display_name: Some("Gemini 2.5 Pro".into()),
                context_window: 1_048_576,
                max_output: 8_192,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: true,
                    reasoning: true,
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
            success: self.api_key.is_some(),
            latency_ms: None,
            message: if self.api_key.is_some() {
                "Gemini key present".into()
            } else {
                "No Gemini key — use stream with Credential".into()
            },
        })
    }
}

#[cfg(test)]
mod tool_message_tests {
    use super::*;

    #[test]
    fn gemini_body_uses_function_call_and_response() {
        let body = build_generate_body(&ProviderRequest {
            model: "gemini-2.0-flash".into(),
            messages: vec![
                ProviderMessage {
                    role: "user".into(),
                    content: vec![ProviderContentBlock::Text {
                        text: "weather?".into(),
                    }],
                },
                ProviderMessage {
                    role: "assistant".into(),
                    content: vec![ProviderContentBlock::ToolCall {
                        id: "c1".into(),
                        name: "get_weather".into(),
                        input: serde_json::json!({"city": "SF"}),
                    }],
                },
                ProviderMessage {
                    role: "tool".into(),
                    content: vec![ProviderContentBlock::ToolResult {
                        tool_call_id: "c1".into(),
                        content: r#"{"temp":72}"#.into(),
                        name: Some("get_weather".into()),
                    }],
                },
            ],
            system_prompt: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            stream: true,
            structured_output: None,
        });

        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "get_weather");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "get_weather"
        );
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["response"]["temp"],
            72
        );
    }
}
