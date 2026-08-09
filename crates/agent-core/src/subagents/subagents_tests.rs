//! Sub-agent manager unit tests (extracted from `subagents.rs`).

use super::*;

use super::*;

async fn spawn_default(
    manager: &SubAgentManager,
    parent: &str,
    task: &str,
    depth: u32,
) -> Result<SubAgent, String> {
    manager
        .spawn(
            parent,
            task.to_string(),
            depth,
            "openai".into(),
            format!("key-{}", uuid::Uuid::new_v4()),
            "gpt-4o".into(),
            "ask".into(),
            vec!["read_file".into()],
            None,
            None,
            None,
        )
        .await
}

#[tokio::test]
async fn test_spawn_sub_agent() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    let sub = spawn_default(&manager, "parent-1", "Test task", 1)
        .await
        .unwrap();
    assert_eq!(sub.parent_run_id, "parent-1");
    assert_eq!(sub.status, SubAgentStatus::Queued);
    assert!(!sub.run_id.is_empty());
    assert_eq!(sub.provider_id, "openai");
    assert!(!sub.key_id.is_empty());
    // Independent identity: not empty key/model
    assert_eq!(sub.model_id, "gpt-4o");
    assert_eq!(sub.permission_profile, "ask");
}

#[tokio::test]
async fn test_spawn_requires_key_identity() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    let err = manager
        .spawn(
            "parent-1",
            "x".into(),
            1,
            "openai".into(),
            "".into(),
            "gpt-4o".into(),
            "ask".into(),
            vec![],
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(err.contains("key_id"));
}

#[tokio::test]
async fn test_depth_limit() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    let result = spawn_default(&manager, "parent-1", "Deep task", 100).await;
    assert!(result.is_err(), "Should reject deep sub-agent");
}

#[tokio::test]
async fn test_concurrency_limit() {
    let config = SubAgentConfig {
        max_concurrent: 1,
        ..Default::default()
    };
    let manager = SubAgentManager::new(config);

    // Spawn first sub-agent
    let sub1 = spawn_default(&manager, "parent-1", "Task 1", 1)
        .await
        .unwrap();
    manager
        .update_status(&sub1.id, SubAgentStatus::Running)
        .await
        .unwrap();

    // Second spawn should fail due to concurrency limit
    let result = spawn_default(&manager, "parent-1", "Task 2", 1).await;
    assert!(result.is_err(), "Should reject concurrent sub-agent");
}

#[tokio::test]
async fn test_cascade_cancel() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    let a = spawn_default(&manager, "parent-1", "A", 1).await.unwrap();
    let b = spawn_default(&manager, "parent-1", "B", 1).await.unwrap();
    manager
        .update_status(&a.id, SubAgentStatus::Running)
        .await
        .unwrap();
    manager
        .update_status(&b.id, SubAgentStatus::Running)
        .await
        .unwrap();
    let n = manager.cascade_cancel("parent-1").await;
    assert_eq!(n, 2);
    assert_eq!(
        manager.get(&a.id).await.unwrap().status,
        SubAgentStatus::Cancelled
    );
}

#[tokio::test]
async fn test_get_children() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    spawn_default(&manager, "parent-1", "Child 1", 1)
        .await
        .unwrap();
    spawn_default(&manager, "parent-1", "Child 2", 1)
        .await
        .unwrap();

    let children = manager.get_children("parent-1").await;
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn test_parent_recovery_after_child_failure() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    let sub = spawn_default(&manager, "parent-1", "Failing task", 1)
        .await
        .unwrap();

    // Simulate child failure
    manager
        .update_status(&sub.id, SubAgentStatus::Failed("Error".to_string()))
        .await
        .unwrap();

    // Parent can still spawn new children
    let new_sub = spawn_default(&manager, "parent-1", "Recovery task", 1).await;
    assert!(new_sub.is_ok(), "Parent should recover after child failure");
}

