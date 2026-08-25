//! Apps 域综合集成测试（APP-070 / 最终 Gate）。

use super::capabilities::CapabilityResolver;
use super::model::{AppRuntimeState, RegistrationOrigin};
use super::mutation_lock::MutationLockRegistry;
use super::repository::AppRepository;
use rusqlite::Connection;
use std::sync::Arc;

fn v28_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::db::create_tables(&conn).unwrap();
    crate::db::apply_migrations(&conn).unwrap();
    conn
}

#[test]
fn test_three_app_kinds_registration_and_views() {
    let conn = v28_db();

    // 1. Local Project
    let local_id = AppRepository::register_local(
        &conn,
        "My Local App",
        "/Users/test/local-app",
        Some("A local project"),
        None,
        RegistrationOrigin::Manual,
    )
    .unwrap();
    let local_view = AppRepository::view(&conn, &local_id).unwrap();
    assert_eq!(local_view.kind, "local_project");
    assert_eq!(local_view.title, "My Local App");

    // 2. System Application
    let sys_id = AppRepository::register_system(
        &conn,
        "Terminal",
        "/System/Applications/Utilities/Terminal.app",
        Some("com.apple.Terminal"),
        "macos",
        Some("activate_existing"),
        RegistrationOrigin::SystemDiscovery,
    )
    .unwrap();
    let sys_view = AppRepository::view(&conn, &sys_id).unwrap();
    assert_eq!(sys_view.kind, "system_application");
    let sys_spec = AppRepository::load_system_spec(&conn, &sys_id).unwrap();
    assert_eq!(
        sys_spec.bundle_identifier.as_deref(),
        Some("com.apple.Terminal")
    );

    // 3. Web Application
    let web_id = AppRepository::register_web(
        &conn,
        "Claude",
        "https://claude.ai",
        &["claude.ai".to_string(), "anthropic.com".to_string()],
        Some("native_webview"),
        true,
    )
    .unwrap();
    let web_view = AppRepository::view(&conn, &web_id).unwrap();
    assert_eq!(web_view.kind, "web_application");
    let web_spec = AppRepository::load_web_spec(&conn, &web_id).unwrap();
    assert_eq!(web_spec.url, "https://claude.ai");
    assert!(web_spec.keep_alive);
    assert_eq!(web_spec.approved_origins.len(), 2);

    // List all
    let all = AppRepository::list(&conn).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn test_capabilities_matrix_discipline() {
    // 1. Web Application: can_stop and can_restart must always be false
    let web_caps = CapabilityResolver::resolve("web_application", AppRuntimeState::Running, true);
    assert_eq!(web_caps.can_open, true);
    assert_eq!(web_caps.can_stop, false, "Web must never have can_stop");
    assert_eq!(
        web_caps.can_restart, false,
        "Web must never have can_restart"
    );
    assert_eq!(web_caps.can_edit, true);
    assert_eq!(web_caps.can_remove, true);

    // 2. Local Project: running has start, stop, restart, open
    let local_caps = CapabilityResolver::resolve("local_project", AppRuntimeState::Running, false);
    assert_eq!(local_caps.can_start, true);
    assert_eq!(local_caps.can_stop, true);
    assert_eq!(local_caps.can_restart, true);
    assert_eq!(local_caps.can_open, true);
    assert_eq!(local_caps.risk_level, 1);

    // 3. System Application: running has stop, restart, open
    let sys_caps =
        CapabilityResolver::resolve("system_application", AppRuntimeState::Running, true);
    assert_eq!(sys_caps.can_open, true);
    assert_eq!(sys_caps.can_stop, true);
    assert_eq!(sys_caps.can_restart, true);
    assert_eq!(sys_caps.risk_level, 2);
}

#[test]
fn test_remove_application_safety_and_cascade() {
    let conn = v28_db();
    let app_id =
        AppRepository::register_web(&conn, "Site", "https://example.com", &[], None, false)
            .unwrap();

    assert!(AppRepository::get(&conn, &app_id).unwrap().is_some());
    assert!(AppRepository::load_web_spec(&conn, &app_id).is_ok());

    let removed = AppRepository::remove_application(&conn, &app_id).unwrap();
    assert!(removed);

    // Metadata is cascaded
    assert!(AppRepository::get(&conn, &app_id).unwrap().is_none());
    assert!(AppRepository::load_web_spec(&conn, &app_id).is_err());

    // Repeating remove is idempotent false
    assert!(!AppRepository::remove_application(&conn, &app_id).unwrap());
}

#[tokio::test]
async fn test_mutation_lock_registry_concurrency() {
    let locks = Arc::new(MutationLockRegistry::new());

    // Acquire lock for app-1
    let guard1 = locks.acquire_app("app-1").await;

    // Concurrently acquiring lock for app-2 succeeds immediately
    let guard2 = locks.acquire_app("app-2").await;

    drop(guard1);
    drop(guard2);
}
