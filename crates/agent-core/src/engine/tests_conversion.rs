use super::*;

#[test]
fn strict_snapshot_decode_rejects_missing_identity_and_unknown_blocks() {
    let missing_id = serde_json::json!([{"role":"assistant","blocks":[]}]);
    assert!(try_agent_messages_from_json(&missing_id).is_err());
    let unknown_block = serde_json::json!([{
        "role": "assistant",
        "message_id": "m-1",
        "blocks": [{"type": "future_block"}]
    }]);
    assert!(try_agent_messages_from_json(&unknown_block).is_err());
    let valid = serde_json::json!([{
        "role": "tool",
        "message_id": "m-2",
        "tool_call_id": "call-1",
        "name": "read_file",
        "tool_result_blocks": [{"type": "json", "value": {"ok": true}}]
    }]);
    let decoded = try_agent_messages_from_json(&valid).expect("valid snapshot");
    assert_eq!(decoded.len(), 1);

    let malformed_arguments = serde_json::json!([{
        "role": "assistant",
        "message_id": "m-3",
        "blocks": [{
            "type": "tool_call",
            "tool_call_id": "call-2",
            "name": "read_file",
            "arguments": "{not-json}"
        }]
    }]);
    let error = try_agent_messages_from_json(&malformed_arguments).unwrap_err();
    assert!(error.contains("arguments are invalid JSON"));
}

#[test]
fn custom_snapshot_round_trips_losslessly() {
    let messages = vec![
        crate::AgentMessage::Custom(crate::CustomMessage {
            message_id: crate::MessageId::from("custom-snap-1"),
            kind: "recipe".into(),
            payload: serde_json::json!({"steps": 3, "tag": "chef"}),
        }),
        crate::AgentMessage::ToolResult(crate::ToolResultMessage {
            message_id: crate::MessageId::from("tool-result-snap-1"),
            tool_call_id: crate::ToolCallId::from("call-snap-1"),
            tool_name: "read_file".into(),
            content: vec![crate::ToolResultBlock::Json {
                value: serde_json::json!({"ok": true}),
            }],
            is_error: false,
            code: None,
        }),
    ];
    let values = agent_messages_to_values(&messages);
    let decoded = try_agent_messages_from_json(&Value::Array(values))
        .expect("strict snapshot decode must accept custom + tool results");
    assert_eq!(
        decoded, messages,
        "snapshot Custom kind/payload and ToolResult identity must be lossless"
    );
}

// ---- multimodal history ----------------------------------------------

#[test]
fn images_survive_the_compaction_value_round_trip() {
    let original = vec![EngineMessage {
        role: "user".into(),
        content: "what is this".into(),
        images: vec![EngineImage {
            url: "data:image/png;base64,AAAB".into(),
            media_type: Some("image/png".into()),
            detail: Some("high".into()),
        }],
        ..Default::default()
    }];
    let values = engine_messages_to_values(&original);
    assert_eq!(values[0]["images"][0]["url"], "data:image/png;base64,AAAB");
    assert_eq!(values_to_engine_messages(&values), original);
}

#[test]
fn text_only_messages_do_not_grow_an_images_key() {
    let values = engine_messages_to_values(&[EngineMessage::text("user", "hi")]);
    assert!(values[0].get("images").is_none(), "{:?}", values[0]);
}

// ---- AgentMessage ↔ EngineMessage round-trip ---------------------------
//
// These tests verify that the conversion between the typed AgentMessage
// (message.rs) and the legacy EngineMessage preserves every content block
// type. Known limitations are documented per test:
//
// - Thinking blocks: signature field is lost (EngineMessage has no
//   dedicated Thinking slot; text survives in the content field).
// - Custom messages: payload is flattened to a string (EngineMessage has
//   no structured payload field).
// - Multiple text blocks within one AgentMessage are concatenated during
//   the round-trip (EngineMessage stores a single content string).

