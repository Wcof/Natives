//! 3 上游协议 Encoder（CanonicalRequest → Chat, Responses, Messages）。

use serde_json::{json, Value};

use super::types::{CanonicalRequest, ProtocolKind};

pub fn encode_upstream_request(
    protocol: ProtocolKind,
    req: &CanonicalRequest,
    upstream_model: &str,
) -> Result<Value, String> {
    match protocol {
        ProtocolKind::ChatCompletions => encode_to_chat_completions(req, upstream_model),
        ProtocolKind::Responses => encode_to_responses(req, upstream_model),
        ProtocolKind::Messages => encode_to_messages(req, upstream_model),
    }
}

pub fn encode_to_chat_completions(
    req: &CanonicalRequest,
    upstream_model: &str,
) -> Result<Value, String> {
    let mut messages = Vec::new();

    if let Some(system) = &req.system {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }

    for msg in &req.messages {
        let mut msg_obj = json!({
            "role": msg.role,
            "content": msg.content,
        });

        if let Some(tool_calls) = &msg.tool_calls {
            let calls_arr: Vec<_> = tool_calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": {
                            "name": c.name,
                            "arguments": c.arguments,
                        }
                    })
                })
                .collect();
            msg_obj["tool_calls"] = Value::Array(calls_arr);
        }

        if let Some(call_id) = &msg.tool_call_id {
            msg_obj["tool_call_id"] = json!(call_id);
        }
        if let Some(name) = &msg.tool_name {
            msg_obj["name"] = json!(name);
        }

        messages.push(msg_obj);
    }

    let mut body = json!({
        "model": upstream_model,
        "messages": messages,
        "stream": req.stream,
    });

    if !req.tools.is_empty() {
        let tools_arr: Vec<_> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools_arr);
    }

    if let Some(tc) = &req.tool_choice {
        body["tool_choice"] = tc.clone();
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(max) = req.max_tokens {
        body["max_tokens"] = json!(max);
    }
    if let Some(effort) = &req.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    if let Some(schema) = &req.structured_output {
        body["response_format"] = schema.clone();
    }

    Ok(body)
}

pub fn encode_to_responses(req: &CanonicalRequest, upstream_model: &str) -> Result<Value, String> {
    let mut input = Vec::new();

    for msg in &req.messages {
        input.push(json!({
            "role": msg.role,
            "content": msg.content,
        }));
    }

    let mut body = json!({
        "model": upstream_model,
        "input": input,
        "stream": req.stream,
    });

    if let Some(sys) = &req.system {
        body["instructions"] = json!(sys);
    }

    if !req.tools.is_empty() {
        let tools_arr: Vec<_> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        body["tools"] = Value::Array(tools_arr);
    }

    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(max) = req.max_tokens {
        body["max_output_tokens"] = json!(max);
    }

    Ok(body)
}

pub fn encode_to_messages(req: &CanonicalRequest, upstream_model: &str) -> Result<Value, String> {
    let mut messages = Vec::new();

    for msg in &req.messages {
        let mut content = Vec::new();
        if !msg.content.is_empty() {
            content.push(json!({
                "type": "text",
                "text": msg.content,
            }));
        }

        if let Some(tool_calls) = &msg.tool_calls {
            for c in tool_calls {
                let input_val = serde_json::from_str::<Value>(&c.arguments).unwrap_or(json!({}));
                content.push(json!({
                    "type": "tool_use",
                    "id": c.id,
                    "name": c.name,
                    "input": input_val,
                }));
            }
        }

        if let Some(call_id) = &msg.tool_call_id {
            content.push(json!({
                "type": "tool_result",
                "tool_use_id": call_id,
                "content": msg.content,
            }));
        }

        messages.push(json!({
            "role": msg.role,
            "content": content,
        }));
    }

    let mut body = json!({
        "model": upstream_model,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(4096),
        "stream": req.stream,
    });

    if let Some(sys) = &req.system {
        body["system"] = json!(sys);
    }

    if !req.tools.is_empty() {
        let tools_arr: Vec<_> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        body["tools"] = Value::Array(tools_arr);
    }

    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(budget) = req.thinking_budget {
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }

    Ok(body)
}
