//! Request/body shaping for OpenAI-compatible HTTP APIs.
//!
//! Extracted from `http_stream.rs`: chat-completions and Responses body
//! builders, provider-message serialisation, and image/tool part shaping.

use crate::capabilities::{ProviderContentBlock, ProviderMessage, ProviderRequest, ProviderTool};
use crate::controls::RequestControls;
use crate::model_profile::{self, ReasoningControl};

/// Build the JSON body for OpenAI chat completions using the request's own
/// [`ProviderRequest::controls`].
///
/// This is the entry point every streaming/non-streaming path uses, so a
/// caller only has to populate `controls` on the request it already builds.
pub fn build_chat_completions_body(request: &ProviderRequest) -> serde_json::Value {
    build_chat_completions_body_with_controls(request, &request.controls)
}

/// Build the JSON body for OpenAI chat completions with caller-supplied
/// controls, which override [`ProviderRequest::controls`] entirely.
///
/// # Prompt caching
///
/// OpenAI-compatible providers that cache do so **automatically** on the
/// longest common prefix — there is no request-side parameter to send, so
/// nothing is emitted here. Cache usage is read back from the response
/// (`prompt_tokens_details.cached_tokens` for OpenAI,
/// `prompt_cache_hit_tokens` for DeepSeek); see
/// [`crate::stream::openai_sse`]. Callers get the benefit only if they keep the
/// prefix byte-stable, which the engine already does by appending turns rather
/// than rewriting history.
pub fn build_chat_completions_body_with_controls(
    request: &ProviderRequest,
    controls: &RequestControls,
) -> serde_json::Value {
    let profile = model_profile::resolve(&request.model);
    let mut messages = Vec::new();
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }
    }
    for message in &request.messages {
        messages.push(message_to_json(message));
    }

    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream,
    });
    if let Some(max) = model_profile::resolve_max_output(request.max_tokens, &profile) {
        body["max_tokens"] = serde_json::json!(max);
    }
    if let Some(temp) = request.temperature {
        // OpenAI's reasoning models reject a non-default `temperature` with a
        // 400. Unknown models keep `sampling_params: true`, so third-party
        // endpoints are unaffected.
        if profile.sampling_params {
            body["temperature"] = serde_json::json!(temp);
        }
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools.iter().map(tool_to_json).collect::<Vec<_>>());
            if let Some(choice) = &controls.tool_choice {
                body["tool_choice"] = choice.to_openai();
            }
            if let Some(parallel) = controls.parallel_tool_calls {
                body["parallel_tool_calls"] = serde_json::json!(parallel);
            }
        }
    }
    if let Some(reasoning) = &controls.reasoning {
        if profile.reasoning == ReasoningControl::OpenAiEffort {
            body["reasoning_effort"] = serde_json::json!(reasoning.effort.as_openai_str());
        }
    }
    if request.stream {
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }
    body
}

/// Serialize one provider message to OpenAI chat-completions message JSON.
///
/// Preserves multi-tool assistant messages and `role: tool` results with `tool_call_id`.
///
/// # Images
///
/// A user message that carries images becomes the multi-part form
/// (`content: [{type:"text"}, {type:"image_url"}, …]`). Messages without images
/// keep the plain string form so the cached prefix stays byte-stable.
///
/// OpenAI's `tool` and `assistant` messages accept text only, so an image on
/// one of those is replaced by a visible note instead of being dropped.
pub fn message_to_json(message: &ProviderMessage) -> serde_json::Value {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_result: Option<(&str, &str)> = None;
    let mut images: Vec<&crate::capabilities::ImageSource> = Vec::new();

    for block in &message.content {
        match block {
            ProviderContentBlock::Text { text } => text_parts.push(text.as_str()),
            ProviderContentBlock::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                // OpenAI: one tool-result message per tool_call_id.
                tool_result = Some((tool_call_id.as_str(), content.as_str()));
            }
            ProviderContentBlock::ToolCall { id, name, input } => {
                let arguments = match input {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                tool_calls.push(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                }));
            }
            ProviderContentBlock::Image { image_url } => images.push(image_url),
        }
    }

    if let Some((tool_call_id, content)) = tool_result {
        let mut body = content.to_string();
        append_image_notes(&mut body, &images, "OpenAI tool messages carry text only");
        return serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": body,
        });
    }

    if !tool_calls.is_empty() {
        let mut body = text_parts.join("\n");
        append_image_notes(
            &mut body,
            &images,
            "OpenAI assistant messages carry text only",
        );
        let content = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(body)
        };
        return serde_json::json!({
            "role": "assistant",
            "content": content,
            "tool_calls": tool_calls,
        });
    }

    if images.is_empty() {
        return serde_json::json!({
            "role": message.role,
            "content": text_parts.join("\n"),
        });
    }

    let mut parts = Vec::new();
    let text = text_parts.join("\n");
    if !text.is_empty() {
        parts.push(serde_json::json!({ "type": "text", "text": text }));
    }
    for image in images {
        parts.push(chat_image_part(image));
    }
    serde_json::json!({
        "role": message.role,
        "content": parts,
    })
}

