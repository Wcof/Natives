//! Byte-for-byte regression guard for the four request-body builders, plus
//! proof that [`ProviderRequest::controls`] actually reaches the wire.
//!
//! `RequestControls` now rides inside `ProviderRequest`. The whole point of its
//! `Default` being "today's behaviour" is that a caller who sets nothing keeps
//! producing the exact same bytes. The `GOLDEN_*` constants below were captured
//! from the builders **before** `ProviderRequest::controls` existed; if one of
//! them has to change, the wire format moved and that is a decision, not a
//! detail.

use provider_adapters::capabilities::{
    ImageSource, ProviderContentBlock, ProviderMessage, ProviderRequest, ProviderTool,
    ReasoningEffort, ReasoningRequest, RequestControls, ToolChoice,
};
use provider_adapters::http_stream::{build_chat_completions_body, build_responses_body};
use provider_adapters::providers::anthropic::build_messages_body;
use provider_adapters::providers::gemini::build_generate_body;

/// One request exercising every branch the builders care about: system prompt,
/// tools, a plain user turn, an assistant tool call, a tool result, an image.
fn fixture(model: &str) -> ProviderRequest {
    ProviderRequest {
        model: model.to_string(),
        messages: vec![
            ProviderMessage {
                role: "user".into(),
                content: vec![
                    ProviderContentBlock::Text {
                        text: "read a.txt then describe this".into(),
                    },
                    ProviderContentBlock::Image {
                        image_url: ImageSource::new("data:image/png;base64,AAAB"),
                    },
                ],
            },
            ProviderMessage {
                role: "assistant".into(),
                content: vec![
                    ProviderContentBlock::Text {
                        text: "calling".into(),
                    },
                    ProviderContentBlock::ToolCall {
                        id: "call_1".into(),
                        name: "read_file".into(),
                        input: serde_json::json!({ "path": "a.txt" }),
                    },
                ],
            },
            ProviderMessage {
                role: "tool".into(),
                content: vec![ProviderContentBlock::ToolResult {
                    tool_call_id: "call_1".into(),
                    content: r#"{"ok":true}"#.into(),
                    name: Some("read_file".into()),
                }],
            },
            ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text {
                    text: "thanks".into(),
                }],
            },
        ],
        system_prompt: Some("You are a careful assistant.".into()),
        tools: Some(vec![
            ProviderTool {
                name: "read_file".into(),
                description: Some("Read a file".into()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                }),
            },
            ProviderTool {
                name: "write_file".into(),
                description: Some("Write a file".into()),
                input_schema: serde_json::json!({ "type": "object" }),
            },
        ]),
        max_tokens: Some(2048),
        temperature: Some(0.3),
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

fn with_controls(model: &str, controls: RequestControls) -> ProviderRequest {
    ProviderRequest {
        controls,
        ..fixture(model)
    }
}

const GOLDEN_CHAT: &str = r#"{"max_tokens":2048,"messages":[{"content":"You are a careful assistant.","role":"system"},{"content":[{"text":"read a.txt then describe this","type":"text"},{"image_url":{"url":"data:image/png;base64,AAAB"},"type":"image_url"}],"role":"user"},{"content":"calling","role":"assistant","tool_calls":[{"function":{"arguments":"{\"path\":\"a.txt\"}","name":"read_file"},"id":"call_1","type":"function"}]},{"content":"{\"ok\":true}","role":"tool","tool_call_id":"call_1"},{"content":"thanks","role":"user"}],"model":"gpt-4o","stream":true,"stream_options":{"include_usage":true},"temperature":0.3,"tools":[{"function":{"description":"Read a file","name":"read_file","parameters":{"properties":{"path":{"type":"string"}},"type":"object"}},"type":"function"},{"function":{"description":"Write a file","name":"write_file","parameters":{"type":"object"}},"type":"function"}]}"#;

