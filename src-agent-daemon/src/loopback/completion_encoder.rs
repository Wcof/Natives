use agent_core::{EngineProviderEvent, ProviderStopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::stream_encoder::{
    anthropic_usage, chat_stop_reason, messages_stop_reason, openai_usage, responses_status,
    responses_usage, Usage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    ChatCompletions,
    Responses,
    Messages,
}

#[derive(Debug, Default)]
struct ToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub(super) struct CompletionEncoder {
    protocol: Protocol,
    model: String,
    text: String,
    reasoning: String,
    tools: BTreeMap<usize, ToolCall>,
    usage: Option<Usage>,
    stop_reason: Option<ProviderStopReason>,
}

impl CompletionEncoder {
    pub fn new(path: &str, model: &str) -> Result<Self, String> {
        let protocol = match path {
            "/v1/chat/completions" => Protocol::ChatCompletions,
            "/v1/responses" => Protocol::Responses,
            "/v1/messages" => Protocol::Messages,
            _ => return Err(format!("unsupported loopback protocol path: {path}")),
        };
        Ok(Self {
            protocol,
            model: model.to_string(),
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            usage: None,
            stop_reason: None,
        })
    }

    pub fn push(&mut self, event: EngineProviderEvent) -> Result<(), String> {
        if self.stop_reason.is_some() {
            return Err("provider emitted an event after the terminal event".into());
        }
        match event {
            EngineProviderEvent::TextDelta(delta) => self.text.push_str(&delta),
            EngineProviderEvent::ReasoningDelta(delta) => self.reasoning.push_str(&delta),
            EngineProviderEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let tool = self.tools.entry(index).or_default();
                if id.as_ref().is_some_and(|value| !value.is_empty()) {
                    tool.id = id;
                }
                if name.as_ref().is_some_and(|value| !value.is_empty()) {
                    tool.name = name;
                }
                tool.arguments.push_str(&arguments_delta);
            }
            EngineProviderEvent::Usage {
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            } => {
                self.usage = Some(Usage {
                    input_tokens,
                    output_tokens,
                    reasoning_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                });
            }
            EngineProviderEvent::Completed => self.stop_reason = Some(ProviderStopReason::Stop),
            EngineProviderEvent::CompletedWithReason { reason } => self.stop_reason = Some(reason),
            EngineProviderEvent::Error { message, .. } => return Err(message),
        }
        Ok(())
    }

    pub fn is_terminal(&self) -> bool {
        self.stop_reason.is_some()
    }

    pub fn finish(self) -> Result<Value, String> {
        let reason = self
            .stop_reason
            .as_ref()
            .ok_or_else(|| "provider stream ended without a terminal event".to_string())?;
        match self.protocol {
            Protocol::ChatCompletions => self.chat_payload(reason),
            Protocol::Responses => self.responses_payload(reason),
            Protocol::Messages => self.messages_payload(reason),
        }
    }

    fn chat_payload(&self, reason: &ProviderStopReason) -> Result<Value, String> {
        let mut message = json!({"role": "assistant", "content": self.text});
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = json!(self.reasoning);
        }
        let tools = self
            .tools
            .values()
            .map(openai_tool)
            .collect::<Result<Vec<_>, _>>()?;
        if !tools.is_empty() {
            message["tool_calls"] = Value::Array(tools);
        }
        let mut payload = json!({
            "id": "chatcmpl-local",
            "object": "chat.completion",
            "model": self.model,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": chat_stop_reason(reason)
            }]
        });
        if let Some(usage) = &self.usage {
            payload["usage"] = openai_usage(usage);
        }
        Ok(payload)
    }

    fn responses_payload(&self, reason: &ProviderStopReason) -> Result<Value, String> {
        let mut output = Vec::new();
        if !self.reasoning.is_empty() {
            output.push(json!({
                "id": "rs_local",
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": self.reasoning}]
            }));
        }
        if !self.text.is_empty() {
            output.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": self.text}]
            }));
        }
        output.extend(
            self.tools
                .values()
                .map(responses_tool)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut payload = json!({
            "id": "resp_local",
            "object": "response",
            "model": self.model,
            "status": responses_status(reason),
            "output": output
        });
        if let Some(usage) = &self.usage {
            payload["usage"] = responses_usage(usage);
        }
        if reason == &ProviderStopReason::Length {
            payload["incomplete_details"] = json!({"reason": "max_output_tokens"});
        }
        Ok(payload)
    }

    fn messages_payload(&self, reason: &ProviderStopReason) -> Result<Value, String> {
        let mut content = Vec::new();
        if !self.reasoning.is_empty() {
            content.push(json!({"type": "thinking", "thinking": self.reasoning}));
        }
        if !self.text.is_empty() {
            content.push(json!({"type": "text", "text": self.text}));
        }
        content.extend(
            self.tools
                .values()
                .map(messages_tool)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut payload = json!({
            "id": "msg_local",
            "type": "message",
            "role": "assistant",
            "model": self.model,
            "content": content,
            "stop_reason": messages_stop_reason(reason)
        });
        if let Some(usage) = &self.usage {
            payload["usage"] = anthropic_usage(usage);
        }
        Ok(payload)
    }
}

fn required_tool_fields(tool: &ToolCall) -> Result<(&str, &str), String> {
    let id = tool
        .id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "provider tool call completed without an id".to_string())?;
    let name = tool
        .name
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "provider tool call completed without a name".to_string())?;
    Ok((id, name))
}

fn openai_tool(tool: &ToolCall) -> Result<Value, String> {
    let (id, name) = required_tool_fields(tool)?;
    Ok(json!({
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": tool.arguments}
    }))
}

fn responses_tool(tool: &ToolCall) -> Result<Value, String> {
    let (id, name) = required_tool_fields(tool)?;
    Ok(json!({
        "type": "function_call",
        "id": id,
        "call_id": id,
        "name": name,
        "arguments": tool.arguments
    }))
}

fn messages_tool(tool: &ToolCall) -> Result<Value, String> {
    let (id, name) = required_tool_fields(tool)?;
    let input = serde_json::from_str::<Value>(&tool.arguments)
        .map_err(|_| "provider tool call completed with invalid JSON arguments".to_string())?;
    Ok(json!({"type": "tool_use", "id": id, "name": name, "input": input}))
}

#[cfg(test)]
#[path = "completion_encoder_tests.rs"]
mod tests;
