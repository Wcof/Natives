use super::*;
use provider_adapters::stream::{
    sse_data_payload, AnthropicSseParser, OpenAiResponsesParser, OpenAiSseParser, ProviderEvent,
};

fn canonical_events() -> Vec<EngineProviderEvent> {
    vec![
        EngineProviderEvent::ReasoningDelta("Need the file.".into()),
        EngineProviderEvent::ToolCallDelta {
            index: 0,
            id: Some("call_fixture".into()),
            name: Some("read_file".into()),
            arguments_delta: String::new(),
        },
        EngineProviderEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments_delta: "{\"path\":\"AGENTS.md\"}".into(),
        },
        EngineProviderEvent::Usage {
            input_tokens: 40,
            output_tokens: 18,
            reasoning_tokens: Some(4),
            cache_creation_tokens: None,
            cache_read_tokens: Some(80),
        },
        EngineProviderEvent::CompletedWithReason {
            reason: ProviderStopReason::ToolUse,
        },
    ]
}

fn encode(path: &str) -> String {
    let mut encoder = StreamEncoder::new(path).unwrap();
    let mut wire = canonical_events()
        .into_iter()
        .map(|event| encoder.encode(event).unwrap())
        .collect::<String>();
    if let Some(done) = encoder.chat_done_frame() {
        wire.push_str(done);
    }
    assert!(encoder.is_terminal());
    wire
}

#[test]
fn chat_round_trip_preserves_reasoning_tool_usage_and_stop_reason() {
    let mut parser = OpenAiSseParser::new();
    let events = encode("/v1/chat/completions")
        .lines()
        .filter_map(sse_data_payload)
        .flat_map(|data| parser.push_data_line(data))
        .collect::<Vec<_>>();
    assert_canonical(&events);
}

#[test]
fn responses_round_trip_preserves_reasoning_tool_usage_and_stop_reason() {
    let mut parser = OpenAiResponsesParser::new();
    let events = encode("/v1/responses")
        .lines()
        .filter_map(sse_data_payload)
        .flat_map(|data| parser.push_data_line(data))
        .collect::<Vec<_>>();
    assert_canonical(&events);
}

#[test]
fn messages_round_trip_preserves_reasoning_tool_usage_and_stop_reason() {
    let mut parser = AnthropicSseParser::new();
    let events = encode("/v1/messages")
        .lines()
        .filter_map(sse_data_payload)
        .flat_map(|data| parser.push_data_line(data))
        .collect::<Vec<_>>();
    assert_canonical(&events);
}

fn assert_canonical(events: &[ProviderEvent]) {
    assert!(events.iter().any(
        |event| matches!(event, ProviderEvent::ReasoningDelta(text) if text == "Need the file.")
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::ToolCallDelta { id: Some(id), name: Some(name), .. }
            if id == "call_fixture" && name == "read_file"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::ToolCallDelta { arguments_delta, .. }
            if arguments_delta.contains("AGENTS.md")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::Usage(usage)
            if usage.input_tokens == 40
                && usage.output_tokens == 18
                && usage.total_prompt_tokens() == 120
    )));
    assert!(matches!(
        events.last(),
        Some(ProviderEvent::Completed {
            reason: provider_adapters::stream::ProviderStopReason::ToolUse
        })
    ));
}