const GOLDEN_RESPONSES: &str = r#"{"input":[{"content":[{"text":"read a.txt then describe this","type":"input_text"},{"image_url":"data:image/png;base64,AAAB","type":"input_image"}],"role":"user"},{"arguments":"{\"path\":\"a.txt\"}","call_id":"call_1","name":"read_file","type":"function_call"},{"content":"calling","role":"assistant"},{"call_id":"call_1","output":"{\"ok\":true}","type":"function_call_output"},{"content":"thanks","role":"user"}],"instructions":"You are a careful assistant.","max_output_tokens":2048,"model":"o3","stream":true,"tools":[{"description":"Read a file","name":"read_file","parameters":{"properties":{"path":{"type":"string"}},"type":"object"},"type":"function"},{"description":"Write a file","name":"write_file","parameters":{"type":"object"},"type":"function"}]}"#;

const GOLDEN_ANTHROPIC: &str = r#"{"max_tokens":2048,"messages":[{"content":[{"text":"read a.txt then describe this","type":"text"},{"source":{"data":"AAAB","media_type":"image/png","type":"base64"},"type":"image"}],"role":"user"},{"content":[{"text":"calling","type":"text"},{"id":"call_1","input":{"path":"a.txt"},"name":"read_file","type":"tool_use"}],"role":"assistant"},{"content":[{"cache_control":{"type":"ephemeral"},"content":"{\"ok\":true}","tool_use_id":"call_1","type":"tool_result"}],"role":"user"},{"content":[{"text":"thanks","type":"text"}],"role":"user"}],"model":"claude-sonnet-4-5","stream":true,"system":"You are a careful assistant.","temperature":0.3,"tools":[{"description":"Read a file","input_schema":{"properties":{"path":{"type":"string"}},"type":"object"},"name":"read_file"},{"cache_control":{"type":"ephemeral"},"description":"Write a file","input_schema":{"type":"object"},"name":"write_file"}]}"#;

const GOLDEN_GEMINI: &str = r#"{"contents":[{"parts":[{"text":"read a.txt then describe this"},{"inlineData":{"data":"AAAB","mimeType":"image/png"}}],"role":"user"},{"parts":[{"text":"calling"},{"functionCall":{"args":{"path":"a.txt"},"name":"read_file"}}],"role":"model"},{"parts":[{"functionResponse":{"name":"read_file","response":{"ok":true}}}],"role":"user"},{"parts":[{"text":"thanks"}],"role":"user"}],"generationConfig":{"maxOutputTokens":2048,"temperature":0.3},"systemInstruction":{"parts":[{"text":"You are a careful assistant."}]},"tools":[{"functionDeclarations":[{"description":"Read a file","name":"read_file","parameters":{"properties":{"path":{"type":"string"}},"type":"object"}},{"description":"Write a file","name":"write_file","parameters":{"type":"object"}}]}]}"#;

fn wire(body: &serde_json::Value) -> String {
    serde_json::to_string(body).expect("body serializes")
}

#[test]
fn default_controls_produce_the_pre_existing_bytes() {
    assert_eq!(wire(&build_chat_completions_body(&fixture("gpt-4o"))), GOLDEN_CHAT);
    assert_eq!(wire(&build_responses_body(&fixture("o3"))), GOLDEN_RESPONSES);
    assert_eq!(
        wire(&build_messages_body(&fixture("claude-sonnet-4-5"))),
        GOLDEN_ANTHROPIC
    );
    assert_eq!(
        wire(&build_generate_body(&fixture("gemini-2.5-pro"))),
        GOLDEN_GEMINI
    );
}

/// The struct grew a field; serializing a default-controls request must not
/// grow the JSON, or anything that persists a `ProviderRequest` changes shape.
#[test]
fn default_controls_are_skipped_when_serializing_the_request() {
    let json = serde_json::to_value(fixture("gpt-4o")).expect("request serializes");
    assert!(
        json.get("controls").is_none(),
        "default controls must not appear on the wire: {json}"
    );

    let mut set = fixture("gpt-4o");
    set.controls.tool_choice = Some(ToolChoice::Required);
    let json = serde_json::to_value(&set).expect("request serializes");
    assert_eq!(json["controls"]["tool_choice"]["type"], "required");

    // Round-trips, and an older payload without the field still deserializes.
    let mut legacy = serde_json::to_value(fixture("gpt-4o")).expect("request serializes");
    legacy.as_object_mut().unwrap().remove("controls");
    let back: ProviderRequest = serde_json::from_value(legacy).expect("legacy payload loads");
    assert!(back.controls.is_default());
}