/// Whether OpenAI's `image_url.url` can carry this reference verbatim.
///
/// The API fetches `http(s)` URLs and decodes `data:` URIs; nothing else.
fn openai_accepts_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || url.starts_with("data:")
}

/// One `image_url` content part, or a visible note when the URL is unusable.
fn chat_image_part(image: &crate::capabilities::ImageSource) -> serde_json::Value {
    if !openai_accepts_url(&image.url) {
        return serde_json::json!({
            "type": "text",
            "text": image.degraded_note("OpenAI accepts only an http(s) URL or a data: URI"),
        });
    }
    let mut image_url = serde_json::json!({ "url": image.url });
    if let Some(detail) = &image.detail {
        image_url["detail"] = serde_json::json!(detail);
    }
    serde_json::json!({ "type": "image_url", "image_url": image_url })
}

/// One Responses `input_image` part, or a visible note when the URL is unusable.
fn responses_image_part(image: &crate::capabilities::ImageSource) -> serde_json::Value {
    if !openai_accepts_url(&image.url) {
        return serde_json::json!({
            "type": "input_text",
            "text": image.degraded_note("OpenAI accepts only an http(s) URL or a data: URI"),
        });
    }
    let mut part = serde_json::json!({ "type": "input_image", "image_url": image.url });
    if let Some(detail) = &image.detail {
        part["detail"] = serde_json::json!(detail);
    }
    part
}

/// Append a degradation note per image to a text-only message body.
///
/// Used where the wire format has no image slot at all; the alternative would be
/// dropping the attachment without telling anyone.
fn append_image_notes(
    body: &mut String,
    images: &[&crate::capabilities::ImageSource],
    reason: &str,
) {
    for image in images {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&image.degraded_note(reason));
    }
}

fn tool_to_json(tool: &ProviderTool) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}
/// Build JSON body for OpenAI Responses API using the request's own
/// [`ProviderRequest::controls`].
pub fn build_responses_body(request: &ProviderRequest) -> serde_json::Value {
    build_responses_body_with_controls(request, &request.controls)
}

/// Build JSON body for OpenAI Responses API with caller-supplied controls,
/// which override [`ProviderRequest::controls`] entirely.
pub fn build_responses_body_with_controls(
    request: &ProviderRequest,
    controls: &RequestControls,
) -> serde_json::Value {
    let profile = model_profile::resolve(&request.model);
    let mut input = Vec::new();
    for message in &request.messages {
        // Responses API input is looser than chat completions; still forward tool structure
        // as role+content text plus function_call / function_call_output items when present.
        let mut text_parts = Vec::new();
        let mut pushed_structured = false;
        let mut images: Vec<&crate::capabilities::ImageSource> = Vec::new();
        for block in &message.content {
            match block {
                ProviderContentBlock::Text { text } => text_parts.push(text.as_str()),
                ProviderContentBlock::ToolCall {
                    id,
                    name,
                    input: tool_input,
                } => {
                    pushed_structured = true;
                    let arguments = match tool_input {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    input.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": arguments,
                    }));
                }
                ProviderContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    pushed_structured = true;
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": tool_call_id,
                        "output": content,
                    }));
                }
                ProviderContentBlock::Image { image_url } => images.push(image_url),
            }
        }
        if !images.is_empty() {
            // Responses input items take a typed content array; only switch to
            // it when there is an image, so text-only turns keep the cheap
            // string form (and its byte-stable cache prefix).
            let mut parts = Vec::new();
            let text = text_parts.join("\n");
            if !text.is_empty() {
                parts.push(serde_json::json!({ "type": "input_text", "text": text }));
            }
            for image in images {
                parts.push(responses_image_part(image));
            }
            input.push(serde_json::json!({
                "role": message.role,
                "content": parts,
            }));
        } else if !text_parts.is_empty() || !pushed_structured {
            input.push(serde_json::json!({
                "role": message.role,
                "content": text_parts.join("\n"),
            }));
        }
    }
    let mut body = serde_json::json!({
        "model": request.model,
        "input": input,
        "stream": true,
    });
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            body["instructions"] = serde_json::json!(system);
        }
    }
    if let Some(max) = model_profile::resolve_max_output(request.max_tokens, &profile) {
        body["max_output_tokens"] = serde_json::json!(max);
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools
                .iter()
                .map(|t| serde_json::json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }))
                .collect::<Vec<_>>());
            if let Some(choice) = &controls.tool_choice {
                body["tool_choice"] = choice.to_openai();
            }
            if let Some(parallel) = controls.parallel_tool_calls {
                body["parallel_tool_calls"] = serde_json::json!(parallel);
            }
        }
    }
    if let Some(reasoning) = &controls.reasoning {
        if profile.reasoning == ReasoningControl::OpenAiEffort {
            // Responses nests the level under `reasoning`, unlike chat
            // completions' flat `reasoning_effort`.
            body["reasoning"] = serde_json::json!({
                "effort": reasoning.effort.as_openai_str(),
            });
        }
    }
    body
}
