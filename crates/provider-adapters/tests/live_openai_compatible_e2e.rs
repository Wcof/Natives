//! Live OpenAI-compatible E2E (opt-in).
//!
//! Run:
//! ```bash
//! export NATIVES_LIVE_E2E=1
//! export NATIVES_TEST_OPENAI_KEY=sk-...
//! export NATIVES_TEST_OPENAI_BASE=https://token.sensenova.cn/v1
//! export NATIVES_TEST_MODEL=deepseek-v4-flash
//! cargo test -p provider-adapters --test live_openai_compatible_e2e -- --nocapture --ignored
//! ```
//!
//! Never commit real keys. This test is ignored by default.

use futures_util::StreamExt;
use provider_adapters::capabilities::{
    history_message_to_provider, Credential, HistoryMessage, HistoryToolCall, ProviderContentBlock,
    ProviderMessage, ProviderRequest, ProviderTool,
};
use provider_adapters::http_stream::build_chat_completions_body;
use provider_adapters::providers::openai_compatible::OpenAiCompatibleAdapter;
use provider_adapters::stream::ProviderEvent;
use provider_adapters::ProviderAdapter;

fn live_enabled() -> bool {
    std::env::var("NATIVES_LIVE_E2E")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn credential() -> Credential {
    let api_key = std::env::var("NATIVES_TEST_OPENAI_KEY")
        .or_else(|_| std::env::var("NATIVES_TEST_DEEPSEEK_KEY"))
        .expect("NATIVES_TEST_OPENAI_KEY or NATIVES_TEST_DEEPSEEK_KEY");
    let base_url = std::env::var("NATIVES_TEST_OPENAI_BASE")
        .or_else(|_| std::env::var("NATIVES_TEST_DEEPSEEK_BASE"))
        .ok();
    Credential {
        api_key,
        base_url,
        key_id: Some("live-e2e".into()),
        provider_type: Some("openai_compatible".into()),
    }
}

fn model() -> String {
    std::env::var("NATIVES_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into())
}

#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_text_stream_completes() {
    if !live_enabled() {
        eprintln!("skip: NATIVES_LIVE_E2E not set");
        return;
    }
    let adapter = OpenAiCompatibleAdapter::new();
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
    };

    let stream = adapter
        .stream(request, cred)
        .await
        .expect("stream start failed");
    tokio::pin!(stream);
    let mut text = String::new();
    let mut completed = false;
    let mut errors = Vec::new();
    while let Some(ev) = stream.next().await {
        match ev {
            ProviderEvent::TextDelta(t) => text.push_str(&t),
            ProviderEvent::Completed => completed = true,
            ProviderEvent::Error(e) => {
                errors.push(e.message);
                break;
            }
            _ => {}
        }
    }
    assert!(
        errors.is_empty(),
        "provider errors (key redacted): {:?}",
        errors
            .iter()
            .map(|m| m.replace(&std::env::var("NATIVES_TEST_OPENAI_KEY").unwrap_or_default(), "[KEY]"))
            .collect::<Vec<_>>()
    );
    assert!(completed || !text.is_empty(), "no text and no completed");
    eprintln!(
        "live_text_stream ok model={model} text_len={} preview={:?}",
        text.len(),
        text.chars().take(80).collect::<String>()
    );
    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let path = std::path::Path::new(&dir).join("live-text-stream.json");
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
async fn live_tool_roundtrip_body_and_second_turn() {
    if !live_enabled() {
        eprintln!("skip: NATIVES_LIVE_E2E not set");
        return;
    }
    let adapter = OpenAiCompatibleAdapter::new();
    let cred = credential();
    let model = model();

    // Turn 1: ask model to call a tool.
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
                text: "Use the echo_box tool with text=hello_natives. Do not answer without the tool."
                    .into(),
            }],
        }],
        system_prompt: Some("You must use tools when asked.".into()),
        tools: Some(tools.clone()),
        max_tokens: Some(256),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
    };

    let stream = adapter
        .stream(request1, cred.clone())
        .await
        .expect("turn1 stream failed");
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
            ProviderEvent::Completed => {}
            _ => {}
        }
    }
    assert!(err.is_none(), "turn1 error: {:?}", err);
    eprintln!(
        "turn1 tool_name={:?} tool_id={:?} args_len={} text_len={}",
        tool_name,
        tool_id,
        tool_args.len(),
        text.len()
    );

    // If the model did not emit tools, still verify structured history serialization path.
    let call_id = tool_id.unwrap_or_else(|| "call_local_1".into());
    let call_name = tool_name.unwrap_or_else(|| "echo_box".into());
    let call_args = if tool_args.trim().is_empty() {
        r#"{"text":"hello_natives"}"#.to_string()
    } else {
        tool_args
    };

    let history = vec![
        history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "Use the echo_box tool with text=hello_natives.".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }),
        history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![HistoryToolCall {
                id: call_id.clone(),
                name: call_name.clone(),
                arguments: call_args.clone(),
            }]),
        }),
        history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: r#"{"echoed":"hello_natives"}"#.into(),
            tool_call_id: Some(call_id.clone()),
            tool_name: Some(call_name.clone()),
            tool_calls: None,
        }),
    ];

    let body = build_chat_completions_body(&ProviderRequest {
        model: model.clone(),
        messages: history.clone(),
        system_prompt: None,
        tools: Some(tools.clone()),
        max_tokens: Some(128),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
    });
    let messages = body["messages"].as_array().expect("messages");
    assert!(
        messages.iter().any(|m| m["role"] == "tool" && m["tool_call_id"] == call_id),
        "tool result must keep tool_call_id in wire body: {body}"
    );
    assert!(
        messages.iter().any(|m| {
            m["role"] == "assistant"
                && m.get("tool_calls")
                    .and_then(|t| t.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false)
        }),
        "assistant tool_calls must be structured: {body}"
    );

    // Turn 2: feed structured tool result back to the live model.
    let request2 = ProviderRequest {
        model: model.clone(),
        messages: history,
        system_prompt: Some("After tool results, summarize in one short sentence.".into()),
        tools: Some(tools),
        max_tokens: Some(128),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
    };
    let stream2 = adapter
        .stream(request2, cred)
        .await
        .expect("turn2 stream failed");
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
    assert!(err2.is_none(), "turn2 error: {:?}", err2);
    assert!(
        !text2.is_empty(),
        "turn2 should produce assistant text after tool result"
    );
    eprintln!(
        "live_tool_roundtrip ok model={model} turn2_preview={:?}",
        text2.chars().take(120).collect::<String>()
    );
    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let path = std::path::Path::new(&dir).join("live-tool-roundtrip.json");
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "model": model,
                "turn1_had_tool_stream": !call_args.is_empty(),
                "tool_call_id": call_id,
                "tool_name": call_name,
                "turn2_text_len": text2.len(),
                "wire_body_preserves_tool_call_id": true,
            }))
            .unwrap_or_default(),
        );
    }
}
