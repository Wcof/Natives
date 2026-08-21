use super::*;

#[test]
fn proxy_requests_receive_unique_lease_identity() {
    let first = proxy_request_context();
    let second = proxy_request_context();
    assert!(first.run_id.starts_with("proxy-request:"));
    assert!(second.run_id.starts_with("proxy-request:"));
    assert_ne!(first.run_id, second.run_id);
}

#[test]
fn antigravity_provider_identity_overrides_generic_gemini_platform() {
    let account = Sub2ApiAccountCredential {
        id: "account-1".into(),
        provider_id: "antigravity".into(),
        platform: "gemini".into(),
        account_type: "oauth".into(),
        credentials: serde_json::json!({}),
        extra: serde_json::json!({}),
        priority: 0,
        concurrency: 1,
        expires_at: None,
        proxy_url: None,
        lease_id: "lease-1".into(),
        run_id: "run-1".into(),
    };
    assert_eq!(
        account_adapter_kind(&account),
        AccountAdapterKind::Antigravity
    );
}

#[test]
fn oauth_refresh_locks_are_shared_only_while_live() {
    let first = oauth_refresh_lock("account-lock-test").unwrap();
    let second = oauth_refresh_lock("account-lock-test").unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    drop(first);
    drop(second);
    let replacement = oauth_refresh_lock("account-lock-test").unwrap();
    assert_eq!(std::sync::Arc::strong_count(&replacement), 1);
}

#[test]
fn oauth_error_code_recognizes_terminal_invalid_grant_without_exposing_description() {
    assert_eq!(
        oauth_error_code(br#"{"error":"invalid_grant","error_description":"secret detail"}"#)
            .as_deref(),
        Some("invalid_grant")
    );
    assert_eq!(
        oauth_error_code(br#"{"error":{"code":"invalid_grant"}}"#).as_deref(),
        Some("invalid_grant")
    );
}

#[test]
fn circuit_opens_after_three_failures_and_recovers_after_success() {
    let _env_guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("route-health.db");
    let prev_db = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    let target = RouteTarget {
        provider_id: "p".into(),
        credential_kind: "api_key".into(),
        credential_id: Some("k".into()),
        model_id: "m".into(),
    };
    record_success(&target);
    record_failure(&target);
    record_failure(&target);
    assert!(!circuit_open(&target));
    record_failure(&target);
    assert!(circuit_open(&target));
    record_success(&target);
    assert!(!circuit_open(&target));
    match prev_db {
        Some(value) => std::env::set_var("NATIVES_ASSISTANT_DB_PATH", value),
        None => std::env::remove_var("NATIVES_ASSISTANT_DB_PATH"),
    }
}

#[test]
fn primary_credential_kind_falls_back_to_pool_for_oauth_only_provider() {
    assert_eq!(primary_credential_kind(None, true), "sub2api_pool");
    assert_eq!(primary_credential_kind(Some(""), true), "sub2api_pool");
    assert_eq!(primary_credential_kind(Some("k1"), true), "api_key");
    assert_eq!(primary_credential_kind(Some("k1"), false), "api_key");
    assert_eq!(primary_credential_kind(None, false), "api_key");
    assert_eq!(
        primary_credential_kind(Some("_primary_"), true),
        "sub2api_pool"
    );
}

fn account(id: &str, priority: i64) -> Sub2ApiAccountCredential {
    Sub2ApiAccountCredential {
        id: id.into(),
        provider_id: "p".into(),
        platform: "openai".into(),
        account_type: "oauth".into(),
        credentials: serde_json::json!({}),
        extra: serde_json::json!({}),
        priority,
        concurrency: 1,
        expires_at: None,
        proxy_url: None,
        lease_id: format!("lease-{id}"),
        run_id: format!("run-{id}"),
    }
}

#[test]
fn order_pool_accounts_prioritizes_lower_priority_first() {
    let mut accounts = vec![account("a", 5), account("b", 1), account("c", 3)];
    order_pool_accounts(&mut accounts, 0);
    let ids: Vec<&str> = accounts.iter().map(|account| account.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "c", "a"]);
}

#[test]
fn order_pool_accounts_rotates_equal_priority_tier_by_affinity() {
    let mut accounts = vec![account("a", 0), account("b", 0), account("c", 0)];
    order_pool_accounts(&mut accounts, 1);
    let ids: Vec<&str> = accounts.iter().map(|account| account.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "c", "a"], "affinity=1 rotates the top tier");

    let mut accounts = vec![account("a", 0), account("b", 0), account("c", 0)];
    order_pool_accounts(&mut accounts, 2);
    let ids: Vec<&str> = accounts.iter().map(|account| account.id.as_str()).collect();
    assert_eq!(ids, vec!["c", "a", "b"]);

    let mut accounts = vec![account("a", 0)];
    order_pool_accounts(&mut accounts, 999);
    assert_eq!(accounts[0].id, "a");
}

#[test]
fn configured_plan_retries_primary_then_secondary_and_is_bounded() {
    let primary = RouteTarget {
        provider_id: "primary".into(),
        credential_kind: "api_key".into(),
        credential_id: Some("k1".into()),
        model_id: "first".into(),
    };
    let secondary = RouteTarget {
        provider_id: "secondary".into(),
        credential_kind: "api_key".into(),
        credential_id: Some("k2".into()),
        model_id: "second".into(),
    };
    let plan = configured_plan(
        primary.clone(),
        true,
        vec![
            primary,
            secondary,
            RouteTarget {
                provider_id: "third".into(),
                credential_kind: "api_key".into(),
                credential_id: None,
                model_id: "third".into(),
            },
            RouteTarget {
                provider_id: "unreachable-fourth".into(),
                credential_kind: "api_key".into(),
                credential_id: None,
                model_id: "fourth".into(),
            },
        ],
    );

    assert_eq!(
        plan.attempts()
            .map(|target| target.provider_id.as_str())
            .collect::<Vec<_>>(),
        ["primary", "secondary", "third"]
    );
}

#[test]
fn timeout_errors_keep_timeout_taxonomy() {
    let EngineError::Provider { category, .. } = timeout_error("deadline") else {
        panic!("routing timeout must be a provider error");
    };
    assert_eq!(category, "Timeout");
}
