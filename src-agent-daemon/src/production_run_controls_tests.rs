use super::*;
use provider_adapters::capabilities::{
    history_message_to_provider, HistoryMessage, ProviderRequest,
};
use provider_adapters::providers::anthropic::build_messages_body;

fn request(model: &str, effort: Option<&str>) -> ProviderRequest {
    ProviderRequest {
        model: model.into(),
        messages: vec![history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "hi".into(),
            ..Default::default()
        })],
        system_prompt: None,
        tools: None,
        max_tokens: Some(64_000),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: run_request_controls(effort),
    }
}

#[test]
fn run_effort_reaches_the_provider_body() {
    // The daemon builds the request exactly like `RealProvider::stream_with_controls`
    // does; this pins the whole hop from the stored run field to the wire.
    let body = build_messages_body(&request("claude-sonnet-4-5", Some("high")));
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 32_768);

    let low = build_messages_body(&request("claude-sonnet-4-5", Some("low")));
    assert_eq!(low["thinking"]["budget_tokens"], 4_096);
}

#[test]
fn absent_or_unknown_effort_changes_nothing() {
    let baseline = build_messages_body(&request("claude-sonnet-4-5", None));
    assert!(baseline.get("thinking").is_none());
    assert_eq!(
        build_messages_body(&request("claude-sonnet-4-5", Some("ludicrous"))),
        baseline
    );
    assert_eq!(
        build_messages_body(&request("claude-sonnet-4-5", Some(""))),
        baseline
    );
}
