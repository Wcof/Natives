use super::*;

fn canonical_encoder(path: &str, reason: ProviderStopReason) -> CompletionEncoder {
    let mut encoder = CompletionEncoder::new(path, "fixture-model").unwrap();
    for event in [
        EngineProviderEvent::ReasoningDelta("Need the file.".into()),
        EngineProviderEvent::TextDelta("Reading it.".into()),
        EngineProviderEvent::ToolCallDelta {
            index: 0,
            id: Some("call_fixture".into()),
            name: Some("read_file".into()),
            arguments_delta: "{\"path\":\"AGENTS.md\"}".into(),
        },
        EngineProviderEvent::Usage {
            input_tokens: 40,
            output_tokens: 18,
            reasoning_tokens: Some(4),
            cache_creation_tokens: None,
            cache_read_tokens: Some(80),
        },
        EngineProviderEvent::CompletedWithReason { reason },
    ] {
        encoder.push(event).unwrap();
    }
    encoder
}

#[test]
fn chat_completion_preserves_reasoning_tool_usage_and_stop_reason() {
    let payload = canonical_encoder("/v1/chat/completions", ProviderStopReason::ToolUse)
        .finish()
        .unwrap();
    assert_eq!(payload["choices"][0]["message"]["content"], "Reading it.");
    assert_eq!(
        payload["choices"][0]["message"]["reasoning_content"],
        "Need the file."
    );
    assert_eq!(
        payload["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"AGENTS.md\"}"
    );
    assert_eq!(payload["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(payload["usage"]["prompt_tokens"], 120);
    assert_eq!(
        payload["usage"]["completion_tokens_details"]["reasoning_tokens"],
        4
    );
}

#[test]
fn responses_completion_preserves_all_output_and_length_state() {
    let payload = canonical_encoder("/v1/responses", ProviderStopReason::Length)
        .finish()
        .unwrap();
    assert_eq!(payload["status"], "incomplete");
    assert_eq!(payload["incomplete_details"]["reason"], "max_output_tokens");
    assert_eq!(payload["output"][0]["summary"][0]["text"], "Need the file.");
    assert_eq!(payload["output"][1]["content"][0]["text"], "Reading it.");
    assert_eq!(payload["output"][2]["call_id"], "call_fixture");
    assert_eq!(payload["usage"]["input_tokens"], 120);
}

#[test]
fn messages_completion_preserves_blocks_usage_and_stop_reason() {
    let payload = canonical_encoder("/v1/messages", ProviderStopReason::ToolUse)
        .finish()
        .unwrap();
    assert_eq!(payload["content"][0]["type"], "thinking");
    assert_eq!(payload["content"][1]["text"], "Reading it.");
    assert_eq!(payload["content"][2]["input"]["path"], "AGENTS.md");
    assert_eq!(payload["stop_reason"], "tool_use");
    assert_eq!(payload["usage"]["input_tokens"], 40);
    assert_eq!(payload["usage"]["cache_read_input_tokens"], 80);
}

#[test]
fn completion_rejects_stream_without_terminal_event() {
    let mut encoder = CompletionEncoder::new("/v1/chat/completions", "fixture-model").unwrap();
    encoder
        .push(EngineProviderEvent::TextDelta("partial".into()))
        .unwrap();
    assert_eq!(
        encoder.finish().unwrap_err(),
        "provider stream ended without a terminal event"
    );
}
