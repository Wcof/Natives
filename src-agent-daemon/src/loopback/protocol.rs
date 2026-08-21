use agent_core::{EngineImage, EngineMessage, EngineToolCall, ToolSchema};
use provider_adapters::{ReasoningEffort, ReasoningRequest, RequestControls};
use serde_json::{json, Value};

pub(super) struct ParsedRequest {
    pub messages: Vec<EngineMessage>,
    pub tools: Vec<ToolSchema>,
    pub system: Option<String>,
    pub controls: RequestControls,
}

pub(super) fn parse_request(path: &str, body: &Value) -> Result<ParsedRequest, String> {
    Ok(ParsedRequest {
        messages: protocol_messages(path, body)?,
        tools: protocol_tools(body),
        system: protocol_system(path, body),
        controls: protocol_controls(body),
    })
}

fn protocol_messages(path: &str, body: &Value) -> Result<Vec<EngineMessage>, String> {
    let source = message_source(path, body);
    match source {
        Value::String(text) => Ok(vec![EngineMessage::text("user", text)]),
        Value::Array(items) => items
            .into_iter()
            .filter(|item| {
                !matches!(
                    item.get("role").and_then(Value::as_str),
                    Some("system" | "developer")
                )
            })
            .map(protocol_message)
            .collect(),
        _ => Err("messages/input must be a string or array".into()),
    }
}

fn message_source(path: &str, body: &Value) -> Value {
    if path == "/v1/responses" {
        body.get("input").cloned().unwrap_or(Value::Array(vec![]))
    } else {
        body.get("messages")
            .cloned()
            .unwrap_or(Value::Array(vec![]))
    }
}

fn protocol_message(item: Value) -> Result<EngineMessage, String> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
    let content = item.get("content").unwrap_or(&item);
    let tool_calls = item
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter_map(history_tool_call)
                .collect::<Vec<_>>()
        })
        .filter(|calls| !calls.is_empty());
    let tool_call_id = item
        .get("tool_call_id")
        .or_else(|| item.get("tool_use_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(EngineMessage {
        role: role.to_string(),
        content: content_text(content),
        tool_call_id,
        tool_name: item.get("name").and_then(Value::as_str).map(str::to_string),
        tool_calls,
        images: content_images(content),
    })
}

fn history_tool_call(call: &Value) -> Option<EngineToolCall> {
    let function = call.get("function").unwrap_or(call);
    Some(EngineToolCall {
        id: call.get("id")?.as_str()?.to_string(),
        name: function.get("name")?.as_str()?.to_string(),
        arguments: function
            .get("arguments")
            .map(json_argument)
            .unwrap_or_else(|| "{}".into()),
    })
}

fn protocol_tools(body: &Value) -> Vec<ToolSchema> {
    body.get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(ToolSchema {
                name: name.to_string(),
                description: function
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                input_schema: function
                    .get("parameters")
                    .or_else(|| function.get("input_schema"))
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
            })
        })
        .collect()
}

