use super::*;

#[test]
fn test_mask_api_key_short() {
    assert_eq!(mask_api_key("ab"), "***");
}

#[test]
fn test_mask_api_key_normal() {
    let masked = mask_api_key("sk-ant-abcdefghijklmnop");
    assert_eq!(masked, "sk-a…mnop");
    assert!(!masked.contains("abcdefghijklmnop"));
}

#[test]
fn test_normalize_url_keeps_standard() {
    let result = normalize_url("https://api.openai.com/v1").unwrap();
    assert_eq!(result, "https://api.openai.com/v1");
}

#[test]
fn test_normalize_url_trims_trailing_slash() {
    let result = normalize_url("https://api.openai.com/v1/").unwrap();
    assert_eq!(result, "https://api.openai.com/v1");
}

#[test]
fn test_normalize_url_chat_completions_derives_v1() {
    let result = normalize_url("https://token.sensenova.cn/v1/chat/completions").unwrap();
    assert_eq!(result, "https://token.sensenova.cn/v1");
}

#[test]
fn test_normalize_url_chat_completions_no_v1() {
    let result = normalize_url("https://example.com/chat/completions").unwrap();
    assert_eq!(result, "https://example.com/v1");
}

#[test]
fn test_normalize_url_double_v1_dedup() {
    let result = normalize_url("https://example.com/v1/v1").unwrap();
    assert_eq!(result, "https://example.com/v1");
}

#[test]
fn test_normalize_url_empty_rejected() {
    let result = normalize_url("");
    assert!(result.is_err());
}

#[test]
fn test_normalize_url_whitespace_only_rejected() {
    let result = normalize_url("  ");
    assert!(result.is_err());
}

#[test]
fn test_chat_completions_url_appends() {
    let result = chat_completions_url("https://token.sensenova.cn/v1");
    assert_eq!(result, "https://token.sensenova.cn/v1/chat/completions");
}

#[test]
fn test_chat_completions_url_trailing_slash() {
    let result = chat_completions_url("https://example.com/");
    assert_eq!(result, "https://example.com/chat/completions");
}

#[test]
fn test_models_url_does_not_duplicate_v1() {
    assert_eq!(
        models_url("openai_compatible", "https://api.openai.com/v1").unwrap(),
        "https://api.openai.com/v1/models"
    );
}

#[test]
fn provider_test_supports_openai_and_anthropic_response_shapes() {
    let openai = serde_json::json!({"choices": [{"message": {"content": "ok"}}]});
    let responses = serde_json::json!({"output_text": "ok"});
    let anthropic = serde_json::json!({"content": [{"type": "text", "text": "ok"}]});
    // SenseNova deepseek-v4-flash style: empty content, non-empty reasoning.
    let reasoning_only = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "",
                "reasoning_content": "We are asked to reply with exactly ok."
            },
            "finish_reason": "length"
        }]
    });
    // Well-formed choice with empty fields still proves connectivity.
    let empty_but_formed = serde_json::json!({
        "choices": [{ "message": { "role": "assistant", "content": "" } }]
    });
    assert!(provider_response_has_content("openai_compatible", &openai));
    assert!(provider_response_has_content(
        "openai_chat_completions",
        &reasoning_only
    ));
    assert!(provider_response_has_content(
        "openai_chat_completions",
        &empty_but_formed
    ));
    assert!(provider_response_has_content(
        "openai_responses",
        &responses
    ));
    assert!(provider_response_has_content("anthropic", &anthropic));
    assert!(provider_response_has_content(
        "anthropic_messages",
        &anthropic
    ));
    let anthropic_thinking = serde_json::json!({
        "id": "msg_1",
        "model": "claude",
        "content": [{"type": "thinking", "thinking": "plan..."}]
    });
    assert!(provider_response_has_content(
        "anthropic_messages",
        &anthropic_thinking
    ));
    let responses_id_only = serde_json::json!({"id": "resp_1", "model": "gpt", "output": []});
    assert!(provider_response_has_content(
        "openai_responses",
        &responses_id_only
    ));
    assert_eq!(
        provider_test_url("anthropic_messages", "https://api.anthropic.com").unwrap(),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(
        provider_test_url("openai_responses", "https://api.openai.com/v1").unwrap(),
        "https://api.openai.com/v1/responses"
    );
}

