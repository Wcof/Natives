use provider_adapters::stream::{
    parse_gemini_chunk, sse_data_payload, AnthropicSseParser, OpenAiResponsesParser,
    OpenAiSseParser, ProviderEvent, ProviderStopReason,
};

fn payloads(fixture: &str) -> impl Iterator<Item = &str> {
    fixture.lines().filter_map(sse_data_payload)
}

fn assert_successful_terminal(events: &[ProviderEvent], reason: ProviderStopReason) {
    let terminals = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            matches!(
                event,
                ProviderEvent::Completed { .. } | ProviderEvent::Error(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(terminals.len(), 1, "events: {events:?}");
    assert_eq!(terminals[0].0, events.len() - 1, "events: {events:?}");
    assert!(matches!(
        terminals[0].1,
        ProviderEvent::Completed { reason: actual } if *actual == reason
    ));
}

fn assert_common_payload(events: &[ProviderEvent]) {
    assert!(events.iter().any(
        |event| matches!(event, ProviderEvent::ReasoningDelta(text) if text == "Need the file.")
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::ToolCallDelta { name: Some(name), arguments_delta, .. }
            if name == "read_file" && arguments_delta.contains("AGENTS.md")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::Usage(usage)
            if usage.input_tokens == 40
                && usage.output_tokens == 18
                && usage.total_prompt_tokens() == 120
    )));
}

#[test]
fn chat_completions_fixture_preserves_tool_reasoning_usage_and_stop_reason() {
    let fixture = include_str!("fixtures/openai-chat-completions.sse");
    let mut parser = OpenAiSseParser::new();
    let events = payloads(fixture)
        .flat_map(|payload| parser.push_data_line(payload))
        .collect::<Vec<_>>();

    assert_common_payload(&events);
    assert_successful_terminal(&events, ProviderStopReason::ToolUse);
}

#[test]
fn responses_fixture_keeps_function_identity_across_events() {
    let fixture = include_str!("fixtures/openai-responses.sse");
    let mut parser = OpenAiResponsesParser::new();
    let events = payloads(fixture)
        .flat_map(|payload| parser.push_data_line(payload))
        .collect::<Vec<_>>();

    assert_common_payload(&events);
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::ToolCallDelta { id: Some(id), name: Some(name), arguments_delta, .. }
            if id == "call_fixture" && name == "read_file" && arguments_delta.contains("AGENTS.md")
    )));
    assert_successful_terminal(&events, ProviderStopReason::ToolUse);
}

#[test]
fn anthropic_messages_fixture_preserves_tool_reasoning_usage_and_stop_reason() {
    let fixture = include_str!("fixtures/anthropic-messages.sse");
    let mut parser = AnthropicSseParser::new();
    let events = payloads(fixture)
        .flat_map(|payload| parser.push_data_line(payload))
        .collect::<Vec<_>>();

    assert_common_payload(&events);
    assert_successful_terminal(&events, ProviderStopReason::ToolUse);
}

#[test]
fn gemini_fixture_keeps_final_chunk_payload_before_terminal() {
    let fixture = include_str!("fixtures/gemini-generate-content.sse");
    let events = payloads(fixture)
        .flat_map(parse_gemini_chunk)
        .collect::<Vec<_>>();

    assert_common_payload(&events);
    assert_successful_terminal(&events, ProviderStopReason::Stop);
}
