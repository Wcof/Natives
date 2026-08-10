use super::*;
use agent_core::{cap_child_permission, EngineToolRuntime, SubAgentConfig, SubAgentManager};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn gated_with_allowlist(list: Option<Vec<String>>) -> PermissionGatedTools {
    let rt = ProductionRuntime::new();
    PermissionGatedTools {
        gateway: {
            let mut g = CapabilityGateway::new();
            g.set_project_root("/tmp/natives-allowlist-test");
            let _ = g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        interactions: rt.interactions.clone(),
        subagents: Arc::new(SubAgentManager::new(SubAgentConfig::default())),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: None,
        provider_id: "openai".into(),
        key_id: None,
        parent_run_id: "allowlist-parent".into(),
        conversation_id: "c-allow".into(),
        model_id: "m".into(),
        permission_profile: "full_access".into(),
        tool_allowlist: list,
        team: None,
        mcp_tool_schemas: Vec::new(),
        selected_mcp_servers: None,
    }
}

#[tokio::test]
async fn allowlist_hides_and_denies_tools_outside_list() {
    let tools = gated_with_allowlist(Some(vec![
        "read_file".into(),
        "list_dir".into(),
        "grep".into(),
    ]));
    let names: Vec<_> = tools
        .list_tool_schemas()
        .await
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(names.contains(&"read_file".into()));
    assert!(!names
        .iter()
        .any(|n| n == "write_file" || n == "task" || n == "run_terminal"));

    let denied = tools
        .execute_tool(
            "write_file",
            serde_json::json!({"path":"/tmp/x","content":"y"}),
            &CancellationToken::new(),
        )
        .await;
    assert!(denied.is_error);
    assert_eq!(denied.output.get("denied"), Some(&serde_json::json!(true)));
    assert_eq!(
        denied.output.get("code").and_then(|v| v.as_str()),
        Some("tool_not_allowlisted")
    );

    // task must not escalate unless allowlisted.
    let task_denied = tools
        .execute_tool(
            "task",
            serde_json::json!({"prompt":"nope","key_id":"k"}),
            &CancellationToken::new(),
        )
        .await;
    assert!(task_denied.is_error);
    assert_eq!(
        task_denied.output.get("denied"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn empty_allowlist_denies_all_tools() {
    let tools = gated_with_allowlist(Some(vec![]));
    assert!(tools.list_tool_schemas().await.is_empty());
    let denied = tools
        .execute_tool(
            "read_file",
            serde_json::json!({"path":"Cargo.toml"}),
            &CancellationToken::new(),
        )
        .await;
    assert!(denied.is_error);
    assert_eq!(denied.output.get("denied"), Some(&serde_json::json!(true)));
    let write_denied = tools
        .execute_tool(
            "write_file",
            serde_json::json!({"path":"/tmp/x","content":"y"}),
            &CancellationToken::new(),
        )
        .await;
    assert!(write_denied.is_error);
    assert_eq!(
        write_denied.output.get("denied"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn parent_none_allowlist_exposes_full_surface() {
    let tools = gated_with_allowlist(None);
    let names: Vec<_> = tools
        .list_tool_schemas()
        .await
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(names.len() > 5);
    assert!(names.iter().any(|n| n == "write_file"));
    assert!(names.iter().any(|n| n == "task"));
    // Draft tools are opt-in: the default surface must not carry them.
    assert!(!names.iter().any(|n| n == "write_draft_module"));
}

/// ADR-0014 invariant #3: the creative surface is exactly the four draft
/// tools, and no general write tool rides along.
#[test]
fn creative_surface_registers_only_draft_tools() {
    let allowlist = builtin_surface_allowlist(CREATIVE_DRAFT_AGENT_KIND).expect("built-in surface");
    let mut gateway = CapabilityGateway::new();
    register_tools_for_surface(&mut gateway, Some(&allowlist));

    let mut names: Vec<&str> = gateway.list_tools().into_iter().map(|t| t.name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = capability_gateway::tools::CREATIVE_DRAFT_TOOL_NAMES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);

    for banned in ["write_file", "edit_file", "apply_patch", "run_terminal"] {
        assert!(gateway.get_tool(banned).is_none(), "{banned} leaked in");
    }
}

#[test]
fn unknown_agent_kind_keeps_the_existing_fallback() {
    assert!(builtin_surface_allowlist("general").is_none());
    assert!(builtin_surface_allowlist("").is_none());
}

#[tokio::test]
async fn execute_task_caps_child_permission_and_sets_allowlist() {
    let _env_guard = crate::storage::DataStore::env_test_lock();
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    let tools = gated_with_allowlist(None);
    // Parent is full_access in helper; request full_access is allowed.
    let ok = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "child",
                "provider_id": "anthropic",
                "model_id": "claude",
                "key_id": "child-key",
                "permission_profile": "full_access",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!ok.is_error, "{:?}", ok.output);
    assert_eq!(
        ok.output.get("permission_profile").and_then(|v| v.as_str()),
        Some("full_access")
    );
    let task_id = ok
        .output
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let child = tools.subagents.get(&task_id).await.expect("child record");
    assert_eq!(
        child.tool_allowlist,
        agent_core::default_subagent_tool_allowlist()
    );

    // Default request (omit profile) stays ask even under full_access parent.
    let defaulted = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "child-default",
                "provider_id": "anthropic",
                "model_id": "claude",
                "key_id": "child-key-2",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!defaulted.is_error, "{:?}", defaulted.output);
    assert_eq!(
        defaulted
            .output
            .get("permission_profile")
            .and_then(|v| v.as_str()),
        Some("ask")
    );

    // Parent ask: cannot upgrade to full_access (cap before spawn).
    // Use full_access permission_profile field on tools so task gate doesn't block,
    // but cap_child_permission still sees parent profile "ask" via a custom field?
    // We call the helper directly for the pure cap assertion, and use spawn path
    // with parent tools.permission_profile = ask under autonomous class by temporarily
    // using full_access for the parent tools gate while asserting cap_child_permission.
    assert_eq!(cap_child_permission("ask", "full_access"), "ask");
    assert_eq!(cap_child_permission("readonly", "full_access"), "readonly");

    // Empty allowlist on task input is fail-closed for the child record.
    let empty = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "child-empty",
                "provider_id": "anthropic",
                "model_id": "claude",
                "key_id": "child-key-3",
                "tool_allowlist": [],
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!empty.is_error, "{:?}", empty.output);
    let empty_id = empty
        .output
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap();
    let empty_child = tools.subagents.get(empty_id).await.unwrap();
    assert!(empty_child.tool_allowlist.is_empty());

    // readonly parent denies Process side-effect of task before spawn.
    let mut tools_ro = gated_with_allowlist(None);
    tools_ro.permission_profile = "readonly".into();
    let capped = tools_ro
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "child",
                "provider_id": "anthropic",
                "model_id": "claude",
                "key_id": "child-key",
                "permission_profile": "full_access",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(capped.is_error);
    assert_eq!(capped.output.get("denied"), Some(&serde_json::json!(true)));
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");
}

#[tokio::test]
async fn execute_task_ignores_model_key_id_auto_when_policy_exists() {
    let _env_guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir
        .path()
        .join(format!("task-auto-{}.db", uuid::Uuid::new_v4()));
    let art = dir.path().join("artifacts");
    crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
    let _warm = crate::storage::DataStore::new(&db, &art).expect("migrate");
    crate::conversation_store::ensure_conversation_stub(
        "c-auto",
        "openai",
        "gpt-4o",
        Some("full_access"),
        None,
    )
    .unwrap();
    crate::subagent_store::upsert_route_policy(
        "c-auto",
        "default",
        &[crate::subagent_store::RouteBinding {
            provider_id: "anthropic".into(),
            key_id: "policy-key".into(),
            model_id: "claude".into(),
        }],
    )
    .unwrap();

    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    let mut tools = gated_with_allowlist(None);
    tools.conversation_id = "c-auto".into();
    tools.permission_profile = "full_access".into();
    let out = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "ignore-auto",
                "provider_id": "evil-provider",
                "key_id": "auto",
                "model_id": "evil-model",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!out.is_error, "{:?}", out.output);
    assert_eq!(out.output["key_id"], "policy-key");
    assert_eq!(out.output["provider_id"], "anthropic");
    assert_eq!(out.output["model_id"], "claude");
    // Real conversation id (not sub-* pseudo).
    let cid = out.output["conversation_id"].as_str().unwrap_or("");
    assert!(!cid.is_empty());
    assert!(!cid.starts_with("sub-"));
    assert!(!cid.starts_with("subagent-"));
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");
    crate::storage::set_test_db_override(None, None);
}

#[test]
fn cap_child_permission_unit() {
    assert_eq!(cap_child_permission("ask", "full_access"), "ask");
    assert_eq!(cap_child_permission("readonly", "ask"), "readonly");
    assert_eq!(cap_child_permission("full_access", "ask"), "ask");
}