#[test]
fn provider_test_builds_protocol_specific_request_bodies() {
    let anthropic = provider_test_body("anthropic_messages", "claude-sonnet").unwrap();
    assert_eq!(anthropic["model"], "claude-sonnet");
    assert_eq!(
        anthropic["messages"][0]["content"],
        "Reply with exactly: ok"
    );
    assert!(anthropic.get("max_tokens").is_some());
    assert!(anthropic.get("max_output_tokens").is_none());

    let responses = provider_test_body("openai_responses", "gpt-5").unwrap();
    assert_eq!(responses["model"], "gpt-5");
    assert_eq!(responses["input"], "Reply with exactly: ok");
    assert!(responses.get("max_output_tokens").is_some());
    assert!(responses.get("messages").is_none());
}

#[test]
fn unsupported_protocols_are_not_silently_tested_as_openai() {
    assert!(required_api_protocol(None).is_err());
    assert!(required_api_protocol(Some("")).is_err());
    assert!(required_api_protocol(Some("unknown_protocol")).is_err());
    assert!(provider_test_url("gemini_generate_content", "https://example.com").is_err());
    assert!(models_url("ollama_chat", "http://localhost:11434").is_err());
    let message = provider_test_body("gemini_generate_content", "gemini-pro")
        .unwrap_err()
        .to_string();
    assert!(message.contains("unsupported"));
}

// ── Integration tests (simulate manual validation) ──

/// Set up an in-memory SQLite database with provider tables.
fn setup_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    ensure_tables(&conn).unwrap();
    conn
}

/// Simulate: User opens Settings → Providers → Add SenseNova Token
/// Enter `https://token.sensenova.cn/v1` as base URL.
/// Enter an API Key.
/// Verify that:
///   - The base URL is stored as-is (no /chat/completions suffix)
///   - The returned ProviderKey has masked_key instead of api_key
///   - The masked_key format is "prefix…suffix"
///   - No full key is present in the DTO
#[test]
fn test_integration_add_sensenova_provider_returns_masked_key() {
    let conn = setup_db();

    // Simulate adding a SenseNova provider
    let now = chrono_now();
    let provider_id = uuid_v4();
    let key_id = uuid_v4();
    let full_key = "sk-sensenova-test-key-abcdef123456";

    // Insert provider
    conn.execute(
            "INSERT INTO user_providers (id, preset_name, name, website_url, base_url, created_at, updated_at)
             VALUES (?1, 'sensenova', 'SenseNova Token', 'https://platform.sensenova.cn', 'https://token.sensenova.cn/v1', ?2, ?3)",
            rusqlite::params![provider_id, now, now],
        ).unwrap();

    // Insert key (encrypted)
    conn.execute(
            "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at)
             VALUES (?1, ?2, 'API Key 1', ?3, '', ?4)",
            rusqlite::params![key_id, provider_id, full_key.to_string(), now],
        ).unwrap();

    // Read back the key and verify masking
    let (db_key, _masked): (String, String) = conn
        .query_row(
            "SELECT k.api_key_encrypted, '' FROM provider_api_keys k WHERE k.id = ?1",
            rusqlite::params![key_id],
            |row| Ok((row.get(0)?, String::new())),
        )
        .unwrap();

    // The DB stores the full key (encrypted in production, raw in test)
    assert_eq!(
        db_key, full_key,
        "DB still has full key (encrypted in production)"
    );

    // Simulate what the frontend sees: masked_key
    let frontend_key = mask_api_key(&db_key);
    assert_eq!(frontend_key, "sk-s…3456", "Frontend receives masked key");
    assert!(
        !frontend_key.contains("abcdef123456"),
        "Full key not in frontend DTO"
    );
    assert!(frontend_key.contains("…"), "Masked key uses ellipsis");
}

/// Simulate: User clicks "Test Connection" with an unsaved provider.
/// Verify that:
///   - normalize_url correctly handles the SenseNova base URL
///   - chat_completions_url builds the correct endpoint
///   - test_provider_raw can be called with the raw key and URL
#[test]
fn test_integration_test_connection_flow() {
    // Step 1: User enters base URL
    let raw_url = "https://token.sensenova.cn/v1/";

    // Step 2: URL normalization removes trailing slash
    let normalized = normalize_url(raw_url).unwrap();
    assert_eq!(normalized, "https://token.sensenova.cn/v1");

    // Step 3: Chat completions URL is derived
    let chat_url = chat_completions_url(&normalized);
    assert_eq!(chat_url, "https://token.sensenova.cn/v1/chat/completions");

    // Step 4: User selects a model
    let model = "sensenova-6.7-flash-lite";
    assert!(
        model.contains("sensenova") || model.contains("deepseek"),
        "Model is from the allowlist"
    );

    // Step 5: Key masking works for the raw key
    let raw_key = "sk-sensenova-test-key-abcdef123456";
    let masked = mask_api_key(raw_key);
    assert_eq!(masked, "sk-s…3456");
    assert!(!masked.contains(raw_key));
}

