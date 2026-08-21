use agent_core::{EngineProviderEvent, ProviderStopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    ChatCompletions,
    Responses,
    Messages,
}

#[derive(Debug, Default)]
struct ToolIdentity {
    id: Option<String>,
    name: Option<String>,
    announced: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Terminal {
    Open,
    Completed,
    Error,
}

pub(super) struct StreamEncoder {
    protocol: Protocol,
    tools: BTreeMap<usize, ToolIdentity>,
    usage: Option<Usage>,
    terminal: Terminal,
}

impl StreamEncoder {
    pub fn new(path: &str) -> Result<Self, String> {
        let protocol = match path {
            "/v1/chat/completions" => Protocol::ChatCompletions,
            "/v1/responses" => Protocol::Responses,
            "/v1/messages" => Protocol::Messages,
            _ => return Err(format!("unsupported loopback protocol path: {path}")),
        };
        Ok(Self {
            protocol,
            tools: BTreeMap::new(),
            usage: None,
            terminal: Terminal::Open,
        })
    }

    pub fn encode(&mut self, event: EngineProviderEvent) -> Result<String, String> {
        if self.terminal != Terminal::Open {
            return Err("provider emitted an event after the terminal event".into());
        }
        match event {
            EngineProviderEvent::TextDelta(text) => Ok(self.text_delta(text)),
            EngineProviderEvent::ReasoningDelta(text) => Ok(self.reasoning_delta(text)),
            EngineProviderEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => self.tool_delta(index, id, name, arguments_delta),
            EngineProviderEvent::Usage {
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            } => {
                let usage = Usage {
                    input_tokens,
                    output_tokens,
                    reasoning_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                };
                self.usage = Some(usage.clone());
                Ok(match self.protocol {
                    Protocol::ChatCompletions => sse_data(chat_usage(&usage)),
                    Protocol::Responses | Protocol::Messages => String::new(),
                })
            }
            EngineProviderEvent::Completed => self.completed(ProviderStopReason::Stop),
            EngineProviderEvent::CompletedWithReason { reason } => self.completed(reason),
            EngineProviderEvent::Error {
                message,
                code,
                retryable,
                category,
                retry_after_ms,
            } => {
                self.terminal = Terminal::Error;
                Ok(self.error_frame(message, code, retryable, category, retry_after_ms))
            }
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal != Terminal::Open
    }

    pub fn chat_done_frame(&self) -> Option<&'static str> {
        (self.protocol == Protocol::ChatCompletions && self.terminal == Terminal::Completed)
            .then_some("data: [DONE]\n\n")
    }

    fn text_delta(&self, text: String) -> String {
        match self.protocol {
            Protocol::ChatCompletions => sse_data(json!({
                "id": "chatcmpl-local",
                "object": "chat.completion.chunk",
                "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
            })),
            Protocol::Responses => sse_event(
                "response.output_text.delta",
                json!({"type": "response.output_text.delta", "delta": text}),
            ),
            Protocol::Messages => sse_event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": text}
                }),
            ),
        }
    }

    fn reasoning_delta(&self, text: String) -> String {
        match self.protocol {
            Protocol::ChatCompletions => sse_data(json!({
                "id": "chatcmpl-local",
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {"reasoning_content": text},
                    "finish_reason": null
                }]
            })),
            Protocol::Responses => sse_event(
                "response.reasoning_summary_text.delta",
                json!({
                    "type": "response.reasoning_summary_text.delta",
                    "delta": text
                }),
            ),
            Protocol::Messages => sse_event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "thinking_delta", "thinking": text}
                }),
            ),
        }
    }

    fn tool_delta(
        &mut self,
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    ) -> Result<String, String> {
        let identity = self.tools.entry(index).or_default();
        if id.as_ref().is_some_and(|value| !value.is_empty()) {
            identity.id = id;
        }
        if name.as_ref().is_some_and(|value| !value.is_empty()) {
            identity.name = name;
        }

        if self.protocol == Protocol::ChatCompletions {
            return Ok(sse_data(json!({
                "id": "chatcmpl-local",
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {"tool_calls": [{
                        "index": index,
                        "id": identity.id,
                        "type": "function",
                        "function": {
                            "name": identity.name,
                            "arguments": arguments_delta
                        }
                    }]},
                    "finish_reason": null
                }]
            })));
        }

        let mut frame = String::new();
        if !identity.announced {
            let Some(id) = identity.id.as_deref() else {
                if arguments_delta.is_empty() {
                    return Ok(frame);
                }
                return Err("tool arguments arrived before the tool id".into());
            };
            let Some(name) = identity.name.as_deref() else {
                if arguments_delta.is_empty() {
                    return Ok(frame);
                }
                return Err("tool arguments arrived before the tool name".into());
            };
            match self.protocol {
                Protocol::Messages => frame.push_str(&sse_event(
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": {}
                        }
                    }),
                )),
                Protocol::Responses => frame.push_str(&sse_event(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "output_index": index,
                        "item": {
                            "type": "function_call",
                            "id": id,
                            "call_id": id,
                            "name": name,
                            "arguments": ""
                        }
                    }),
                )),
                Protocol::ChatCompletions => unreachable!(),
            }
            identity.announced = true;
        }
        if arguments_delta.is_empty() {
            return Ok(frame);
        }
        match self.protocol {
            Protocol::Messages => frame.push_str(&sse_event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": arguments_delta
                    }
                }),
            )),
            Protocol::Responses => frame.push_str(&sse_event(
                "response.function_call_arguments.delta",
                json!({
                    "type": "response.function_call_arguments.delta",
                    "output_index": index,
                    "item_id": identity.id,
                    "call_id": identity.id,
                    "delta": arguments_delta
                }),
            )),
            Protocol::ChatCompletions => unreachable!(),
        }
        Ok(frame)
    }

    fn completed(&mut self, reason: ProviderStopReason) -> Result<String, String> {
        self.terminal = Terminal::Completed;
        Ok(match self.protocol {
            Protocol::ChatCompletions => sse_data(json!({
                "id": "chatcmpl-local",
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": chat_stop_reason(&reason)
                }]
            })),
            Protocol::Messages => {
                let mut terminal = json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": messages_stop_reason(&reason)}
                });
                if let Some(usage) = &self.usage {
                    terminal["usage"] = anthropic_usage(usage);
                }
                format!(
                    "{}{}",
                    sse_event("message_delta", terminal),
                    sse_event("message_stop", json!({"type": "message_stop"}))
                )
            }
            Protocol::Responses => {
                let event_type = responses_terminal_type(&reason);
                let mut response = json!({
                    "id": "resp_local",
                    "object": "response",
                    "status": responses_status(&reason),
                    "output": response_tool_output(&self.tools)
                });
                if let Some(usage) = &self.usage {
                    response["usage"] = responses_usage(usage);
                }
                if reason == ProviderStopReason::Length {
                    response["incomplete_details"] = json!({"reason": "max_output_tokens"});
                }
                sse_event(
                    event_type,
                    json!({"type": event_type, "response": response}),
                )
            }
        })
    }

    fn error_frame(
        &self,
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    ) -> String {
        match self.protocol {
            Protocol::ChatCompletions => sse_data(json!({
                "error": {
                    "message": message,
                    "code": code,
                    "type": category,
                    "retryable": retryable,
                    "retry_after_ms": retry_after_ms
                }
            })),
            Protocol::Messages => sse_event(
                "error",
                json!({
                    "type": "error",
                    "error": {"type": code, "message": message}
                }),
            ),
            Protocol::Responses => sse_event(
                "response.failed",
                json!({
                    "type": "response.failed",
                    "response": {
                        "status": "failed",
                        "error": {"code": code, "message": message}
                    }
                }),
            ),
        }
    }
}

