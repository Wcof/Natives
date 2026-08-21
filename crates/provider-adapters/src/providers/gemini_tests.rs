use super::*;
use crate::controls::{ReasoningEffort, ReasoningRequest, ToolChoice};

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

#[test]
fn stream_url_never_contains_api_key_material() {
    let url = stream_generate_content_url(
        "https://generativelanguage.googleapis.com/v1beta/",
        "gemini-2.5-pro",
    );

    assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
        );
    assert!(!url.contains("key="));
}

#[tokio::test]
async fn stream_sends_api_key_in_header_not_request_target() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0_u8; 16 * 1024];
        let bytes_read = socket.read(&mut request).await.unwrap();
        socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        String::from_utf8_lossy(&request[..bytes_read]).into_owned()
    });

    let adapter = GeminiAdapter::new().with_base_url(format!("http://{address}"));
    let _events = adapter
        .stream(
            plain("gemini-2.5-pro"),
            Credential {
                api_key: "header-secret".into(),
                base_url: None,
                proxy_url: None,
                key_id: None,
                provider_type: Some("gemini".into()),
                project_id: None,
            },
        )
        .await
        .unwrap();
    let raw_request = server.await.unwrap();
    let lower = raw_request.to_ascii_lowercase();

    assert!(lower.starts_with("post /models/gemini-2.5-pro:streamgeneratecontent?alt=sse http/1.1"));
    assert!(lower.contains("x-goog-api-key: header-secret\r\n"));
    assert!(!lower
        .lines()
        .next()
        .unwrap_or_default()
        .contains("header-secret"));
    assert!(!lower.lines().next().unwrap_or_default().contains("key="));
}

#[test]
fn max_output_tokens_comes_from_the_model_profile() {
    assert_eq!(
        build_generate_body(&plain("gemini-2.5-pro"))["generationConfig"]["maxOutputTokens"],
        65_536
    );
    // Unknown model: no generationConfig at all rather than an invented cap.
    assert!(build_generate_body(&plain("gemma-local"))
        .get("generationConfig")
        .is_none());
}

#[test]
fn thinking_budget_only_for_models_that_expose_it() {
    let controls = RequestControls {
        reasoning: Some(ReasoningRequest::new(ReasoningEffort::Medium)),
        ..Default::default()
    };
    let pro = build_generate_body_with_controls(&plain("gemini-2.5-pro"), &controls);
    assert_eq!(
        pro["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        16_384
    );
    assert_eq!(
        pro["generationConfig"]["thinkingConfig"]["includeThoughts"],
        true
    );

    // Gemini 2.0 has no thinkingConfig; sending one is rejected.
    let flash = build_generate_body_with_controls(&plain("gemini-2.0-flash"), &controls);
    assert!(flash["generationConfig"].get("thinkingConfig").is_none());
}

#[test]
fn tool_choice_encodes_to_function_calling_config() {
    let mut request = plain("gemini-2.5-pro");
    request.tools = Some(vec![ProviderTool {
        name: "get_weather".into(),
        description: Some("weather".into()),
        input_schema: serde_json::json!({"type": "object"}),
    }]);

    let forced = build_generate_body_with_controls(
        &request,
        &RequestControls {
            tool_choice: Some(ToolChoice::Tool {
                name: "get_weather".into(),
            }),
            ..Default::default()
        },
    );
    let config = &forced["toolConfig"]["functionCallingConfig"];
    assert_eq!(config["mode"], "ANY");
    assert_eq!(config["allowedFunctionNames"][0], "get_weather");

    // Nothing requested: no toolConfig, provider default applies.
    assert!(build_generate_body(&request).get("toolConfig").is_none());
}

#[test]
fn gemini_body_uses_function_call_and_response() {
    let body = build_generate_body(&ProviderRequest {
        model: "gemini-2.0-flash".into(),
        messages: vec![
            ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text {
                    text: "weather?".into(),
                }],
            },
            ProviderMessage {
                role: "assistant".into(),
                content: vec![ProviderContentBlock::ToolCall {
                    id: "c1".into(),
                    name: "get_weather".into(),
                    input: serde_json::json!({"city": "SF"}),
                }],
            },
            ProviderMessage {
                role: "tool".into(),
                content: vec![ProviderContentBlock::ToolResult {
                    tool_call_id: "c1".into(),
                    content: r#"{"temp":72}"#.into(),
                    name: Some("get_weather".into()),
                }],
            },
        ],
        system_prompt: None,
        tools: None,
        max_tokens: None,
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    });

    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 3);
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(
        contents[1]["parts"][0]["functionCall"]["name"],
        "get_weather"
    );
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["temp"],
        72
    );
}

fn image_request(image: ImageSource) -> ProviderRequest {
    ProviderRequest {
        model: "gemini-2.5-flash".into(),
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
        max_tokens: None,
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

#[test]
fn data_uri_becomes_an_inline_data_part() {
    let body = build_generate_body(&image_request(ImageSource::new(
        "data:image/png;base64,AAAB",
    )));
    let part = &body["contents"][0]["parts"][1];
    assert_eq!(part["inlineData"]["mimeType"], "image/png");
    assert_eq!(part["inlineData"]["data"], "AAAB");
}

#[test]
fn google_file_uri_becomes_a_file_data_part() {
    let body = build_generate_body(&image_request(
        ImageSource::new("gs://bucket/cat.png").with_media_type("image/png"),
    ));
    let part = &body["contents"][0]["parts"][1];
    assert_eq!(part["fileData"]["mimeType"], "image/png");
    assert_eq!(part["fileData"]["fileUri"], "gs://bucket/cat.png");
}

#[test]
fn plain_web_url_is_announced_not_dropped() {
    // Gemini does not fetch arbitrary URLs. Historically this block was an
    // empty match arm and the image simply vanished.
    let body = build_generate_body(&image_request(ImageSource::new(
        "https://example.test/cat.png",
    )));
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    let note = parts[1]["text"].as_str().unwrap();
    assert!(note.contains("image not sent to the model"), "{note}");
    assert!(note.contains("https://example.test/cat.png"), "{note}");
}

#[test]
fn inline_data_without_a_mime_type_is_announced_not_dropped() {
    let body = build_generate_body(&image_request(ImageSource::new("data:;base64,AAAB")));
    let note = body["contents"][0]["parts"][1]["text"].as_str().unwrap();
    assert!(note.contains("mimeType"), "{note}");
}

#[test]
fn capability_flag_matches_the_encoder() {
    let caps = GeminiAdapter::new().capabilities();
    assert!(caps.image_input);
    let body = build_generate_body(&image_request(ImageSource::new(
        "data:image/webp;base64,AAAB",
    )));
    assert!(body["contents"][0]["parts"][1].get("inlineData").is_some());
}