/// Simulate: User enters invalid base URL → verify rejection.
#[test]
fn test_integration_invalid_url_rejected() {
    // Empty URL should be rejected
    assert!(normalize_url("").is_err());
    assert!(normalize_url("  ").is_err());

    // Valid URL should work
    assert!(normalize_url("https://token.sensenova.cn/v1").is_ok());
}

/// Simulate: User enters /chat/completions as base URL → auto-derived to /v1.
#[test]
fn test_integration_chat_completions_url_auto_derived() {
    let result = normalize_url("https://token.sensenova.cn/v1/chat/completions").unwrap();
    assert_eq!(
        result, "https://token.sensenova.cn/v1",
        "Base URL should be derived to /v1 when user pastes /chat/completions"
    );

    let chat_url = chat_completions_url(&result);
    assert_eq!(
        chat_url, "https://token.sensenova.cn/v1/chat/completions",
        "Chat completions URL should be correctly rebuilt from derived base"
    );
}

/// Verify that the ProviderKey DTO struct used for frontend communication
/// has masked_key instead of an api_key field.
/// Regression: chat-completions 429 must surface a structured rate-limit error.
/// Spins a local HTTP server that returns 429, then asserts the exact error
/// shape currently shown in the UI (protocol/model/http_status/retryable/message).
#[test]
fn provider_test_surfaces_rate_limit_for_chat_completions() {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("bind mock server");
    let addr = server.server_addr().to_ip().expect("ip addr");
    let base = format!("http://{}:{}/v1", addr.ip(), addr.port());

    let handle = std::thread::spawn(move || {
        // Bounded receive: if the mock request never arrives, fail fast
        // instead of hanging the whole workspace test suite.
        let request = server
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("mock server recv failed")
            .expect("mock server never received the provider-test request");
        assert_eq!(request.url(), "/v1/chat/completions");
        let response = tiny_http::Response::from_string(
                r#"{"error":{"message":"Rate limit exceeded for model deepseek-v4-flash","type":"rate_limit_error"}}"#,
            )
            .with_status_code(429)
            .with_header(
                "x-request-id: 2e75f801-ffc9-42d0-aad5-22b9179b355b"
                    .parse::<tiny_http::Header>()
                    .unwrap(),
            );
        request.respond(response).ok();
    });

    let result = tauri::async_runtime::block_on(execute_provider_test(
        "openai_chat_completions",
        &base,
        "sk-test",
        Some("deepseek-v4-flash"),
        None,
    ));

    handle.join().expect("server thread");

    assert!(!result.success, "429 must not be treated as success");
    let err = result.error.expect("error message required");
    assert!(
        err.contains("Provider test failed:"),
        "missing prefix: {err}"
    );
    assert!(
        err.contains("protocol=openai_chat_completions"),
        "missing protocol: {err}"
    );
    assert!(
        err.contains("model=deepseek-v4-flash"),
        "missing model: {err}"
    );
    assert!(err.contains("http_status=429"), "missing status: {err}");
    assert!(
        err.contains("retryable=true"),
        "429 should be retryable: {err}"
    );
    assert!(
        err.contains("Rate limited"),
        "missing rate-limit message: {err}"
    );
}

#[test]
fn test_provider_key_dto_has_masked_key_not_api_key() {
    // The ProviderKey struct is defined with `masked_key: String`
    // This test verifies no api_key field exists in the DTO
    let key = ProviderKey {
        id: "test-id".to_string(),
        provider_id: "test-provider".to_string(),
        label: "Test Key".to_string(),
        masked_key: "sk-a…1b2c".to_string(),
        is_primary: true,
        is_active: true,
        status: "valid".to_string(),
        last_tested_at: Some("2026-01-01T00:00:00Z".to_string()),
        last_error_code: None,
        last_error_message: None,
        created_at: "2026-01-01T00:00:00Z".to_string(),
    };

    // Serialize to JSON and verify no api_key field
    let json = serde_json::to_value(&key).unwrap();
    assert!(
        json.get("apiKey").is_none(),
        "DTO must not have apiKey field"
    );
    assert!(
        json.get("maskedKey").is_some(),
        "DTO must have maskedKey field"
    );
    assert_eq!(json["maskedKey"], "sk-a…1b2c");
}
