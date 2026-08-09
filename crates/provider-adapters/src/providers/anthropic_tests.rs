//! Anthropic adapter request/response tests (extracted from `anthropic.rs`).

use super::*;

use super::*;

fn tool(name: &str) -> ProviderTool {
    ProviderTool {
        name: name.into(),
        description: Some(format!("{name} description")),
        input_schema: serde_json::json!({"type": "object"}),
    }
}

fn text(role: &str, body: &str) -> ProviderMessage {
    ProviderMessage {
        role: role.into(),
        content: vec![ProviderContentBlock::Text { text: body.into() }],
    }
}

fn request(model: &str) -> ProviderRequest {
    ProviderRequest {
        model: model.into(),
        messages: vec![text("user", "hi")],
        system_prompt: None,
        tools: None,
        max_tokens: None,
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

/// Long enough to clear every model's minimum cacheable prefix, since one
/// token is never fewer than one character.
fn long_system() -> String {
    "You are a careful engineer. ".repeat(400)
}

#[test]
fn caches_system_and_tools_with_ephemeral_breakpoints() {
    let mut req = request("claude-sonnet-4-5");
    req.system_prompt = Some(long_system());
    req.tools = Some(vec![tool("read_file"), tool("write_file")]);
    let body = build_messages_body(&req);

    // System becomes a block array carrying the breakpoint.
    let system = body["system"].as_array().expect("system block array");
    assert_eq!(system.len(), 1);
    assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    assert_eq!(system[0]["type"], "text");

    // Only the *last* tool is marked — one breakpoint covers all tools.
    let tools = body["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 2);
    assert!(tools[0].get("cache_control").is_none());
    assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
}

#[test]
fn never_exceeds_the_four_breakpoint_limit() {
    let mut req = request("claude-sonnet-4-5");
    req.system_prompt = Some(long_system());
    req.tools = Some(vec![tool("a"), tool("b")]);
    req.messages = (0..40)
        .map(|i| {
            text(
                if i % 2 == 0 { "user" } else { "assistant" },
                &format!("turn {i}"),
            )
        })
        .collect();
    let body = build_messages_body(&req);
    let marks = body.to_string().matches("cache_control").count();
    assert!(
        marks <= MAX_CACHE_BREAKPOINTS,
        "emitted {marks} breakpoints, Anthropic accepts at most {MAX_CACHE_BREAKPOINTS}"
    );
    assert_eq!(marks, 3, "tools + system + one stable history boundary");
}

#[test]
fn history_breakpoint_lands_on_the_stable_boundary_not_the_last_turn() {
    let mut req = request("claude-sonnet-4-5");
    req.messages = vec![
        text("user", "first"),
        text("assistant", "second"),
        text("user", "third"),
    ];
    let body = build_messages_body(&req);
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert!(messages[0]["content"][0].get("cache_control").is_none());
    // Second-to-last: the prefix the next request will extend.
    assert_eq!(
        messages[1]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );
    // The final turn is left unmarked on purpose.
    assert!(messages[2]["content"][0].get("cache_control").is_none());
}

#[test]
fn short_conversation_gets_no_history_breakpoint() {
    let body = build_messages_body(&request("claude-sonnet-4-5"));
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0]["content"][0].get("cache_control").is_none());
}

#[test]
fn short_system_prompt_does_not_burn_a_breakpoint() {
    let mut req = request("claude-opus-4-6"); // 4096-token minimum
    req.system_prompt = Some("Be brief.".into());
    let body = build_messages_body(&req);
    // Falls back to the plain-string form: provably below the minimum, so
    // marking it would consume a slot for a guaranteed no-op.
    assert!(body["system"].is_string());
}

#[test]
fn caching_can_be_disabled_per_request() {
    let mut req = request("claude-sonnet-4-5");
    req.system_prompt = Some(long_system());
    req.tools = Some(vec![tool("read_file")]);
    req.messages = vec![text("user", "a"), text("assistant", "b"), text("user", "c")];
    let controls = RequestControls {
        prompt_cache: Some(false),
        ..Default::default()
    };
    let body = build_messages_body_with_controls(&req, &controls);
    assert!(!body.to_string().contains("cache_control"));
    assert!(body["system"].is_string());
}

#[test]
fn resolves_max_tokens_from_the_model_instead_of_a_hardcoded_4096() {
    // Known model, no caller value: use the real ceiling.
    assert_eq!(
        build_messages_body(&request("claude-sonnet-4-5"))["max_tokens"],
        64_000
    );
    assert_eq!(
        build_messages_body(&request("claude-opus-5"))["max_tokens"],
        128_000
    );
    // Unknown model: unchanged historical fallback, no invented ceiling.
    assert_eq!(
        build_messages_body(&request("claude-unreleased-99"))["max_tokens"],
        4096
    );

    // An explicit caller value is honoured and clamped, never raised.
    let mut small = request("claude-sonnet-4-5");
    small.max_tokens = Some(256);
    assert_eq!(build_messages_body(&small)["max_tokens"], 256);
    let mut huge = request("claude-sonnet-4-5");
    huge.max_tokens = Some(10_000_000);
    assert_eq!(build_messages_body(&huge)["max_tokens"], 64_000);
}

#[test]
fn thinking_uses_the_dialect_the_model_accepts() {
    let controls = RequestControls {
        reasoning: Some(ReasoningRequest::new(ReasoningEffort::High)),
        ..Default::default()
    };

    // Claude 4.5 and older: explicit token budget.
    let budgeted = build_messages_body_with_controls(&request("claude-sonnet-4-5"), &controls);
    assert_eq!(budgeted["thinking"]["type"], "enabled");
    assert_eq!(budgeted["thinking"]["budget_tokens"], 32_768);

    // Claude 4.6+: `budget_tokens` is a 400 there, so it must be absent.
    let adaptive = build_messages_body_with_controls(&request("claude-opus-5"), &controls);
    assert_eq!(adaptive["thinking"]["type"], "adaptive");
    assert!(adaptive["thinking"].get("budget_tokens").is_none());

    // No reasoning requested: no `thinking` key at all.
    assert!(build_messages_body(&request("claude-sonnet-4-5"))
        .get("thinking")
        .is_none());
}

#[test]
fn thinking_budget_stays_below_max_tokens() {
    let mut req = request("claude-sonnet-4-5");
    req.max_tokens = Some(2_000);
    let controls = RequestControls {
        reasoning: Some(ReasoningRequest::new(ReasoningEffort::High)),
        ..Default::default()
    };
    let body = build_messages_body_with_controls(&req, &controls);
    let budget = body["thinking"]["budget_tokens"].as_u64().unwrap();
    assert!(
        budget < 2_000,
        "budget {budget} must stay under max_tokens or Anthropic returns 400"
    );
    assert!(budget >= MIN_THINKING_BUDGET);
}

#[test]
fn temperature_is_forwarded_only_where_the_model_accepts_it() {
    let mut older = request("claude-sonnet-4-5");
    older.temperature = Some(0.2);
    assert_eq!(build_messages_body(&older)["temperature"], 0.2);

    // Opus 5 rejects sampling parameters outright.
    let mut newer = request("claude-opus-5");
    newer.temperature = Some(0.2);
    assert!(build_messages_body(&newer).get("temperature").is_none());

    // Thinking and temperature are mutually exclusive.
    let controls = RequestControls {
        reasoning: Some(ReasoningRequest::new(ReasoningEffort::Low)),
        ..Default::default()
    };
    let body = build_messages_body_with_controls(&older, &controls);
    assert!(body.get("temperature").is_none());
}

#[test]
fn tool_choice_and_parallelism_encode_to_the_anthropic_shape() {
    let mut req = request("claude-sonnet-4-5");
    req.tools = Some(vec![tool("read_file")]);

    let forced = build_messages_body_with_controls(
        &req,
        &RequestControls {
            tool_choice: Some(ToolChoice::Tool {
                name: "read_file".into(),
            }),
            ..Default::default()
        },
    );
    assert_eq!(forced["tool_choice"]["type"], "tool");
    assert_eq!(forced["tool_choice"]["name"], "read_file");

    // `Required` is spelled `any` on Anthropic.
    let required = build_messages_body_with_controls(
        &req,
        &RequestControls {
            tool_choice: Some(ToolChoice::Required),
            ..Default::default()
        },
    );
    assert_eq!(required["tool_choice"]["type"], "any");

    // Parallelism rides inside tool_choice, so a bare override still needs
    // an object.
    let serial = build_messages_body_with_controls(
        &req,
        &RequestControls {
            parallel_tool_calls: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(serial["tool_choice"]["type"], "auto");
    assert_eq!(serial["tool_choice"]["disable_parallel_tool_use"], true);

    // Nothing requested: no key, so the provider default applies.
    assert!(build_messages_body(&req).get("tool_choice").is_none());
}

#[test]
fn tool_choice_is_omitted_when_no_tools_are_present() {
    let body = build_messages_body_with_controls(
        &request("claude-sonnet-4-5"),
        &RequestControls {
            tool_choice: Some(ToolChoice::Required),
            ..Default::default()
        },
    );
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn anthropic_body_uses_tool_use_and_tool_result_blocks() {
    let body = build_messages_body(&ProviderRequest {
        model: "claude-sonnet-4".into(),
        messages: vec![
            ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text {
                    text: "read it".into(),
                }],
            },
            ProviderMessage {
                role: "assistant".into(),
                content: vec![ProviderContentBlock::ToolCall {
                    id: "toolu_1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "x"}),
                }],
            },
            ProviderMessage {
                role: "tool".into(),
                content: vec![ProviderContentBlock::ToolResult {
                    tool_call_id: "toolu_1".into(),
                    content: "file data".into(),
                    name: Some("read_file".into()),
                }],
            },
        ],
        system_prompt: Some("sys".into()),
        tools: None,
        max_tokens: Some(256),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    });

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][0]["id"], "toolu_1");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(messages[2]["content"][0]["content"], "file data");
}