#[tokio::test]
async fn test_list_sub_agents() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    spawn_default(&manager, "parent-1", "Task 1", 1)
        .await
        .unwrap();
    spawn_default(&manager, "parent-1", "Task 2", 1)
        .await
        .unwrap();
    assert_eq!(manager.list().await.len(), 2);
}

#[test]
fn test_cap_child_permission_never_upgrades() {
    // Parent ask: child cannot become full_access.
    assert_eq!(cap_child_permission("ask", "full_access"), "ask");
    assert_eq!(cap_child_permission("readonly", "ask"), "readonly");
    assert_eq!(cap_child_permission("readonly", "full_access"), "readonly");
    // Parent full_access: explicit full_access request allowed; default ask stays ask.
    assert_eq!(
        cap_child_permission("full_access", "full_access"),
        "full_access"
    );
    assert_eq!(cap_child_permission("full_access", "ask"), "ask");
    assert_eq!(cap_child_permission("full_access", "readonly"), "readonly");
    assert_eq!(cap_child_permission("autonomous", "full"), "full_access");
    // Unknown → ask floor.
    assert_eq!(cap_child_permission("ask", ""), "ask");
    assert_eq!(cap_child_permission("", "full_access"), "ask");
}

#[test]
fn test_default_subagent_tool_allowlist_is_readonly() {
    let list = default_subagent_tool_allowlist();
    assert!(list.contains(&"read_file".into()));
    assert!(list.contains(&"list_dir".into()));
    assert!(list.contains(&"grep".into()));
    assert!(!list
        .iter()
        .any(|t| t == "write_file" || t == "task" || t == "run_terminal"));
}

#[tokio::test]
async fn queued_occupies_concurrent_reservation() {
    let config = SubAgentConfig {
        max_concurrent: 1,
        max_concurrent_global: 1,
        ..Default::default()
    };
    let manager = SubAgentManager::new(config);
    let _a = spawn_default(&manager, "p", "A", 1).await.unwrap();
    // Still Queued — must block second spawn.
    assert_eq!(manager.active_reservation_count().await, 1);
    let err = spawn_default(&manager, "p", "B", 1).await.unwrap_err();
    assert!(err.contains("concurrent") || err.contains("Max"), "{err}");
}

#[tokio::test]
async fn batch_preflight_all_or_nothing() {
    let config = SubAgentConfig {
        max_concurrent_global: 2,
        max_concurrent: 2,
        ..Default::default()
    };
    let manager = SubAgentManager::new(config);
    // Preflight of 3 must fail without leaving reservations.
    let err = manager.reserve_batch("p", 3).await.unwrap_err();
    assert!(err.contains("concurrent") || err.contains("Max"), "{err}");
    assert_eq!(manager.active_reservation_count().await, 0);
    manager.reserve_batch("p", 2).await.unwrap();
    assert_eq!(manager.active_reservation_count().await, 2);
    manager.release_batch_reservation("p", 2).await;
    assert_eq!(manager.active_reservation_count().await, 0);
}

#[tokio::test]
async fn depth_increments_from_parent_chain() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    manager.register_root_depth("root").await;
    let c1 = spawn_default(&manager, "root", "L1", 0).await.unwrap();
    assert_eq!(c1.depth, 1);
    let c2 = manager
        .spawn(
            &c1.run_id,
            "L2".into(),
            0,
            "openai".into(),
            format!("key-{}", uuid::Uuid::new_v4()),
            "gpt-4o".into(),
            "ask".into(),
            vec!["read_file".into()],
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(c2.depth, 2);
}

#[tokio::test]
async fn tool_and_token_budgets_enforce() {
    let config = SubAgentConfig {
        max_tool_calls_per_child: 2,
        max_tokens_per_child: 10,
        max_tokens_per_tree: 15,
        ..Default::default()
    };
    let manager = SubAgentManager::new(config);
    manager.consume_tool_call("c1", "root").await.unwrap();
    manager.consume_tool_call("c1", "root").await.unwrap();
    let err = manager.consume_tool_call("c1", "root").await.unwrap_err();
    assert!(err.contains("tool-call"), "{err}");
    manager.settle_tokens("c1", "root", 10).await.unwrap();
    let err = manager.settle_tokens("c1", "root", 1).await.unwrap_err();
    assert!(err.contains("token"), "{err}");
}

