//! 响应流与完整响应编码器。

use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::types::{CanonicalEvent, CanonicalUsage, ProtocolKind};

pub struct InboundStreamEncoder {
    protocol: ProtocolKind,
    model: String,
    tools: BTreeMap<usize, (Option<String>, Option<String>, bool)>,
    usage: Option<CanonicalUsage>,
    completed: bool,
}

impl InboundStreamEncoder {
    pub fn new(protocol: ProtocolKind, model: String) -> Self {
        Self {
            protocol,
            model,
            tools: BTreeMap::new(),
            usage: None,
            completed: false,
        }
    }

    pub fn encode_event(&mut self, event: CanonicalEvent) -> String {
        match event {
            CanonicalEvent::TextDelta(text) => match self.protocol {
                ProtocolKind::ChatCompletions => format!(
                    "data: {}\n\n",
                    json!({
                        "id": "chatcmpl-proxy",
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
                    })
                ),
                ProtocolKind::Responses => format!(
                    "event: response.output_text.delta\ndata: {}\n\n",
                    json!({"type": "response.output_text.delta", "delta": text})
                ),
                ProtocolKind::Messages => format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {"type": "text_delta", "text": text}
                    })
                ),
            },
            CanonicalEvent::ReasoningDelta(text) => match self.protocol {
                ProtocolKind::ChatCompletions => format!(
                    "data: {}\n\n",
                    json!({
                        "id": "chatcmpl-proxy",
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {"reasoning_content": text}, "finish_reason": null}]
                    })
                ),
                ProtocolKind::Responses => format!(
                    "event: response.reasoning_summary_text.delta\ndata: {}\n\n",
                    json!({"type": "response.reasoning_summary_text.delta", "delta": text})
                ),
                ProtocolKind::Messages => format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {"type": "thinking_delta", "thinking": text}
                    })
                ),
            },
            CanonicalEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let entry = self.tools.entry(index).or_insert((None, None, false));
                if id.is_some() {
                    entry.0 = id.clone();
                }
                if name.is_some() {
                    entry.1 = name.clone();
                }

                match self.protocol {
                    ProtocolKind::ChatCompletions => format!(
                        "data: {}\n\n",
                        json!({
                            "id": "chatcmpl-proxy",
                            "object": "chat.completion.chunk",
                            "model": self.model,
                            "choices": [{
                                "index": 0,
                                "delta": {"tool_calls": [{
                                    "index": index,
                                    "id": entry.0,
                                    "type": "function",
                                    "function": {
                                        "name": entry.1,
                                        "arguments": arguments_delta,
                                    }
                                }]},
                                "finish_reason": null
                            }]
                        })
                    ),
                    ProtocolKind::Responses => {
                        let mut out = String::new();
                        if !entry.2 {
                            if let (Some(id_val), Some(name_val)) = (&entry.0, &entry.1) {
                                out.push_str(&format!(
                                    "event: response.output_item.added\ndata: {}\n\n",
                                    json!({
                                        "type": "response.output_item.added",
                                        "output_index": index,
                                        "item": {
                                            "type": "function_call",
                                            "id": id_val,
                                            "call_id": id_val,
                                            "name": name_val,
                                            "arguments": ""
                                        }
                                    })
                                ));
                                entry.2 = true;
                            }
                        }
                        if !arguments_delta.is_empty() {
                            out.push_str(&format!(
                                "event: response.function_call_arguments.delta\ndata: {}\n\n",
                                json!({
                                    "type": "response.function_call_arguments.delta",
                                    "output_index": index,
                                    "delta": arguments_delta,
                                })
                            ));
                        }
                        out
                    }
                    ProtocolKind::Messages => {
                        let mut out = String::new();
                        if !entry.2 {
                            if let (Some(id_val), Some(name_val)) = (&entry.0, &entry.1) {
                                out.push_str(&format!(
                                    "event: content_block_start\ndata: {}\n\n",
                                    json!({
                                        "type": "content_block_start",
                                        "index": index,
                                        "content_block": {
                                            "type": "tool_use",
                                            "id": id_val,
                                            "name": name_val,
                                            "input": {}
                                        }
                                    })
                                ));
                                entry.2 = true;
                            }
                        }
                        if !arguments_delta.is_empty() {
                            out.push_str(&format!(
                                "event: content_block_delta\ndata: {}\n\n",
                                json!({
                                    "type": "content_block_delta",
                                    "index": index,
                                    "delta": {
                                        "type": "input_json_delta",
                                        "partial_json": arguments_delta
                                    }
                                })
                            ));
                        }
                        out
                    }
                }
            }
            CanonicalEvent::Usage(usage) => {
                self.usage = Some(usage.clone());
                match self.protocol {
                    ProtocolKind::ChatCompletions => format!(
                        "data: {}\n\n",
                        json!({
                            "id": "chatcmpl-proxy",
                            "object": "chat.completion.chunk",
                            "model": self.model,
                            "choices": [],
                            "usage": {
                                "prompt_tokens": usage.prompt_tokens,
                                "completion_tokens": usage.completion_tokens,
                                "total_tokens": usage.prompt_tokens + usage.completion_tokens,
                            }
                        })
                    ),
                    ProtocolKind::Responses | ProtocolKind::Messages => String::new(),
                }
            }
            CanonicalEvent::Completed { finish_reason } => {
                self.completed = true;
                match self.protocol {
                    ProtocolKind::ChatCompletions => {
                        let finish = match finish_reason.as_str() {
                            "tool_use" => "tool_calls",
                            "length" => "length",
                            _ => "stop",
                        };
                        format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({
                                "id": "chatcmpl-proxy",
                                "object": "chat.completion.chunk",
                                "model": self.model,
                                "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]
                            })
                        )
                    }
                    ProtocolKind::Responses => {
                        format!(
                            "event: response.completed\ndata: {}\n\n",
                            json!({
                                "type": "response.completed",
                                "response": {
                                    "id": "resp_proxy",
                                    "status": "completed",
                                    "model": self.model,
                                }
                            })
                        )
                    }
                    ProtocolKind::Messages => {
                        let stop = match finish_reason.as_str() {
                            "tool_use" => "tool_use",
                            "length" => "max_tokens",
                            _ => "end_turn",
                        };
                        format!(
                            "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
                            json!({
                                "type": "message_delta",
                                "delta": {"stop_reason": stop}
                            }),
                            json!({"type": "message_stop"})
                        )
                    }
                }
            }
            CanonicalEvent::Error {
                message,
                code,
                category,
                retryable: _,
            } => match self.protocol {
                ProtocolKind::ChatCompletions => format!(
                    "data: {}\n\n",
                    json!({
                        "error": {
                            "message": message,
                            "type": category,
                            "code": code,
                        }
                    })
                ),
                ProtocolKind::Responses => format!(
                    "event: response.failed\ndata: {}\n\n",
                    json!({
                        "type": "response.failed",
                        "response": {
                            "status": "failed",
                            "error": {"code": code, "message": message}
                        }
                    })
                ),
                ProtocolKind::Messages => format!(
                    "event: error\ndata: {}\n\n",
                    json!({
                        "type": "error",
                        "error": {"type": code, "message": message}
                    })
                ),
            },
        }
    }
}

