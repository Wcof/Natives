//! HTTP request-body tests (extracted from `http_stream.rs`).

use super::*;
use crate::capabilities::{
    history_message_to_provider, HistoryMessage, HistoryToolCall, ProviderContentBlock,
    ProviderErrorCategory, ProviderMessage, ProviderRequest, RequestControls,
};
use chrono::Utc;

fn multi_turn_history() -> Vec<ProviderMessage> {
    vec![
        history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "read a and b".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            images: Vec::new(),
        }),
        history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![
                HistoryToolCall {
                    id: "call_a".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.txt"}"#.into(),
                },
                HistoryToolCall {
                    id: "call_b".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"b.txt"}"#.into(),
                },
            ]),
            images: Vec::new(),
        }),
        history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: "contents of a".into(),
            tool_call_id: Some("call_a".into()),
            tool_name: Some("read_file".into()),
            tool_calls: None,
            images: Vec::new(),
        }),
        history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: "contents of b".into(),
            tool_call_id: Some("call_b".into()),
            tool_name: Some("read_file".into()),
            tool_calls: None,
            images: Vec::new(),
        }),
    ]
}

#[test]
fn openai_body_preserves_multi_tool_calls_and_tool_results() {
    let body = build_chat_completions_body(&ProviderRequest {
        model: "gpt-4o".into(),
        messages: multi_turn_history(),
        system_prompt: None,
        tools: None,
        max_tokens: Some(100),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    });
    let messages = body["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 4);

    let assistant = &messages[1];
    assert_eq!(assistant["role"], "assistant");
    let tool_calls = assistant["tool_calls"].as_array().expect("tool_calls");
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0]["id"], "call_a");
    assert_eq!(tool_calls[1]["id"], "call_b");
    assert_eq!(tool_calls[0]["function"]["name"], "read_file");

    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_a");
    assert_eq!(messages[2]["content"], "contents of a");
    assert_eq!(messages[3]["tool_call_id"], "call_b");

    // Must not flatten tool structure into a single text blob.
    let wire = body.to_string();
    assert!(!wire.contains(r#""role":"assistant","content":"call_a"#));
}

fn image_message(image: crate::capabilities::ImageSource) -> ProviderMessage {
    ProviderMessage {
        role: "user".into(),
        content: vec![
            ProviderContentBlock::Text {
                text: "what is this".into(),
            },
            ProviderContentBlock::Image { image_url: image },
        ],
    }
}

fn image_request(image: crate::capabilities::ImageSource) -> ProviderRequest {
    ProviderRequest {
        model: "gpt-4o".into(),
        messages: vec![image_message(image)],
        system_prompt: None,
        tools: None,
        max_tokens: Some(256),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

#[test]
fn chat_completions_user_image_becomes_an_image_url_part() {
    let body = build_chat_completions_body(&image_request(crate::capabilities::ImageSource {
        url: "https://example.test/cat.png".into(),
        detail: Some("high".into()),
        media_type: None,
    }));
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "https://example.test/cat.png"
    );
    assert_eq!(content[1]["image_url"]["detail"], "high");
}

#[test]
fn chat_completions_data_uri_is_forwarded_verbatim() {
    let body = build_chat_completions_body(&image_request(crate::capabilities::ImageSource::new(
        "data:image/png;base64,AAAB",
    )));
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,AAAB"
    );
}

#[test]
fn chat_completions_keeps_the_plain_string_form_without_images() {
    let body = build_chat_completions_body(&ProviderRequest {
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text { text: "hi".into() }],
        }],
        ..image_request(crate::capabilities::ImageSource::new(
            "data:image/png;base64,A",
        ))
    });
    assert_eq!(body["messages"][0]["content"], "hi");
}

#[test]
fn chat_completions_unsupported_image_scheme_is_announced_not_dropped() {
    let body = build_chat_completions_body(&image_request(crate::capabilities::ImageSource::new(
        "gs://bucket/cat.png",
    )));
    let part = &body["messages"][0]["content"][1];
    assert_eq!(part["type"], "text");
    assert!(part["text"]
        .as_str()
        .unwrap()
        .contains("image not sent to the model"));
}

#[test]
fn chat_completions_tool_message_announces_an_image_it_cannot_carry() {
    // OpenAI tool messages are text-only. Silently dropping the attachment
    // is what this note replaces.
    let json = message_to_json(&ProviderMessage {
        role: "tool".into(),
        content: vec![
            ProviderContentBlock::ToolResult {
                tool_call_id: "t1".into(),
                content: "captured".into(),
                name: Some("screenshot".into()),
            },
            ProviderContentBlock::Image {
                image_url: crate::capabilities::ImageSource::new("data:image/png;base64,AAAB"),
            },
        ],
    });
    assert_eq!(json["role"], "tool");
    let content = json["content"].as_str().unwrap();
    assert!(content.starts_with("captured"), "{content}");
    assert!(content.contains("image not sent to the model"), "{content}");
}

#[test]
fn responses_user_image_becomes_an_input_image_part() {
    let body = build_responses_body(&image_request(crate::capabilities::ImageSource {
        url: "https://example.test/cat.png".into(),
        detail: Some("low".into()),
        media_type: None,
    }));
    let content = body["input"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[1]["type"], "input_image");
    assert_eq!(content[1]["image_url"], "https://example.test/cat.png");
    assert_eq!(content[1]["detail"], "low");
}

#[test]
fn responses_keeps_the_plain_string_form_without_images() {
    let body = build_responses_body(&ProviderRequest {
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text { text: "hi".into() }],
        }],
        ..image_request(crate::capabilities::ImageSource::new(
            "data:image/png;base64,A",
        ))
    });
    assert_eq!(body["input"][0]["content"], "hi");
}