fn response_tool_output(tools: &BTreeMap<usize, ToolIdentity>) -> Value {
    Value::Array(
        tools
            .values()
            .filter_map(|tool| {
                Some(json!({
                    "type": "function_call",
                    "id": tool.id.as_deref()?,
                    "call_id": tool.id.as_deref()?,
                    "name": tool.name.as_deref()?,
                    "arguments": ""
                }))
            })
            .collect(),
    )
}

fn chat_usage(usage: &Usage) -> Value {
    json!({
        "id": "chatcmpl-local",
        "object": "chat.completion.chunk",
        "choices": [],
        "usage": openai_usage(usage)
    })
}

pub(super) fn openai_usage(usage: &Usage) -> Value {
    json!({
        "prompt_tokens": usage.input_tokens
            .saturating_add(usage.cache_creation_tokens.unwrap_or(0))
            .saturating_add(usage.cache_read_tokens.unwrap_or(0)),
        "completion_tokens": usage.output_tokens,
        "prompt_tokens_details": {"cached_tokens": usage.cache_read_tokens.unwrap_or(0)},
        "completion_tokens_details": {
            "reasoning_tokens": usage.reasoning_tokens.unwrap_or(0)
        }
    })
}

pub(super) fn responses_usage(usage: &Usage) -> Value {
    json!({
        "input_tokens": usage.input_tokens
            .saturating_add(usage.cache_creation_tokens.unwrap_or(0))
            .saturating_add(usage.cache_read_tokens.unwrap_or(0)),
        "output_tokens": usage.output_tokens,
        "input_tokens_details": {"cached_tokens": usage.cache_read_tokens.unwrap_or(0)},
        "output_tokens_details": {
            "reasoning_tokens": usage.reasoning_tokens.unwrap_or(0)
        }
    })
}

pub(super) fn anthropic_usage(usage: &Usage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cache_creation_input_tokens": usage.cache_creation_tokens.unwrap_or(0),
        "cache_read_input_tokens": usage.cache_read_tokens.unwrap_or(0)
    })
}

pub(super) fn chat_stop_reason(reason: &ProviderStopReason) -> &str {
    match reason {
        ProviderStopReason::ToolUse => "tool_calls",
        ProviderStopReason::Length => "length",
        ProviderStopReason::Cancelled => "cancelled",
        ProviderStopReason::Error => "error",
        ProviderStopReason::Unknown(value) => value,
        ProviderStopReason::Stop => "stop",
    }
}

pub(super) fn messages_stop_reason(reason: &ProviderStopReason) -> &str {
    match reason {
        ProviderStopReason::ToolUse => "tool_use",
        ProviderStopReason::Length => "max_tokens",
        ProviderStopReason::Cancelled => "cancelled",
        ProviderStopReason::Error => "error",
        ProviderStopReason::Unknown(value) => value,
        ProviderStopReason::Stop => "end_turn",
    }
}

fn responses_terminal_type(reason: &ProviderStopReason) -> &'static str {
    match reason {
        ProviderStopReason::Length => "response.incomplete",
        ProviderStopReason::Cancelled => "response.cancelled",
        _ => "response.completed",
    }
}

pub(super) fn responses_status(reason: &ProviderStopReason) -> &'static str {
    match reason {
        ProviderStopReason::Length => "incomplete",
        ProviderStopReason::Cancelled => "cancelled",
        ProviderStopReason::Error => "failed",
        _ => "completed",
    }
}

fn sse_data(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn sse_event(event: &str, value: Value) -> String {
    format!("event: {event}\ndata: {value}\n\n")
}

#[cfg(test)]
#[path = "stream_encoder_tests.rs"]
mod tests;