pub struct InboundCompletionEncoder {
    protocol: ProtocolKind,
    model: String,
    text: String,
    reasoning: String,
    tools: BTreeMap<usize, (Option<String>, Option<String>, String)>,
    usage: Option<CanonicalUsage>,
    finish_reason: Option<String>,
}

impl InboundCompletionEncoder {
    pub fn new(protocol: ProtocolKind, model: String) -> Self {
        Self {
            protocol,
            model,
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            usage: None,
            finish_reason: None,
        }
    }

    pub fn push_event(&mut self, event: CanonicalEvent) {
        match event {
            CanonicalEvent::TextDelta(t) => self.text.push_str(&t),
            CanonicalEvent::ReasoningDelta(r) => self.reasoning.push_str(&r),
            CanonicalEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let entry = self
                    .tools
                    .entry(index)
                    .or_insert((None, None, String::new()));
                if id.is_some() {
                    entry.0 = id;
                }
                if name.is_some() {
                    entry.1 = name;
                }
                entry.2.push_str(&arguments_delta);
            }
            CanonicalEvent::Usage(u) => self.usage = Some(u),
            CanonicalEvent::Completed { finish_reason } => self.finish_reason = Some(finish_reason),
            CanonicalEvent::Error { message, .. } => {
                self.finish_reason = Some(format!("error: {message}"))
            }
        }
    }

    pub fn finish(self) -> Result<Value, String> {
        let reason = self.finish_reason.unwrap_or_else(|| "stop".into());

        match self.protocol {
            ProtocolKind::ChatCompletions => {
                let finish = match reason.as_str() {
                    "tool_use" => "tool_calls",
                    "length" => "length",
                    _ => "stop",
                };

                let mut msg = json!({
                    "role": "assistant",
                    "content": self.text,
                });

                if !self.reasoning.is_empty() {
                    msg["reasoning_content"] = json!(self.reasoning);
                }

                if !self.tools.is_empty() {
                    let tool_calls: Vec<_> = self
                        .tools
                        .values()
                        .map(|(id, name, args)| {
                            json!({
                                "id": id.as_deref().unwrap_or("call_1"),
                                "type": "function",
                                "function": {
                                    "name": name.as_deref().unwrap_or("unknown"),
                                    "arguments": args,
                                }
                            })
                        })
                        .collect();
                    msg["tool_calls"] = Value::Array(tool_calls);
                }

                let usage_obj = self.usage.as_ref().map(|u| {
                    json!({
                        "prompt_tokens": u.prompt_tokens,
                        "completion_tokens": u.completion_tokens,
                        "total_tokens": u.prompt_tokens + u.completion_tokens,
                    })
                });

                Ok(json!({
                    "id": "chatcmpl-proxy",
                    "object": "chat.completion",
                    "model": self.model,
                    "choices": [{
                        "index": 0,
                        "message": msg,
                        "finish_reason": finish,
                    }],
                    "usage": usage_obj.unwrap_or_else(|| json!({"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}))
                }))
            }
            ProtocolKind::Responses => {
                let mut output = Vec::new();
                if !self.text.is_empty() {
                    output.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": self.text}]
                    }));
                }
                for (id, name, args) in self.tools.values() {
                    output.push(json!({
                        "type": "function_call",
                        "id": id.as_deref().unwrap_or("call_1"),
                        "call_id": id.as_deref().unwrap_or("call_1"),
                        "name": name.as_deref().unwrap_or("unknown"),
                        "arguments": args,
                    }));
                }

                Ok(json!({
                    "id": "resp_proxy",
                    "object": "response",
                    "model": self.model,
                    "status": "completed",
                    "output": output,
                }))
            }
            ProtocolKind::Messages => {
                let stop = match reason.as_str() {
                    "tool_use" => "tool_use",
                    "length" => "max_tokens",
                    _ => "end_turn",
                };

                let mut content = Vec::new();
                if !self.reasoning.is_empty() {
                    content.push(json!({"type": "thinking", "thinking": self.reasoning}));
                }
                if !self.text.is_empty() {
                    content.push(json!({"type": "text", "text": self.text}));
                }
                for (id, name, args) in self.tools.values() {
                    let input_val = serde_json::from_str::<Value>(args).unwrap_or(json!({}));
                    content.push(json!({
                        "type": "tool_use",
                        "id": id.as_deref().unwrap_or("tool_1"),
                        "name": name.as_deref().unwrap_or("unknown"),
                        "input": input_val,
                    }));
                }

                let usage_obj = self.usage.as_ref().map(|u| {
                    json!({
                        "input_tokens": u.prompt_tokens,
                        "output_tokens": u.completion_tokens,
                    })
                });

                Ok(json!({
                    "id": "msg_proxy",
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": content,
                    "stop_reason": stop,
                    "usage": usage_obj.unwrap_or_else(|| json!({"input_tokens": 0, "output_tokens": 0}))
                }))
            }
        }
    }
}