#[test]
fn message_to_json_single_tool_result() {
    let msg = ProviderMessage {
        role: "tool".into(),
        content: vec![ProviderContentBlock::ToolResult {
            tool_call_id: "t1".into(),
            content: "ok".into(),
            name: Some("echo".into()),
        }],
    };
    let json = message_to_json(&msg);
    assert_eq!(json["role"], "tool");
    assert_eq!(json["tool_call_id"], "t1");
    assert_eq!(json["content"], "ok");
}

#[test]
fn retry_after_parses_seconds_http_date_and_reset_epoch() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::RETRY_AFTER, "3".parse().unwrap());
    assert_eq!(retry_after_ms(&headers), Some(3_000));

    headers.insert(
        reqwest::header::RETRY_AFTER,
        (Utc::now() + chrono::Duration::seconds(3))
            .to_rfc2822()
            .parse()
            .unwrap(),
    );
    assert!(matches!(retry_after_ms(&headers), Some(ms) if (1_000..=3_000).contains(&ms)));

    headers.remove(reqwest::header::RETRY_AFTER);
    headers.insert(
        "x-ratelimit-reset",
        (Utc::now().timestamp() + 3).to_string().parse().unwrap(),
    );
    assert!(matches!(retry_after_ms(&headers), Some(ms) if (2_000..=3_000).contains(&ms)));
}

fn plain(model: &str) -> ProviderRequest {
    ProviderRequest {
        model: model.into(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text { text: "hi".into() }],
        }],
        system_prompt: None,
        tools: None,
        max_tokens: None,
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

fn with_tool(model: &str) -> ProviderRequest {
    let mut request = plain(model);
    request.tools = Some(vec![crate::capabilities::ProviderTool {
        name: "read_file".into(),
        description: Some("read a file".into()),
        input_schema: serde_json::json!({"type": "object"}),
    }]);
    request
}

#[test]
fn chat_completions_max_tokens_comes_from_the_model_profile() {
    assert_eq!(
        build_chat_completions_body(&plain("gpt-4o"))["max_tokens"],
        16_384
    );
    // Unknown model: omit the field entirely and let the provider default
    // stand, exactly as before.
    assert!(build_chat_completions_body(&plain("some-local-llm"))
        .get("max_tokens")
        .is_none());
    // Explicit values are clamped down, never raised.
    let mut huge = plain("gpt-4o");
    huge.max_tokens = Some(999_999);
    assert_eq!(build_chat_completions_body(&huge)["max_tokens"], 16_384);
}

#[test]
fn temperature_is_dropped_for_openai_reasoning_models_only() {
    let mut gpt4o = plain("gpt-4o");
    gpt4o.temperature = Some(0.3);
    assert_eq!(build_chat_completions_body(&gpt4o)["temperature"], 0.3);

    let mut o3 = plain("o3-mini");
    o3.temperature = Some(0.3);
    assert!(build_chat_completions_body(&o3)
        .get("temperature")
        .is_none());

    // Unknown third-party models keep sampling parameters.
    let mut local = plain("qwen2.5-coder");
    local.temperature = Some(0.3);
    assert_eq!(build_chat_completions_body(&local)["temperature"], 0.3);
}

#[test]
fn tool_choice_and_parallel_flag_encode_to_the_openai_shape() {
    let request = with_tool("gpt-4o");

    let forced = build_chat_completions_body_with_controls(
        &request,
        &RequestControls {
            tool_choice: Some(crate::capabilities::ToolChoice::Tool {
                name: "read_file".into(),
            }),
            parallel_tool_calls: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(forced["tool_choice"]["type"], "function");
    assert_eq!(forced["tool_choice"]["function"]["name"], "read_file");
    assert_eq!(forced["parallel_tool_calls"], false);

    // OpenAI spells "must call something" as the bare string "required".
    let required = build_chat_completions_body_with_controls(
        &request,
        &RequestControls {
            tool_choice: Some(crate::capabilities::ToolChoice::Required),
            ..Default::default()
        },
    );
    assert_eq!(required["tool_choice"], "required");

    // Default controls change nothing on the wire.
    let body = build_chat_completions_body(&request);
    assert!(body.get("tool_choice").is_none());
    assert!(body.get("parallel_tool_calls").is_none());
}

#[test]
fn reasoning_effort_only_reaches_models_that_accept_it() {
    let controls = RequestControls {
        reasoning: Some(crate::capabilities::ReasoningRequest::new(
            crate::capabilities::ReasoningEffort::High,
        )),
        ..Default::default()
    };
    assert_eq!(
        build_chat_completions_body_with_controls(&plain("o3-mini"), &controls)["reasoning_effort"],
        "high"
    );
    // gpt-4o has no reasoning knob; sending one is a 400.
    assert!(
        build_chat_completions_body_with_controls(&plain("gpt-4o"), &controls)
            .get("reasoning_effort")
            .is_none()
    );

    // The Responses API nests it instead of using a flat field.
    let responses = build_responses_body_with_controls(&plain("o3"), &controls);
    assert_eq!(responses["reasoning"]["effort"], "high");
    assert_eq!(responses["max_output_tokens"], 100_000);
}

#[test]
fn rate_limit_error_includes_retry_after_when_available() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
    let error = map_http_status(429, "too many requests", Some(&headers));
    assert_eq!(error.category, ProviderErrorCategory::RateLimit);
    assert_eq!(error.retry_after_ms, Some(2_000));
    assert!(serde_json::to_string(&error).is_ok());
}