fn image_request(image: ImageSource) -> ProviderRequest {
    ProviderRequest {
        model: "claude-sonnet-4-5".into(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![
                ProviderContentBlock::Text {
                    text: "what is this".into(),
                },
                ProviderContentBlock::Image { image_url: image },
            ],
        }],
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
fn data_uri_becomes_a_base64_image_source() {
    let body = build_messages_body(&image_request(ImageSource::new(
        "data:image/png;base64,AAAB",
    )));
    let block = &body["messages"][0]["content"][1];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "base64");
    assert_eq!(block["source"]["media_type"], "image/png");
    assert_eq!(block["source"]["data"], "AAAB");
}

#[test]
fn https_url_becomes_a_url_image_source() {
    let body = build_messages_body(&image_request(ImageSource::new(
        "https://example.test/cat.png",
    )));
    let block = &body["messages"][0]["content"][1];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "url");
    assert_eq!(block["source"]["url"], "https://example.test/cat.png");
}

#[test]
fn unsupported_image_reference_is_announced_not_dropped() {
    let body = build_messages_body(&image_request(ImageSource::new("gs://bucket/cat.png")));
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2, "the block must survive as something");
    assert_eq!(content[1]["type"], "text");
    let note = content[1]["text"].as_str().unwrap();
    assert!(note.contains("image not sent to the model"), "{note}");
    assert!(note.contains("gs://bucket/cat.png"), "{note}");
}