#[test]
fn thinking_block_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::Assistant(crate::AssistantMessage {
        message_id: crate::MessageId::from("thinking-1"),
        content: vec![
            crate::ContentBlock::Thinking {
                text: "step 1: analyze the problem".into(),
                signature: Some("sig_abc123".into()),
            },
            crate::ContentBlock::Text {
                text: "The answer is 42".into(),
            },
        ],
        stop_reason: Some(crate::StopReason::Stop),
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    // Text from both Thinking and Text blocks is concatenated
    assert!(engine_msgs[0]
        .content
        .contains("step 1: analyze the problem"));
    assert!(engine_msgs[0].content.contains("The answer is 42"));

    // Round-trip back: signature is lost (EngineMessage has no slot)
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let assistant = match &round_tripped[0] {
        crate::AgentMessage::Assistant(msg) => msg,
        other => panic!("expected Assistant, got {other:?}"),
    };
    // Text content survives
    assert!(assistant.content.iter().any(|b| matches!(b,
        crate::ContentBlock::Text { text } if text.contains("step 1: analyze the problem")
    )));
    assert!(assistant.content.iter().any(|b| matches!(b,
        crate::ContentBlock::Text { text } if text.contains("The answer is 42")
    )));
    // Thinking signature is lost (documented limitation)
    assert!(!assistant.content.iter().any(|b| matches!(
        b,
        crate::ContentBlock::Thinking {
            signature: Some(_),
            ..
        }
    )));
}

#[test]
fn image_block_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::User(crate::UserMessage {
        message_id: crate::MessageId::from("img-1"),
        content: vec![
            crate::ContentBlock::Text {
                text: "what is this image".into(),
            },
            crate::ContentBlock::Image {
                source: crate::ImageSource {
                    url: "data:image/png;base64,AAAB".into(),
                    media_type: Some("image/png".into()),
                    detail: Some("high".into()),
                },
            },
        ],
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    assert_eq!(engine_msgs[0].images.len(), 1);
    assert_eq!(engine_msgs[0].images[0].url, "data:image/png;base64,AAAB");
    assert_eq!(
        engine_msgs[0].images[0].media_type,
        Some("image/png".into())
    );

    // Round-trip back
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let user = match &round_tripped[0] {
        crate::AgentMessage::User(msg) => msg,
        other => panic!("expected User, got {other:?}"),
    };
    let image_blocks: Vec<_> = user
        .content
        .iter()
        .filter_map(|b| {
            if let crate::ContentBlock::Image { source } = b {
                Some(source)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(image_blocks.len(), 1, "image block must survive round-trip");
    assert_eq!(image_blocks[0].url, "data:image/png;base64,AAAB");
    assert_eq!(image_blocks[0].media_type, Some("image/png".into()));
    assert_eq!(image_blocks[0].detail, Some("high".into()));
}

#[test]
fn custom_message_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::Custom(crate::CustomMessage {
        message_id: crate::MessageId::from("custom-1"),
        kind: "recipe".into(),
        payload: serde_json::json!({"steps": 3, "tag": "chef"}),
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    // Custom message kind becomes role, payload becomes content string
    assert_eq!(engine_msgs[0].role, "recipe");
    assert!(engine_msgs[0].content.contains("chef"));

    // Round-trip back: payload is a string, not structured JSON
    // (documented limitation of the EngineMessage format)
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let custom = match &round_tripped[0] {
        crate::AgentMessage::Custom(msg) => msg,
        other => panic!("expected Custom, got {other:?}"),
    };
    assert_eq!(custom.kind, "recipe");
    // The structured payload is flattened to a string in the round-trip
    // (EngineMessage has no structured payload field)
    assert!(custom.payload.to_string().contains("chef"));
}

#[test]
fn tool_call_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::Assistant(crate::AssistantMessage {
        message_id: crate::MessageId::from("tool-call-1"),
        content: vec![
            crate::ContentBlock::Text {
                text: "Let me check the file".into(),
            },
            crate::ContentBlock::ToolCall(crate::ToolCall {
                tool_call_id: crate::ToolCallId::from("call_read"),
                name: "read_file".into(),
                arguments_json: r#"{"path":"Cargo.toml"}"#.into(),
            }),
        ],
        stop_reason: Some(crate::StopReason::ToolUse),
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    assert!(engine_msgs[0].tool_calls.is_some());
    let calls = engine_msgs[0].tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "read_file");
    assert_eq!(calls[0].id, "call_read");

    // Round-trip back
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let assistant = match &round_tripped[0] {
        crate::AgentMessage::Assistant(msg) => msg,
        other => panic!("expected Assistant, got {other:?}"),
    };
    let tool_calls: Vec<_> = assistant
        .content
        .iter()
        .filter_map(|b| {
            if let crate::ContentBlock::ToolCall(call) = b {
                Some(call)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(tool_calls.len(), 1, "tool call must survive round-trip");
    assert_eq!(tool_calls[0].name, "read_file");
    assert_eq!(tool_calls[0].tool_call_id.to_string(), "call_read");
}

#[test]
fn tool_result_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::ToolResult(crate::ToolResultMessage {
        message_id: crate::MessageId::from("tr-1"),
        tool_call_id: crate::ToolCallId::from("call_read"),
        tool_name: "read_file".into(),
        content: vec![crate::ToolResultBlock::Json {
            value: serde_json::json!({"content": "fn main() {}", "path": "src/main.rs"}),
        }],
        is_error: false,
        code: None,
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    assert_eq!(engine_msgs[0].role, "tool");
    assert_eq!(engine_msgs[0].tool_call_id, Some("call_read".into()));
    assert_eq!(engine_msgs[0].tool_name, Some("read_file".into()));

    // Round-trip back
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let result = match &round_tripped[0] {
        crate::AgentMessage::ToolResult(msg) => msg,
        other => panic!("expected ToolResult, got {other:?}"),
    };
    assert_eq!(result.tool_call_id.to_string(), "call_read");
    assert_eq!(result.tool_name, "read_file");
    assert!(result.content.iter().any(|b| matches!(b,
        crate::ToolResultBlock::Json { value } if value.to_string().contains("fn main()")
    )));
}

#[test]
fn round_trip_preserves_message_order_and_roles() {
    let original = vec![
        crate::AgentMessage::System(crate::SystemMessage {
            message_id: crate::MessageId::from("sys-1"),
            text: "You are a helpful assistant".into(),
        }),
        crate::AgentMessage::User(crate::UserMessage {
            message_id: crate::MessageId::from("user-1"),
            content: vec![crate::ContentBlock::Text {
                text: "Hello".into(),
            }],
        }),
        crate::AgentMessage::Assistant(crate::AssistantMessage {
            message_id: crate::MessageId::from("asst-1"),
            content: vec![crate::ContentBlock::Text {
                text: "Hi! How can I help?".into(),
            }],
            stop_reason: None,
        }),
    ];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 3);
    assert_eq!(engine_msgs[0].role, "system");
    assert_eq!(engine_msgs[1].role, "user");
    assert_eq!(engine_msgs[2].role, "assistant");

    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 3);
    assert!(matches!(round_tripped[0], crate::AgentMessage::System(_)));
    assert!(matches!(round_tripped[1], crate::AgentMessage::User(_)));
    assert!(matches!(
        round_tripped[2],
        crate::AgentMessage::Assistant(_)
    ));
}
