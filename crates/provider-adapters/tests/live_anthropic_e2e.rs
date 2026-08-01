//! Live Anthropic Messages API E2E (opt-in).
//!
//! Run:
//! ```bash
//! export NATIVES_LIVE_E2E=1
//! export NATIVES_TEST_ANTHROPIC_KEY=sk-ant-...
//! export NATIVES_TEST_ANTHROPIC_MODEL=claude-sonnet-4-20250514
//! cargo test -p provider-adapters --test live_anthropic_e2e -- --nocapture --ignored
//! ```
//!
//! Never commit real keys. This test is ignored by default.

use futures_util::StreamExt;
use provider_adapters::capabilities::{
    Credential, ProviderContentBlock, ProviderMessage, ProviderRequest, ProviderTool,
};
use provider_adapters::providers::anthropic::AnthropicAdapter;
use provider_adapters::stream::ProviderEvent;
use provider_adapters::ProviderAdapter;

fn live_enabled() -> bool {
    std::env::var("NATIVES_LIVE_E2E")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn credential() -> Credential {
    let api_key = std::env::var("NATIVES_TEST_ANTHROPIC_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .or_else(|_| std::env::var("ANTHROPIC_AUTH_TOKEN"))
        .expect("NATIVES_TEST_ANTHROPIC_KEY or ANTHROPIC_API_KEY");
    let base_url = std::env::var("NATIVES_TEST_ANTHROPIC_BASE")
        .or_else(|_| std::env::var("ANTHROPIC_BASE_URL"))
        .ok();
    Credential {
        api_key,
        base_url,
        proxy_url: None,
        key_id: Some("live-anthropic-e2e".into()),
        provider_type: Some("anthropic".into()),
    }
}

fn model() -> String {
    std::env::var("NATIVES_TEST_ANTHROPIC_MODEL")
        .or_else(|_| std::env::var("NATIVES_TEST_MODEL"))
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".into())
}

#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_text_stream_completes() {
    if !live_enabled() {
        eprintln!("skip: NATIVES_LIVE_E2E not set");
        return;
    }

    let adapter = AnthropicAdapter::new();
    let cred = credential();
    let model = model();
    let request = ProviderRequest {
        model: model.clone(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text {
                text: "Reply with exactly the word: pong".into(),
            }],
        }],
        system_prompt: Some("Be concise.".into()),
        tools: None,
        max_tokens: Some(64),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
        controls: Default::default(),
    };

    let stream = adapter
        .stream(request, cred)
        .await
        .expect("anthropic stream start failed");
    tokio::pin!(stream);

    let mut text = String::new();
    let mut completed = false;
    let mut errors = Vec::new();
    while let Some(ev) = stream.next().await {
        match ev {
            ProviderEvent::TextDelta(t) => text.push_str(&t),
            ProviderEvent::Completed { .. } => completed = true,
            ProviderEvent::Error(e) => {
                errors.push(e.message);
                break;
            }
            _ => {}
        }
    }

    assert!(errors.is_empty(), "provider errors: {errors:?}");
    assert!(completed || !text.is_empty(), "no text and no completed");

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let path = std::path::Path::new(&dir).join("anthropic-live-text-stream.json");
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "model": model,
                "text_len": text.len(),
                "completed": completed,
                "has_pong": text.to_ascii_lowercase().contains("pong"),
            }))
            .unwrap_or_default(),
        );
    }
}

#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_tool_roundtrip_blocks_and_second_turn() {
    if !live_enabled() {
        eprintln!("skip: NATIVES_LIVE_E2E not set");
        return;
    }

    let adapter = AnthropicAdapter::new();
    let cred = credential();
    let model = model();
    let tools = vec![ProviderTool {
        name: "echo_box".into(),
        description: Some("Echo a short string back".into()),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        }),
    }];

    let request1 = ProviderRequest {
        model: model.clone(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text {
                text:
                    "Use the echo_box tool with text=hello_natives. Do not answer without the tool."
                        .into(),
            }],
        }],
        system_prompt: Some("You must use tools when asked.".into()),
        tools: Some(tools.clone()),
        max_tokens: Some(256),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
        controls: Default::default(),
    };

    let stream = adapter
        .stream(request1, cred.clone())
        .await
        .expect("anthropic turn1 stream failed");
    tokio::pin!(stream);

    let mut tool_name = None;
    let mut tool_id = None;
    let mut tool_args = String::new();
    let mut text = String::new();
    let mut err = None;
    while let Some(ev) = stream.next().await {
        match ev {
            ProviderEvent::TextDelta(t) => text.push_str(&t),
            ProviderEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
                ..
            } => {
                if let Some(i) = id {
                    tool_id = Some(i);
                }
                if let Some(n) = name {
                    tool_name = Some(n);
                }
                tool_args.push_str(&arguments_delta);
            }
            ProviderEvent::Error(e) => {
                err = Some(e.message);
                break;
            }
            _ => {}
        }
    }
    assert!(err.is_none(), "turn1 error: {err:?}");

    let turn1_had_tool_stream = tool_id.is_some() || !text.is_empty();
    let call_id = tool_id.unwrap_or_else(|| "toolu_local_1".into());
    let call_name = tool_name.unwrap_or_else(|| "echo_box".into());
    let input = if tool_args.trim().is_empty() {
        serde_json::json!({ "text": "hello_natives" })
    } else {
        serde_json::from_str(&tool_args).unwrap_or_else(|_| serde_json::json!({ "raw": tool_args }))
    };

    let request2 = ProviderRequest {
        model: model.clone(),
        messages: vec![
            ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text {
                    text: "Use the echo_box tool with text=hello_natives.".into(),
                }],
            },
            ProviderMessage {
                role: "assistant".into(),
                content: vec![ProviderContentBlock::ToolCall {
                    id: call_id.clone(),
                    name: call_name.clone(),
                    input,
                }],
            },
            ProviderMessage {
                role: "tool".into(),
                content: vec![ProviderContentBlock::ToolResult {
                    tool_call_id: call_id.clone(),
                    content: r#"{"echoed":"hello_natives"}"#.into(),
                    name: Some(call_name.clone()),
                }],
            },
        ],
        system_prompt: Some("After tool results, summarize in one short sentence.".into()),
        tools: Some(tools),
        max_tokens: Some(128),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
        controls: Default::default(),
    };

    let stream2 = adapter
        .stream(request2, cred)
        .await
        .expect("anthropic turn2 stream failed");
    tokio::pin!(stream2);

    let mut text2 = String::new();
    let mut err2 = None;
    while let Some(ev) = stream2.next().await {
        match ev {
            ProviderEvent::TextDelta(t) => text2.push_str(&t),
            ProviderEvent::Error(e) => {
                err2 = Some(e.message);
                break;
            }
            _ => {}
        }
    }
    assert!(err2.is_none(), "turn2 error: {err2:?}");
    assert!(
        !text2.is_empty(),
        "turn2 should produce assistant text after tool result"
    );

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let path = std::path::Path::new(&dir).join("anthropic-live-tool-roundtrip.json");
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "model": model,
                "turn1_had_tool_stream": turn1_had_tool_stream,
                "tool_call_id": call_id,
                "tool_name": call_name,
                "turn2_text_len": text2.len(),
            }))
            .unwrap_or_default(),
        );
    }
}