#[tokio::test]
async fn parent_cycle_rejected() {
    let manager = SubAgentManager::new(SubAgentConfig::default());
    let err = manager
        .assert_no_parent_cycle("same", "same")
        .await
        .unwrap_err();
    assert!(err.contains("DEADLOCK"), "{err}");
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn tool_list_allows_exact_and_mcp_surface() {
    let list = owned(&["read_file", "mcp_call"]);
    assert!(tool_list_allows(&list, "read_file"));
    assert!(!tool_list_allows(&list, "write_file"));
    // `mcp_call` stands for the whole MCP surface.
    assert!(tool_list_allows(&list, "mcp__github__create_issue"));
    // Without it, only the exact MCP tool name is admitted.
    let exact = owned(&["mcp__github__create_issue"]);
    assert!(tool_list_allows(&exact, "mcp__github__create_issue"));
    assert!(!tool_list_allows(&exact, "mcp__github__delete_repo"));
    assert!(!tool_list_allows(&[], "read_file"));
}

#[test]
fn resolve_child_permission_profile_only_tightens() {
    // A profile declaring full_access does not elevate an unrequested child.
    assert_eq!(
        resolve_child_permission("full_access", None, Some("full_access")),
        "ask"
    );
    // Nor does it elevate past a readonly request.
    assert_eq!(
        resolve_child_permission("full_access", Some("readonly"), Some("full_access")),
        "readonly"
    );
    // A restrictive profile tightens an explicit full_access request.
    assert_eq!(
        resolve_child_permission("full_access", Some("full_access"), Some("readonly")),
        "readonly"
    );
    // No profile: the request stands, still capped by the parent.
    assert_eq!(
        resolve_child_permission("full_access", Some("full_access"), None),
        "full_access"
    );
    assert_eq!(resolve_child_permission("full_access", None, None), "ask");
}

#[test]
fn resolve_child_permission_parent_is_the_hard_ceiling() {
    // The headline escalation attempt: readonly parent, child asks for
    // full_access, and the chosen profile also declares full_access.
    assert_eq!(
        resolve_child_permission("readonly", Some("full_access"), Some("full_access")),
        "readonly"
    );
    assert_eq!(
        resolve_child_permission("ask", Some("full_access"), Some("full_access")),
        "ask"
    );
    // Unknown / blank parent floors at ask.
    assert_eq!(
        resolve_child_permission("", Some("full_access"), Some("full_access")),
        "ask"
    );
    assert_eq!(
        resolve_child_permission("   ", Some("full_access"), None),
        "ask"
    );
    // Blank request is treated as absent, not as an elevation.
    assert_eq!(
        resolve_child_permission("full_access", Some(""), None),
        "ask"
    );
}

#[test]
fn resolve_child_tool_allowlist_precedence() {
    let parent = owned(&["read_file", "grep", "write_file", "task"]);
    // Explicit request wins over the profile.
    assert_eq!(
        resolve_child_tool_allowlist(
            Some(&parent),
            Some(&owned(&["grep"])),
            Some(&owned(&["write_file"])),
            None
        ),
        owned(&["grep"])
    );
    // Profile tools apply when nothing is requested.
    assert_eq!(
        resolve_child_tool_allowlist(
            Some(&parent),
            None,
            Some(&owned(&["read_file", "grep"])),
            None
        ),
        owned(&["read_file", "grep"])
    );
    // Neither: inherit the parent surface.
    assert_eq!(
        resolve_child_tool_allowlist(Some(&parent), None, None, None),
        parent
    );
    // Unrestricted parent + nothing declared: readonly default floor.
    assert_eq!(
        resolve_child_tool_allowlist(None, None, None, None),
        default_subagent_tool_allowlist()
    );
    // An explicitly empty request is fail-closed, not "fall back to default".
    assert!(resolve_child_tool_allowlist(Some(&parent), Some(&[]), None, None).is_empty());
    // Duplicates collapse, order preserved.
    assert_eq!(
        resolve_child_tool_allowlist(
            None,
            Some(&owned(&["grep", "read_file", "grep"])),
            None,
            None
        ),
        owned(&["grep", "read_file"])
    );
}

#[test]
fn resolve_child_tool_allowlist_parent_is_the_hard_ceiling() {
    let parent = owned(&["read_file", "grep", "task"]);
    // Profile asking for a surface the parent lacks gets intersected down.
    assert_eq!(
        resolve_child_tool_allowlist(
            Some(&parent),
            None,
            Some(&owned(&["read_file", "write_file", "run_terminal"])),
            None
        ),
        owned(&["read_file"])
    );
    // Same for a directly requested surface.
    assert_eq!(
        resolve_child_tool_allowlist(
            Some(&parent),
            Some(&owned(&["run_terminal", "write_file", "apply_patch"])),
            None,
            None
        ),
        Vec::<String>::new()
    );
    // Profile disallowedTools subtract even when the parent would allow them.
    assert_eq!(
        resolve_child_tool_allowlist(
            Some(&parent),
            None,
            Some(&owned(&["read_file", "grep"])),
            Some(&owned(&["grep"]))
        ),
        owned(&["read_file"])
    );
    // MCP: a parent holding only `mcp_call` still admits a named MCP tool.
    let mcp_parent = owned(&["mcp_call"]);
    assert_eq!(
        resolve_child_tool_allowlist(
            Some(&mcp_parent),
            Some(&owned(&["mcp__github__create_issue", "read_file"])),
            None,
            None
        ),
        owned(&["mcp__github__create_issue"])
    );
    // Unrestricted parent: no ceiling, permission profile still gates calls.
    assert_eq!(
        resolve_child_tool_allowlist(None, Some(&owned(&["run_terminal"])), None, None),
        owned(&["run_terminal"])
    );
}

#[test]
fn failure_policy_parse() {
    assert_eq!(FailurePolicy::parse("isolate"), FailurePolicy::Isolate);
    assert_eq!(FailurePolicy::parse("fail_fast"), FailurePolicy::FailFast);
    assert_eq!(
        FailurePolicy::parse("require_all"),
        FailurePolicy::RequireAll
    );
    assert_eq!(FailurePolicy::parse("retry"), FailurePolicy::Retry);
}

/// T05: the failure policy must produce a concrete, testable parent-side
/// effect for every terminal child failure — the production watcher (and
/// nothing else) consumes this decision.
#[test]
fn failure_policy_effects_on_child_failure() {
    use ChildFailureEffect::*;
    // Isolate: parent observes the failure and continues — regardless of
    // sibling state or retries left.
    assert_eq!(FailurePolicy::Isolate.on_child_failed(true, 0), Isolate);
    assert_eq!(FailurePolicy::Isolate.on_child_failed(false, 3), Isolate);
    // FailFast: one failure fails the parent immediately, even while
    // siblings are still running.
    assert_eq!(
        FailurePolicy::FailFast.on_child_failed(false, 0),
        FailParent
    );
    assert_eq!(FailurePolicy::FailFast.on_child_failed(true, 0), FailParent);
    // RequireAll waits for every sibling to settle before failing the
    // parent (aggregate outcome).
    assert_eq!(FailurePolicy::RequireAll.on_child_failed(false, 0), Isolate);
    assert_eq!(
        FailurePolicy::RequireAll.on_child_failed(true, 0),
        FailParent
    );
    // Retry re-queues the child while retries remain; exhausting retries
    // isolates (parent continues, child is failed).
    assert_eq!(FailurePolicy::Retry.on_child_failed(true, 1), Retry);
    assert_eq!(FailurePolicy::Retry.on_child_failed(false, 1), Retry);
    assert_eq!(FailurePolicy::Retry.on_child_failed(true, 0), Isolate);
}
