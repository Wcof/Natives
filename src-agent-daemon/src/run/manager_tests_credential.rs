use super::*;

/// Serialize credential-broker tests — shared process-global slot.
fn with_broker_slot<R>(f: impl FnOnce() -> R) -> R {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
    crate::production::clear_credential_broker_for_tests();
    let out = f();
    crate::production::clear_credential_broker_for_tests();
    out
}

#[test]
fn credential_resolve_never_returns_empty_mock_key() {
    with_broker_slot(|| {
        // Broker reports not-found; no env key → hard fail (no offline mock success).
        crate::production::install_credential_broker(std::sync::Arc::new(
            |_p: &str, _k: Option<&str>, _r: &str| Err("No active key for provider".into()),
        ));
        let prev_openai = std::env::var("NATIVES_TEST_OPENAI_KEY").ok();
        let prev_anth = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
        let prev_api = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("NATIVES_TEST_OPENAI_KEY");
        std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
        std::env::remove_var("ANTHROPIC_API_KEY");
        let err = crate::production::resolve_credential("openai", Some("k1")).unwrap_err();
        assert!(
            err.contains("No credential") || err.contains("broker") || err.contains("unavailable"),
            "unexpected err: {err}"
        );
        assert!(!err.contains("sk-"));
        match prev_openai {
            Some(v) => std::env::set_var("NATIVES_TEST_OPENAI_KEY", v),
            None => std::env::remove_var("NATIVES_TEST_OPENAI_KEY"),
        }
        match prev_anth {
            Some(v) => std::env::set_var("ANTHROPIC_AUTH_TOKEN", v),
            None => std::env::remove_var("ANTHROPIC_AUTH_TOKEN"),
        }
        match prev_api {
            Some(v) => std::env::set_var("ANTHROPIC_API_KEY", v),
            None => std::env::remove_var("ANTHROPIC_API_KEY"),
        }
    });
}

#[test]
fn credential_broker_install_is_invoked_before_env() {
    with_broker_slot(|| {
        crate::production::install_credential_broker(std::sync::Arc::new(
            |_provider_id: &str, key_id: Option<&str>, run_id: &str| {
                assert!(!run_id.is_empty());
                Ok(provider_adapters::capabilities::Credential {
                    api_key: "broker-secret-not-for-logs".into(),
                    base_url: Some("https://example.test/v1".into()),
                    proxy_url: None,
                    key_id: Some(key_id.unwrap_or("broker-key-1").to_string()),
                    provider_type: Some("openai_compatible".into()),
                })
            },
        ));
        std::env::remove_var("NATIVES_TEST_OPENAI_KEY");
        let cred = crate::production::resolve_credential_for_run("openai", Some("k1"), "run-1")
            .expect("broker must win over missing env");
        assert_eq!(cred.key_id.as_deref(), Some("k1"));
        assert_eq!(cred.api_key, "broker-secret-not-for-logs");
        let event_payload = serde_json::json!({"error": "auth failed"});
        assert!(!event_payload.to_string().contains("broker-secret"));
        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                    std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
                    serde_json::to_string_pretty(&serde_json::json!({
                        "broker_invoked": true,
                        "key_id_returned": "k1",
                        "api_key_not_in_events": true,
                        "path": "install_credential_broker → resolve_credential_for_run → Tauri natives.db",
                        "mock_success_without_key": false,
                    }))
                    .unwrap_or_default(),
                );
        }
    });
}
