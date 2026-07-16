//! runtime/native/stream_provider.rs — SSE 流式数据提供者（原子能力层）
//!
//! 从旧版 native_runtime.rs 中提取的独立 SSE 流式消费者。
//! 职责单一：接收 HTTP SSE 流 → 解析为结构化事件。
//!
//! 对比旧版手动 buffer 解析，本层提供：
//!   - `SseEvent` 枚举：清晰的事件类型化
//!   - 统一的 `DeltaConsumer` trait：支持多种流式协议（OpenAI / Anthropic / 自定义）
//!   - 事件累积器：自动合并分块 tool_calls
//!   - 独立于 agent loop：可被其他 runtime 复用

use futures_util::StreamExt;
use serde_json::json;
use std::time::Duration;

// ──────────────────────────────────────────────
// SSE 事件类型
// ──────────────────────────────────────────────

/// 经过解析后的 SSE 数据块
#[derive(Clone, Debug)]
pub enum SseEvent {
    /// 文本 content delta
    ContentDelta {
        text: String,
        reasoning: Option<String>,
    },
    /// tool_call 累积片段（需要合并到 accumulator）
    ToolCallFragment {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_fragment: String,
    },
    /// 本轮结束标志
    FinishReason(String),
    /// API 错误
    ApiError { status: u16, body: String },
}

/// 累积后的完整 tool_call
#[derive(Clone, Debug)]
pub struct AccumulatedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

// ──────────────────────────────────────────────
// ToolCall 累积器
// ──────────────────────────────────────────────

/// 工具调用累积器（合并流式分片）
#[derive(Default)]
pub struct ToolCallAccumulator {
    slots: Vec<(String, String, String)>, // (id, name, arguments)
}

impl ToolCallAccumulator {
    /// 应用一个 fragment。工具调用只有在本轮结束时统一 drain，避免
    /// id/name 先到、参数后到时提前产生空参数和重复事件。
    pub fn apply(&mut self, fragment: &SseEvent) -> Option<AccumulatedToolCall> {
        match fragment {
            SseEvent::ToolCallFragment {
                index,
                id,
                name,
                arguments_fragment,
            } => {
                while self.slots.len() <= *index {
                    self.slots
                        .push((String::new(), String::new(), String::new()));
                }
                let slot = &mut self.slots[*index];
                if let Some(id) = id {
                    slot.0 = id.clone();
                }
                if let Some(name) = name {
                    slot.1 = name.clone();
                }
                slot.2.push_str(arguments_fragment);

                None
            }
            _ => None,
        }
    }

    /// 消费所有累积的工具调用
    pub fn drain(&mut self) -> Vec<AccumulatedToolCall> {
        let mut result = Vec::new();
        for (id, name, args_str) in self.slots.drain(..) {
            if !id.is_empty() && !name.is_empty() {
                let args = serde_json::from_str(&args_str).unwrap_or(json!({}));
                result.push(AccumulatedToolCall {
                    id,
                    name,
                    arguments: args,
                });
            }
        }
        result
    }
}

// ──────────────────────────────────────────────
// Delta Consumer — 流式数据消费者
// ──────────────────────────────────────────────

/// SSE 流解析结果
#[derive(Clone, Debug)]
pub enum ConsumeResult {
    /// 文本增量
    Delta(String),
    /// 推理内容增量
    Reasoning(String),
    /// 工具调用结束（完整）
    ToolCall(AccumulatedToolCall),
    /// 本轮结束
    Done,
    /// 跳过（空行 / keepalive）
    Skip,
}

/// 流式数据消费者 trait
///
/// 抽象不同供应商的 SSE 格式差异。
/// 目前支持 OpenAI 兼容格式；可扩展 Anthropic 格式。
pub trait DeltaConsumer: Send + Sync {
    /// 消费一段 SSE data 行，返回解析结果
    fn consume(&mut self, data: &str) -> Vec<ConsumeResult>;
    /// 清空剩余的累积工具调用
    fn drain_tools(&mut self) -> Vec<AccumulatedToolCall> {
        vec![]
    }
}

/// OpenAI 兼容格式的 SSE 消费者
pub struct OpenAiDeltaConsumer {
    accumulator: ToolCallAccumulator,
}

impl OpenAiDeltaConsumer {
    pub fn new() -> Self {
        Self {
            accumulator: ToolCallAccumulator::default(),
        }
    }

    pub fn into_accumulator(self) -> ToolCallAccumulator {
        self.accumulator
    }

    pub fn drain_tools(&mut self) -> Vec<AccumulatedToolCall> {
        self.accumulator.drain()
    }
}

impl Default for OpenAiDeltaConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl DeltaConsumer for OpenAiDeltaConsumer {
    fn consume(&mut self, data: &str) -> Vec<ConsumeResult> {
        let mut results = Vec::new();

        // 解析 JSON
        let parsed: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return vec![ConsumeResult::Skip],
        };

