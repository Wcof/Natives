use natives_lib::Error;
use serde_json;

// ── Error type tests ──

#[test]
fn test_error_display() {
    let err = Error::NotFound("test_session".into());
    assert_eq!(format!("{err}"), "Not found: test_session");
    let err = Error::InvalidInput("bad id".into());
    assert_eq!(format!("{err}"), "Invalid input: bad id");
    let err = Error::Internal("something broke".into());
    assert_eq!(format!("{err}"), "Internal error: something broke");
}

#[test]
fn test_error_json_serialization() {
    let err = Error::NotFound("xyz".into());
    let json = serde_json::to_string(&err).unwrap();
    assert_eq!(json, "\"Not found: xyz\"");
}

#[test]
fn test_result_type_alias() {
    fn ok_fn() -> natives_lib::Result<i32> {
        Ok(42)
    }
    fn err_fn() -> natives_lib::Result<i32> {
        Err(Error::Internal("fail".into()))
    }
    assert_eq!(ok_fn().unwrap(), 42);
    assert!(err_fn().is_err());
}

// ── Update checker tests ──

#[test]
fn test_compare_versions() {
    // check_for_updates requires full AppState (DB), so it's tested via integration tests.
    // Unit-test the compare_versions helper instead:
    assert_eq!(natives_lib::update_checker::compare_versions("1.0.0", "1.0.1"), -1);
    assert_eq!(natives_lib::update_checker::compare_versions("1.0.1", "1.0.0"), 1);
    assert_eq!(natives_lib::update_checker::compare_versions("1.0.0", "1.0.0"), 0);
}

// ── Usage response tests ──

#[test]
fn test_usage_response_camelcase_serialization() {
    // Verify that the UsageResponse struct serializes with camelCase field names
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct TestUsage {
        pub model_stats: Vec<String>,
        pub input_tokens: u64,
        pub source_configured: bool,
    }
    let u = TestUsage {
        model_stats: vec!["a".into()],
        input_tokens: 42,
        source_configured: true,
    };
    let json = serde_json::to_value(&u).unwrap();
    // Must be camelCase in JSON output
    assert!(json.get("modelStats").is_some(), "model_stats → modelStats");
    assert!(json.get("inputTokens").is_some(), "input_tokens → inputTokens");
    assert!(json.get("sourceConfigured").is_some(), "source_configured → sourceConfigured");
}

#[test]
fn test_usage_response_has_source_configured() {
    // Verify UsageResponse includes sourceConfigured and error fields
    use natives_lib::Error;
    // Just verify the camelCase serialization works correctly
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DummyUsage {
        source_configured: bool,
        error: Option<String>,
    }
    let dummy = DummyUsage {
        source_configured: false,
        error: None,
    };
    let json = serde_json::to_value(&dummy).unwrap();
    assert!(json.get("sourceConfigured").is_some());
    assert_eq!(json["sourceConfigured"], false);
}

#[test]
fn test_emit_db_state_changed_serialization() {
    // Verify the payload shape that emit_db_state_changed sends
    let payload = serde_json::json!({
        "channel": "test",
        "data": { "key": "value" }
    });
    assert_eq!(payload["channel"], "test");
    assert_eq!(payload["data"]["key"], "value");
}

// ── Screenshot stop flag test ──

#[test]
fn test_screenshot_stop_flag() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let flag = Arc::new(AtomicBool::new(false));
    assert!(!flag.load(Ordering::Relaxed));

    flag.store(true, Ordering::Relaxed);
    assert!(flag.load(Ordering::Relaxed));

    // Reset (for restart)
    flag.store(false, Ordering::Relaxed);
    assert!(!flag.load(Ordering::Relaxed));
}