#[test]
fn tool_choice_on_the_request_reaches_every_wire_format() {
    let controls = RequestControls {
        tool_choice: Some(ToolChoice::Tool {
            name: "read_file".into(),
        }),
        ..Default::default()
    };

    let chat = build_chat_completions_body(&with_controls("gpt-4o", controls.clone()));
    assert_eq!(chat["tool_choice"]["function"]["name"], "read_file");

    let responses = build_responses_body(&with_controls("o3", controls.clone()));
    assert_eq!(responses["tool_choice"]["function"]["name"], "read_file");

    let anthropic = build_messages_body(&with_controls("claude-sonnet-4-5", controls.clone()));
    assert_eq!(anthropic["tool_choice"]["type"], "tool");
    assert_eq!(anthropic["tool_choice"]["name"], "read_file");

    let gemini = build_generate_body(&with_controls("gemini-2.5-pro", controls));
    assert_eq!(
        gemini["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
        "read_file"
    );
}

#[test]
fn serial_tool_calls_on_the_request_reach_the_wire() {
    let controls = RequestControls {
        parallel_tool_calls: Some(false),
        ..Default::default()
    };

    let chat = build_chat_completions_body(&with_controls("gpt-4o", controls.clone()));
    assert_eq!(chat["parallel_tool_calls"], false);

    // Anthropic carries it on the tool_choice object, so it needs one.
    let anthropic = build_messages_body(&with_controls(
        "claude-sonnet-4-5",
        RequestControls {
            tool_choice: Some(ToolChoice::Auto),
            ..controls
        },
    ));
    assert_eq!(anthropic["tool_choice"]["disable_parallel_tool_use"], true);
}

#[test]
fn reasoning_on_the_request_reaches_every_wire_format() {
    let controls = RequestControls {
        reasoning: Some(ReasoningRequest::new(ReasoningEffort::High)),
        ..Default::default()
    };

    let chat = build_chat_completions_body(&with_controls("o3-mini", controls.clone()));
    assert_eq!(chat["reasoning_effort"], "high");

    let responses = build_responses_body(&with_controls("o3", controls.clone()));
    assert_eq!(responses["reasoning"]["effort"], "high");

    let anthropic = build_messages_body(&with_controls("claude-sonnet-4-5", controls.clone()));
    assert_eq!(anthropic["thinking"]["type"], "enabled");
    // The effort default (32768) is clamped below the fixture's 2048-token
    // output ceiling, which Anthropic requires.
    assert_eq!(anthropic["thinking"]["budget_tokens"], 2047);
    // `thinking` and `temperature` are mutually exclusive upstream.
    assert!(anthropic.get("temperature").is_none());

    let gemini = build_generate_body(&with_controls("gemini-2.5-pro", controls));
    assert_eq!(
        gemini["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        ReasoningEffort::High.default_budget_tokens()
    );

    // A non-reasoning model must not grow a reasoning parameter.
    let plain = build_chat_completions_body(&with_controls(
        "gpt-4o",
        RequestControls {
            reasoning: Some(ReasoningRequest::new(ReasoningEffort::High)),
            ..Default::default()
        },
    ));
    assert!(plain.get("reasoning_effort").is_none());
}

#[test]
fn prompt_cache_opt_out_on_the_request_removes_every_breakpoint() {
    let body = build_messages_body(&with_controls(
        "claude-sonnet-4-5",
        RequestControls {
            prompt_cache: Some(false),
            ..Default::default()
        },
    ));
    assert!(
        !wire(&body).contains("cache_control"),
        "opting out must strip every breakpoint: {}",
        wire(&body)
    );
    // Everything else is untouched — only the breakpoints differ from golden.
    assert_eq!(
        wire(&body),
        GOLDEN_ANTHROPIC
            .replace(r#"{"cache_control":{"type":"ephemeral"},"content""#, r#"{"content""#)
            .replace(
                r#"{"cache_control":{"type":"ephemeral"},"description":"Write a file""#,
                r#"{"description":"Write a file""#
            )
    );
}