fn protocol_system(path: &str, body: &Value) -> Option<String> {
    let direct = if path == "/v1/responses" {
        body.get("instructions").or_else(|| body.get("system"))
    } else {
        body.get("system")
    };
    let mut parts = direct
        .map(content_text)
        .filter(|text| !text.trim().is_empty())
        .into_iter()
        .collect::<Vec<_>>();
    if let Value::Array(messages) = message_source(path, body) {
        parts.extend(messages.iter().filter_map(|message| {
            matches!(
                message.get("role").and_then(Value::as_str),
                Some("system" | "developer")
            )
            .then(|| content_text(message.get("content").unwrap_or(message)))
            .filter(|text| !text.trim().is_empty())
        }));
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn protocol_controls(body: &Value) -> RequestControls {
    let effort = body
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .or_else(|| body.pointer("/reasoning/effort").and_then(Value::as_str));
    let mut controls = RequestControls::default().with_effort_str(effort);
    let budget = body
        .pointer("/thinking/budget_tokens")
        .or_else(|| body.pointer("/reasoning/budget_tokens"))
        .and_then(Value::as_u64);
    if let Some(budget_tokens) = budget {
        let effort = controls
            .reasoning
            .as_ref()
            .map(|reasoning| reasoning.effort)
            .unwrap_or_else(|| effort_for_budget(budget_tokens));
        controls.reasoning = Some(ReasoningRequest {
            effort,
            budget_tokens: Some(budget_tokens),
        });
    }
    controls
}

fn effort_for_budget(budget: u64) -> ReasoningEffort {
    if budget <= 8_192 {
        ReasoningEffort::Low
    } else if budget <= 24_576 {
        ReasoningEffort::Medium
    } else {
        ReasoningEffort::High
    }
}

fn json_argument(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn content_images(value: &Value) -> Vec<EngineImage> {
    let Value::Array(parts) = value else {
        return Vec::new();
    };
    parts
        .iter()
        .filter_map(|part| {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
            if kind != "image_url" && kind != "input_image" {
                return None;
            }
            let source = part.get("image_url")?;
            let (url, detail) = match source {
                Value::String(url) => (url.as_str(), None),
                other => (
                    other.get("url").and_then(Value::as_str)?,
                    other.get("detail").and_then(Value::as_str),
                ),
            };
            if url.trim().is_empty() {
                return None;
            }
            Some(EngineImage {
                url: url.to_string(),
                media_type: part
                    .get("media_type")
                    .or_else(|| part.get("mime_type"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                detail: detail.map(str::to_string),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tools_system_and_reasoning_without_leaving_system_in_history() {
        let parsed = parse_request(
            "/v1/chat/completions",
            &json!({
                "messages": [
                    {"role": "system", "content": "Be exact."},
                    {"role": "user", "content": "Read it."}
                ],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "description": "Read a file",
                        "parameters": {"type": "object", "required": ["path"]}
                    }
                }],
                "reasoning_effort": "high"
            }),
        )
        .unwrap();

        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.system.as_deref(), Some("Be exact."));
        assert_eq!(parsed.tools[0].name, "read_file");
        assert_eq!(
            parsed.controls.reasoning.as_ref().map(|value| value.effort),
            Some(ReasoningEffort::High)
        );
    }

    #[test]
    fn anthropic_budget_and_tool_schema_reach_canonical_request() {
        let parsed = parse_request(
            "/v1/messages",
            &json!({
                "system": [{"type": "text", "text": "Be exact."}],
                "messages": [{"role": "user", "content": "Read it."}],
                "tools": [{
                    "name": "read_file",
                    "description": "Read a file",
                    "input_schema": {"type": "object"}
                }],
                "thinking": {"type": "enabled", "budget_tokens": 32000}
            }),
        )
        .unwrap();

        assert_eq!(parsed.system.as_deref(), Some("Be exact."));
        assert_eq!(parsed.tools[0].input_schema["type"], "object");
        let reasoning = parsed.controls.reasoning.unwrap();
        assert_eq!(reasoning.effort, ReasoningEffort::High);
        assert_eq!(reasoning.budget_tokens, Some(32_000));
    }

    #[test]
    fn chat_completions_image_parts_reach_the_engine() {
        let parsed = parse_request(
            "/v1/chat/completions",
            &json!({"messages":[{"role":"user","content":[
                {"type":"text","text":"what is this"},
                {"type":"image_url","image_url":{
                    "url":"data:image/png;base64,iVBORw0KGgo=",
                    "detail":"high"
                }}
            ]}]}),
        )
        .unwrap();
        assert_eq!(parsed.messages[0].content, "what is this");
        assert_eq!(parsed.messages[0].images.len(), 1);
        assert_eq!(
            parsed.messages[0].images[0].url,
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(parsed.messages[0].images[0].detail.as_deref(), Some("high"));
    }

    #[test]
    fn responses_input_image_reaches_the_engine() {
        let parsed = parse_request(
            "/v1/responses",
            &json!({"input":[{"role":"user","content":[
                {"type":"input_image","image_url":"https://example.test/a.png"}
            ]}]}),
        )
        .unwrap();
        assert_eq!(parsed.messages[0].images.len(), 1);
        assert_eq!(
            parsed.messages[0].images[0].url,
            "https://example.test/a.png"
        );
    }

    #[test]
    fn image_parts_without_a_url_are_skipped() {
        let parsed = parse_request(
            "/v1/chat/completions",
            &json!({"messages":[{"role":"user","content":[
                {"type":"text","text":"hi"},
                {"type":"image_url","image_url":{"url":"   "}},
                {"type":"image_url"}
            ]}]}),
        )
        .unwrap();
        assert!(parsed.messages[0].images.is_empty());
        assert_eq!(parsed.messages[0].content, "hi");
    }

    #[test]
    fn response_instructions_become_system_prompt() {
        let parsed = parse_request(
            "/v1/responses",
            &json!({"instructions":"Be exact.","input":"Hello"}),
        )
        .unwrap();
        assert_eq!(parsed.system.as_deref(), Some("Be exact."));
    }
}
