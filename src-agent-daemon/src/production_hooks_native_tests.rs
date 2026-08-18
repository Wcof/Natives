//! Native Hook unit tests (extracted from `production_hooks.rs`, task-01
//! structure): MCP input-template substitution and the structured Prompt Hook
//! allow/deny decision.

use super::*;

fn install_agent_hook_test_manager() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("agent-hook.db");
    let artifacts = dir.path().join("artifacts");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db.clone()), Some(artifacts.clone()));
    let store = std::sync::Arc::new(crate::storage::DataStore::new(&db, &artifacts).unwrap());
    crate::run_manager::install_global_for_test(crate::run_manager::RunManager::new_with_store(
        store,
    ));
    dir
}

fn create_agent_hook_parent() -> assistant_protocol::v2::RunV2 {
    crate::global_run_manager()
        .create_run(assistant_protocol::v2::CreateRunRequest {
            conversation_id: "agent-hook-parent".into(),
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            key_id: Some("parent-key".into()),
            agent_profile_id: None,
            permission_profile: Some("ask".into()),
            content: Some("parent".into()),
            attachments: None,
            max_steps: Some(2),
            parent_run_id: None,
            project_path: std::env::var("NATIVES_RUNTIME_DIR").ok(),
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".into()),
            capability_selection: None,
            disabled_tools: None,
        })
        .unwrap()
}

#[test]
fn mcp_template_substitutes_structured_event_input() {
    let mut value = serde_json::json!({"payload": "${input}", "literal": "keep"});
    substitute_hook_input(&mut value, &serde_json::json!({"command": "cargo test"}));
    assert_eq!(value["payload"]["command"], "cargo test");
    assert_eq!(value["literal"], "keep");
}

#[test]
fn prompt_hook_requires_a_structured_allow_or_deny() {
    assert!(matches!(
        prompt_decision(r#"{"decision":"allow","reason":"safe"}"#),
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Allow
        })
    ));
    assert!(matches!(
        prompt_decision(r#"{"decision":"deny","reason":"unsafe"}"#),
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny { .. }
        })
    ));
    assert!(matches!(
        prompt_decision("maybe"),
        HookOutcome::Failed { .. }
    ));
}

#[tokio::test]
async fn agent_hook_records_an_independent_binding_in_child_session_and_run() {
    let _env_guard = crate::storage::DataStore::env_test_lock();
    let _env_restore = crate::storage::EnvRestore::capture();
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    let _dir = install_agent_hook_test_manager();
    let parent = create_agent_hook_parent();
    let child_binding = crate::subagent_store::RouteBinding {
        provider_id: "anthropic".into(),
        key_id: "child-key".into(),
        model_id: "claude".into(),
    };
    crate::subagent_store::upsert_route_policy(
        &parent.conversation_id,
        "default",
        &[
            crate::subagent_store::RouteBinding {
                provider_id: parent.provider_id.clone(),
                key_id: parent.key_id.clone().unwrap(),
                model_id: parent.model_id.clone(),
            },
            child_binding.clone(),
        ],
    )
    .unwrap();

    let outcome = NativeAgentHook {
        prompt: "Inspect only.".into(),
        model_override: None,
        max_steps: 1,
        readonly_tools: vec![],
        timeout_ms: 1_000,
    }
    .handle_outcome(HookRequest {
        event: harness_core::hooks::HookEvent::Notification,
        run_id: parent.id.clone(),
        tool_name: None,
        input: serde_json::json!({"event": "check"}),
    })
    .await;

    let session =
        crate::subagent_store::list_subagent_sessions(Some(&parent.conversation_id), true)
            .unwrap()
            .pop()
            .expect("child session");
    assert_eq!(session.provider_id, child_binding.provider_id);
    assert_eq!(session.key_id, child_binding.key_id);
    assert_eq!(session.model_id, child_binding.model_id);
    assert_ne!(session.key_id, parent.key_id.unwrap());

    let child = crate::global_run_manager()
        .list_runs(Some(&session.child_conversation_id))
        .pop()
        .expect("child run");
    assert!(
        matches!(outcome, HookOutcome::Decided(_)),
        "{outcome:?}; child error: {:?}",
        child.error_code
    );
    assert_eq!(child.provider_id, session.provider_id);
    assert_eq!(child.key_id.as_deref(), Some(session.key_id.as_str()));
    assert_eq!(child.model_id, session.model_id);
    crate::storage::set_test_db_override(None, None);
}

#[tokio::test]
async fn agent_hook_without_an_independent_route_fails_before_child_creation() {
    let _env_guard = crate::storage::DataStore::env_test_lock();
    let _env_restore = crate::storage::EnvRestore::capture();
    let _dir = install_agent_hook_test_manager();
    let parent = create_agent_hook_parent();

    let outcome = NativeAgentHook {
        prompt: "Inspect only.".into(),
        model_override: None,
        max_steps: 1,
        readonly_tools: vec![],
        timeout_ms: 1_000,
    }
    .handle_outcome(HookRequest {
        event: harness_core::hooks::HookEvent::Notification,
        run_id: parent.id.clone(),
        tool_name: None,
        input: serde_json::json!({}),
    })
    .await;

    assert!(matches!(outcome, HookOutcome::Failed { .. }));
    assert!(
        crate::subagent_store::list_subagent_sessions(Some(&parent.conversation_id), true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::global_run_manager()
            .list_runs(Some(&parent.conversation_id))
            .len(),
        1
    );
    crate::storage::set_test_db_override(None, None);
}