        let choices = match parsed["choices"].as_array() {
            Some(c) if !c.is_empty() => c,
            _ => return vec![ConsumeResult::Skip],
        };

        let choice = &choices[0];
        let delta = &choice["delta"];

        // 1. reasoning_content（深度求索系供应商）
        if let Some(r) = delta["reasoning_content"].as_str() {
            if !r.is_empty() {
                results.push(ConsumeResult::Reasoning(r.to_string()));
            }
        }

        // 2. content delta
        if let Some(text) = delta["content"].as_str() {
            if !text.is_empty() {
                results.push(ConsumeResult::Delta(text.to_string()));
            }
        }

        // 3. tool_calls delta（流式分片）
        if let Some(tc_arr) = delta["tool_calls"].as_array() {
            for tc in tc_arr {
                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                let id = tc["id"].as_str().map(|s| s.to_string());
                let name = tc["function"]["name"].as_str().map(|s| s.to_string());
                let args_frag = tc["function"]["arguments"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                if let Some(complete) = self.accumulator.apply(&SseEvent::ToolCallFragment {
                    index: idx,
                    id,
                    name,
                    arguments_fragment: args_frag,
                }) {
                    results.push(ConsumeResult::ToolCall(complete));
                }
            }
        }

        // 4. finish_reason
        if let Some(reason) = choice["finish_reason"].as_str() {
            if !reason.is_empty() && reason != "null" {
                results.push(ConsumeResult::Done);
            }
        }

        results
    }
}

// ──────────────────────────────────────────────
// SSE 流客户端
// ──────────────────────────────────────────────

/// SSE 流客户端
pub struct SseClient {
    client: reqwest::Client,
}

impl SseClient {
    pub fn new() -> crate::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| crate::Error::Internal(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self { client })
    }

    /// 发起流式请求并返回字节流
    pub async fn stream_chat(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        cancel_rx: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> std::result::Result<reqwest::Response, String> {
        let body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "tools": tools,
        });

        let completions_url = if base_url.ends_with("/chat/completions") {
            base_url.to_string()
        } else {
            format!("{}/chat/completions", base_url.trim_end_matches('/'))
        };

        let request = self
            .client
            .post(&completions_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body);

        tokio::select! {
            biased;
            _ = cancel_rx => {
                Err("Cancelled".into())
            }
            result = request.send() => {
                let response = result.map_err(|e| format!("HTTP request failed: {e}"))?;
                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();
                    return Err(format!("API error {}: {}", status, body_text));
                }
                Ok(response)
            }
        }
    }
}

// ──────────────────────────────────────────────
// 流式解析器
// ──────────────────────────────────────────────

/// 从 HTTP 响应字节流中逐行解析 SSE，驱动 consumer。
pub async fn parse_sse_stream(
    response: reqwest::Response,
    consumer: &mut dyn DeltaConsumer,
) -> (Vec<ConsumeResult>, Vec<AccumulatedToolCall>) {
    let mut all_results = Vec::new();
    let mut buffer = String::new();

    let mut stream = response.bytes_stream();
    'sse: while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(_) => break,
        };
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);

        // 逐行解析
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data == "[DONE]" {
                break 'sse;
            }

            let events = consumer.consume(data);
            all_results.extend(events);
        }
    }

    let remaining = consumer.drain_tools();
    (all_results, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_consumer_content_delta() {
        let mut consumer = OpenAiDeltaConsumer::new();
        let data = r#"{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let results = consumer.consume(data);
        assert!(results
            .iter()
            .any(|r| matches!(r, ConsumeResult::Delta(t) if t == "Hello")));
    }

    #[test]
    fn test_openai_consumer_finish_reason() {
        let mut consumer = OpenAiDeltaConsumer::new();
        let data = r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        let results = consumer.consume(data);
        assert!(results.iter().any(|r| matches!(r, ConsumeResult::Done)));
    }

    #[test]
    fn test_tool_call_accumulator_drain() {
        let mut acc = ToolCallAccumulator::default();
        acc.apply(&SseEvent::ToolCallFragment {
            index: 0,
            id: Some("call_1".into()),
            name: Some("read_file".into()),
            arguments_fragment: r#"{"path":"/tmp"}"#.into(),
        });
        let drained = acc.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].id, "call_1");
        assert_eq!(drained[0].name, "read_file");
    }

    #[test]
    fn fragmented_tool_call_emits_once_with_complete_arguments() {
        let mut acc = ToolCallAccumulator::default();
        assert!(acc
            .apply(&SseEvent::ToolCallFragment {
                index: 0,
                id: Some("call_1".into()),
                name: Some("read_file".into()),
                arguments_fragment: String::from("{\"path\":\""),
            })
            .is_none());
        assert!(acc
            .apply(&SseEvent::ToolCallFragment {
                index: 0,
                id: None,
                name: None,
                arguments_fragment: String::from("/tmp\"}"),
            })
            .is_none());
        let drained = acc.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].arguments["path"], "/tmp");
    }
}