#[test]
fn inline_data_without_a_media_type_is_announced_not_dropped() {
    let body = build_messages_body(&image_request(ImageSource::new("data:;base64,AAAB")));
    let block = &body["messages"][0]["content"][1];
    assert_eq!(block["type"], "text");
    assert!(block["text"].as_str().unwrap().contains("media_type"));
}

#[test]
fn image_on_a_tool_message_rides_out_with_the_tool_results() {
    let body = build_messages_body(&ProviderRequest {
        model: "claude-sonnet-4-5".into(),
        messages: vec![
            ProviderMessage {
                role: "assistant".into(),
                content: vec![ProviderContentBlock::ToolCall {
                    id: "toolu_1".into(),
                    name: "screenshot".into(),
                    input: serde_json::json!({}),
                }],
            },
            ProviderMessage {
                role: "tool".into(),
                content: vec![
                    ProviderContentBlock::ToolResult {
                        tool_call_id: "toolu_1".into(),
                        content: "captured".into(),
                        name: Some("screenshot".into()),
                    },
                    ProviderContentBlock::Image {
                        image_url: ImageSource::new("data:image/png;base64,AAAB"),
                    },
                ],
            },
        ],
        system_prompt: None,
        tools: None,
        max_tokens: Some(256),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    });
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    let results = messages[1]["content"].as_array().unwrap();
    // tool_result first (Anthropic's requirement), image after it.
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["type"], "tool_result");
    assert_eq!(results[1]["type"], "image");
}

#[test]
fn capability_flag_matches_the_encoder() {
    let caps = AnthropicAdapter::new().capabilities();
    assert!(caps.image_input);
    assert!(caps.features.iter().any(|f| f == "image_input"));
    // The flag is only honest because an image really is encoded.
    let body = build_messages_body(&image_request(ImageSource::new(
        "data:image/jpeg;base64,AAAB",
    )));
    assert_eq!(body["messages"][0]["content"][1]["type"], "image");
}
