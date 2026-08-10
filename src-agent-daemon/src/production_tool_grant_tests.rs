use super::*;

/// Give each grant test its own temp store: `remember` persists non-once
/// grants durably, and the `check` DB fallback must never see another
/// test's grant (the tests share conversation/run keys).
async fn with_grant_rt<F, Fut>(f: F)
where
    F: FnOnce(Arc<ProductionRuntime>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let _guard = crate::storage::DataStore::env_test_lock();
    let _restore = crate::storage::EnvRestore::capture();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("grant.db");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
    crate::storage::set_test_db_override(Some(db), Some(dir.path().join("artifacts")));
    let rt = Arc::new(ProductionRuntime::new());
    f(rt).await;
    crate::storage::set_test_db_override(None, None);
}

#[tokio::test]
async fn project_grant_skips_second_ask() {
    with_grant_rt(|rt| async move {
        rt.remember_tool_grant("c1", "r1", "write_file", "", "project")
            .await;
        assert!(rt.has_tool_grant("c1", "r1", "write_file", "").await);
        assert!(rt.has_tool_grant("c1", "r2", "write_file", "").await);
        assert!(!rt.has_tool_grant("c1", "r1", "run_terminal", "ls").await);
    })
    .await;
}

#[tokio::test]
async fn this_run_grant_only_same_run() {
    with_grant_rt(|rt| async move {
        rt.remember_tool_grant("c1", "r1", "apply_patch", "", "this_run")
            .await;
        assert!(rt.has_tool_grant("c1", "r1", "apply_patch", "").await);
        assert!(!rt.has_tool_grant("c1", "r2", "apply_patch", "").await);
    })
    .await;
}

#[tokio::test]
async fn once_does_not_persist() {
    with_grant_rt(|rt| async move {
        rt.remember_tool_grant("c1", "r1", "write_file", "", "once")
            .await;
        assert!(!rt.has_tool_grant("c1", "r1", "write_file", "").await);
    })
    .await;
}

#[tokio::test]
async fn terminal_pattern_is_honored() {
    with_grant_rt(|rt| async move {
        rt.remember_tool_grant("c1", "r1", "run_terminal", "cargo test", "project")
            .await;
        assert!(
            rt.has_tool_grant("c1", "r1", "run_terminal", "cargo test")
                .await
        );
        assert!(
            !rt.has_tool_grant("c1", "r1", "run_terminal", "rm -rf /")
                .await
        );
    })
    .await;
}
