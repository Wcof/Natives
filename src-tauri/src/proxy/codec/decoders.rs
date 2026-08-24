//! 3 入站协议 Decoder（Chat, Responses, Messages → CanonicalRequest）。

use serde_json::{json, Value};

use super::types::{
    CanonicalImage, CanonicalMessage, CanonicalRequest, CanonicalTool, CanonicalToolCall,
    ProtocolKind,
};

pub fn decode_inbound_request(
    protocol: ProtocolKind,
    body: &Value,
) -> Result<CanonicalRequest, String> {
    match protocol {
        ProtocolKind::ChatCompletions => decode_chat_completions_request(body),
        ProtocolKind::Responses => decode_responses_request(body),
        ProtocolKind::Messages => decode_messages_request(body),
    }
}

pub fn decode_chat_completions_request(body: &Value) -> Result<CanonicalRequest, String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    if let Some(msg_arr) = body.get("messages").and_then(Value::as_array) {
        for item in msg_arr {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
            let content_str = parse_content_text(item.get("content").unwrap_or(&Value::Null));

            if role == "system" || role == "developer" {
                if !content_str.trim().is_empty() {
                    system_parts.push(content_str);
                }
                continue;
            }

            let tool_calls = item
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|c| {
                            let func = c.get("function").unwrap_or(c);
                            Some(CanonicalToolCall {
                                id: c.get("id")?.as_str()?.to_string(),
                                name: func.get("name")?.as_str()?.to_string(),
                                arguments: func
                                    .get("arguments")
                                    .map(|v| {
                                        v.as_str()
                                            .map(str::to_string)
                                            .unwrap_or_else(|| v.to_string())
                                    })
                                    .unwrap_or_else(|| "{}".into()),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|v| !v.is_empty());

            let tool_call_id = item
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let tool_name = item.get("name").and_then(Value::as_str).map(str::to_string);
            let images = parse_content_images(item.get("content").unwrap_or(&Value::Null));

            messages.push(CanonicalMessage {
                role: role.to_string(),
                content: content_str,
                tool_call_id,
                tool_name,
                tool_calls,
                images,
            });
        }
    }

    let tools = parse_tools(body);
    let tool_choice = body.get("tool_choice").cloned();
    let temperature = body.get("temperature").and_then(Value::as_f64);
    let max_tokens = body
        .get("max_tokens")
        .or_else(|| body.get("max_completion_tokens"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    let reasoning_effort = body
        .get("reasoning_effort")
        .or_else(|| body.pointer("/reasoning/effort"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let thinking_budget = body
        .pointer("/thinking/budget_tokens")
        .or_else(|| body.pointer("/reasoning/budget_tokens"))
        .and_then(Value::as_u64);

    let structured_output = body
        .pointer("/response_format/json_schema")
        .or_else(|| body.get("response_format"))
        .cloned();

    Ok(CanonicalRequest {
        model,
        messages,
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        },
        tools,
        tool_choice,
        temperature,
        max_tokens,
        stream,
        reasoning_effort,
        thinking_budget,
        structured_output,
    })
}

pub fn decode_responses_request(body: &Value) -> Result<CanonicalRequest, String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let mut system_parts = Vec::new();
    if let Some(inst) = body.get("instructions").or_else(|| body.get("system")) {
        let text = parse_content_text(inst);
        if !text.trim().is_empty() {
            system_parts.push(text);
        }
    }

    let mut messages = Vec::new();
    let source = body
        .get("input")
        .or_else(|| body.get("messages"))
        .unwrap_or(&Value::Null);

    if let Some(text) = source.as_str() {
        messages.push(CanonicalMessage {
            role: "user".into(),
            content: text.to_string(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            images: Vec::new(),
        });
    } else if let Some(items) = source.as_array() {
        for item in items {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
            let content_str = parse_content_text(item.get("content").unwrap_or(item));

            if role == "system" || role == "developer" {
                if !content_str.trim().is_empty() {
                    system_parts.push(content_str);
                }
                continue;
            }

            let images = parse_content_images(item.get("content").unwrap_or(item));
            messages.push(CanonicalMessage {
                role: role.to_string(),
                content: content_str,
                tool_call_id: item
                    .get("call_id")
                    .or_else(|| item.get("tool_call_id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                tool_name: item.get("name").and_then(Value::as_str).map(str::to_string),
                tool_calls: None,
                images,
            });
        }
    }

    let tools = parse_tools(body);
    let tool_choice = body.get("tool_choice").cloned();
    let temperature = body.get("temperature").and_then(Value::as_f64);
    let max_tokens = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    let reasoning_effort = body
        .pointer("/reasoning/effort")
        .and_then(Value::as_str)
        .map(str::to_string);

    let thinking_budget = body
        .pointer("/reasoning/budget_tokens")
        .and_then(Value::as_u64);

    let structured_output = body.get("text").and_then(|t| t.get("format")).cloned();

    Ok(CanonicalRequest {
        model,
        messages,
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        },
        tools,
        tool_choice,
        temperature,
        max_tokens,
        stream,
        reasoning_effort,
        thinking_budget,
        structured_output,
    })
}

pub fn decode_messages_request(body: &Value) -> Result<CanonicalRequest, String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let system = body
        .get("system")
        .map(parse_content_text)
        .filter(|s| !s.trim().is_empty());
    let mut messages = Vec::new();

    if let Some(items) = body.get("messages").and_then(Value::as_array) {
        for item in items {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
            let content_val = item.get("content").unwrap_or(&Value::Null);
            let content_str = parse_content_text(content_val);
            let mut tool_calls = Vec::new();
            let mut tool_call_id = None;
            let mut tool_name = None;

            if let Some(blocks) = content_val.as_array() {
                for block in blocks {
                    let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                    if block_type == "tool_use" {
                        if let (Some(id), Some(name)) = (
                            block.get("id").and_then(Value::as_str),
                            block.get("name").and_then(Value::as_str),
                        ) {
                            let input_args = block
                                .get("input")
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "{}".to_string());
                            tool_calls.push(CanonicalToolCall {
                                id: id.to_string(),
                                name: name.to_string(),
                                arguments: input_args,
                            });
                        }
                    } else if block_type == "tool_result" {
                        tool_call_id = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        tool_name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                }
            }

            let images = parse_content_images(content_val);

            messages.push(CanonicalMessage {
                role: role.to_string(),
                content: content_str,
                tool_call_id,
                tool_name,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                images,
            });
        }
    }

    let mut tools = Vec::new();
    if let Some(tools_arr) = body.get("tools").and_then(Value::as_array) {
        for t in tools_arr {
            if let Some(name) = t.get("name").and_then(Value::as_str) {
                let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
                let params = t
                    .get("input_schema")
                    .cloned()
                    .unwrap_or(json!({"type": "object"}));
                tools.push(CanonicalTool {
                    name: name.to_string(),
                    description: desc.to_string(),
                    parameters: params,
                });
            }
        }
    }

    let tool_choice = body.get("tool_choice").cloned();
    let temperature = body.get("temperature").and_then(Value::as_f64);
    let max_tokens = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    let thinking_budget = body
        .pointer("/thinking/budget_tokens")
        .and_then(Value::as_u64);

    let reasoning_effort = if thinking_budget.is_some() {
        Some("high".into())
    } else {
        None
    };

    Ok(CanonicalRequest {
        model,
        messages,
        system,
        tools,
        tool_choice,
        temperature,
        max_tokens,
        stream,
        reasoning_effort,
        thinking_budget,
        structured_output: None,
    })
}

fn parse_content_text(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|p| {
                let p_type = p.get("type").and_then(Value::as_str).unwrap_or("text");
                if p_type == "text" || p_type == "output_text" {
                    p.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(obj) => obj
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn parse_content_images(val: &Value) -> Vec<CanonicalImage> {
    let mut images = Vec::new();
    if let Some(arr) = val.as_array() {
        for item in arr {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            if item_type == "image_url" || item_type == "input_image" {
                if let Some(source) = item.get("image_url") {
                    if let Some(url_str) = source.as_str() {
                        images.push(CanonicalImage {
                            url: url_str.to_string(),
                            media_type: item
                                .get("media_type")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            detail: item
                                .get("detail")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        });
                    } else if let Some(url_str) = source.get("url").and_then(Value::as_str) {
                        images.push(CanonicalImage {
                            url: url_str.to_string(),
                            media_type: item
                                .get("media_type")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            detail: source
                                .get("detail")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        });
                    }
                }
            } else if item_type == "image" {
                if let Some(source) = item.get("source") {
                    let media_type = source
                        .get("media_type")
                        .and_then(Value::as_str)
                        .unwrap_or("image/jpeg");
                    if let Some(data) = source.get("data").and_then(Value::as_str) {
                        images.push(CanonicalImage {
                            url: format!("data:{media_type};base64,{data}"),
                            media_type: Some(media_type.to_string()),
                            detail: None,
                        });
                    }
                }
            }
        }
    }
    images
}

fn parse_tools(body: &Value) -> Vec<CanonicalTool> {
    let mut tools = Vec::new();
    if let Some(arr) = body.get("tools").and_then(Value::as_array) {
        for t in arr {
            let func = t.get("function").unwrap_or(t);
            if let Some(name) = func.get("name").and_then(Value::as_str) {
                let desc = func
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let params = func
                    .get("parameters")
                    .or_else(|| func.get("input_schema"))
                    .cloned()
                    .unwrap_or(json!({"type": "object"}));
                tools.push(CanonicalTool {
                    name: name.to_string(),
                    description: desc.to_string(),
                    parameters: params,
                });
            }
        }
    }
    tools
}
