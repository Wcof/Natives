use super::*;
use tokio::sync::oneshot;

#[tokio::test]
#[allow(deprecated)] // explicitly verifies the deprecated setter is a no-op
async fn legacy_runtime_profile_setter_cannot_mutate_shared_profile() {
    let rt = ProductionRuntime::new();
    rt.set_permission_profile("readonly").await;
    assert_eq!(
        rt.permissions.get_profile().await,
        PermissionProfile::ConfirmEach
    );
    rt.set_permission_profile("ask").await;
    assert_eq!(
        rt.permissions.get_profile().await,
        PermissionProfile::ConfirmEach
    );
    rt.set_permission_profile("full_access").await;
    assert_eq!(
        rt.permissions.get_profile().await,
        PermissionProfile::ConfirmEach
    );
}

#[tokio::test]
async fn rejects_mismatched_run_id() {
    // T01: hermetic fixture — durable pending interaction must exist before
    // respond_permission runs mark_resolved against the store.
    let _env_guard = crate::storage::DataStore::env_test_lock();
    let _env_restore = crate::storage::EnvRestore::capture();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("perm.db");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
    let _warm = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
    let cid = "c-mismatch";
    crate::conversation_store::ensure_conversation_stub(cid, "openai", "gpt-4o", None, None)
        .unwrap();
    crate::interaction_store::insert_pending(
        "p1",
        None,
        Some(cid),
        "tool_permission",
        serde_json::json!({"tool_name": "tool"}),
    )
    .unwrap();
    let rt = ProductionRuntime::new();
    let (tx, _rx) = oneshot::channel();
    rt.insert_permission_waiter("p1", "run-a", "tool", tx).await;
    let err = rt
        .respond_permission("p1", true, Some("run-b"), Some("once"))
        .await
        .unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
    // Still present for correct owner
    assert!(rt.interactions.has_permission("p1").await);
    let ok = rt
        .respond_permission("p1", false, Some("run-a"), Some("once"))
        .await;
    assert!(ok.is_ok());
    assert!(!rt.interactions.has_permission("p1").await);
}

#[test]
fn permission_scope_preserves_session_boundary() {
    assert_eq!(normalize_permission_scope("session"), "session");
    assert_eq!(normalize_permission_scope("this_run"), "this_run");
    assert_eq!(normalize_permission_scope("project"), "project");
}
