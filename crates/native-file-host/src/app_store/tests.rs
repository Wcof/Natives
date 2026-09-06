//! Phase A2 Gate tests: the demo app install metadata → query → enable →
//! sidebar toggle → uninstall cycle, plus restart persistence of the
//! registry and install history.

use super::mutation::AppStore;
use crate::app_store::types::{install_state, AppError, AppMeta, InstallRequest};
use std::path::PathBuf;

fn temp_store(tag: &str) -> (AppStore, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "natives-app-test-{}-{}.db",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    (AppStore::open(&path).expect("open store"), path)
}

/// Assert the error code without requiring `Debug` on the Ok type
/// (registry row types are intentionally Serialize-only, ADR-0025 D13).
fn err_code<T>(result: Result<T, AppError>) -> &'static str {
    match result {
        Ok(_) => panic!("expected an AppError"),
        Err(error) => error.code(),
    }
}

fn demo_request() -> InstallRequest {
    InstallRequest {
        app: AppMeta {
            app_id: "com.natives.app.demo".to_string(),
            kind: "extension_app".to_string(),
            name: "Demo".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            show_in_sidebar: true,
            sidebar_order: 0,
            runtime_spec: serde_json::json!({ "hostId": "com.natives.app.demo" }),
            surface: serde_json::json!({ "route": "app.html?app=com.natives.app.demo" }),
            manifest: serde_json::json!({ "schemaVersion": 1 }),
        },
        packages: vec![],
        permissions: vec!["app.lifecycle".to_string()],
    }
}

#[test]
fn migrations_are_idempotent() {
    let (_store, path) = temp_store("migrate");
    // A second open on the same file re-runs every migration.
    AppStore::open(&path).expect("second open re-migrates");
    let _ = std::fs::remove_file(path);
}

#[test]
fn demo_install_query_enable_sidebar_uninstall_cycle() {
    let (store, path) = temp_store("cycle");

    // begin → catalog_resolved
    let tx = store.install_begin(&demo_request()).expect("begin");
    assert_eq!(tx.state, install_state::CATALOG_RESOLVED);
    assert!(!tx.install_id.is_empty());

    // commit → installed app row
    let app = store.install_commit(&tx.install_id).expect("commit");
    assert_eq!(app.app_id, "com.natives.app.demo");
    assert_eq!(app.kind, "extension_app");
    assert!(app.enabled);
    assert!(app.show_in_sidebar);
    assert_eq!(app.sidebar_order, 0);

    // query
    let apps = store.apps().expect("list apps");
    assert_eq!(apps.len(), 1);
    let detail = store.app_detail(&app.app_id).expect("app detail");
    assert_eq!(detail.app.name, "Demo");
    assert_eq!(detail.permissions.len(), 1);
    assert_eq!(detail.permissions[0].permission, "app.lifecycle");
    assert!(detail.packages.is_empty());

    // enable toggle
    let disabled = store.set_enabled(&app.app_id, false).expect("disable");
    assert!(!disabled.enabled);
    let enabled_again = store.set_enabled(&app.app_id, true).expect("re-enable");
    assert!(enabled_again.enabled);

    // sidebar toggle
    let hidden = store
        .set_sidebar(&app.app_id, false, Some(7))
        .expect("hide from sidebar");
    assert!(!hidden.show_in_sidebar);
    assert_eq!(hidden.sidebar_order, 7);

    // every committed mutation bumped the projection revision
    // (begin is exempt): commit + disable + enable + hide = 4
    let revision = store.global_revision().expect("revision");
    assert!(
        revision >= 4,
        "commit/disable/enable/hide must each bump the revision: {revision}"
    );

    // uninstall → registry gone, receipt promises data preservation
    let receipt = store.uninstall(&app.app_id).expect("uninstall");
    assert!(receipt.data_preserved);
    assert!(store.apps().expect("list after uninstall").is_empty());
    assert_eq!(err_code(store.app(&app.app_id)), "APP_NOT_FOUND");
    // install history survives uninstall (audit)
    let history = store
        .transaction(&tx.install_id)
        .expect("history after uninstall");
    assert_eq!(history.state, install_state::INSTALLED);

    // restart → state preserved (Gate A2: 数据库重启状态不丢)
    let reopened = AppStore::open(&path).expect("reopen after restart");
    assert!(reopened.apps().expect("list after restart").is_empty());
    let history = reopened
        .transaction(&tx.install_id)
        .expect("history after restart");
    assert_eq!(history.state, install_state::INSTALLED);
    assert!(reopened.global_revision().expect("revision after restart") >= revision);
    let _ = std::fs::remove_file(path);
}

#[test]
fn begin_rejects_unknown_kind_and_bad_app_id() {
    let (store, _path) = temp_store("validate");

    let mut bad_kind = demo_request();
    bad_kind.app.kind = "plugin".to_string();
    assert_eq!(
        err_code(store.install_begin(&bad_kind)),
        "APP_INVALID_STATE"
    );

    let mut bad_id = demo_request();
    bad_id.app.app_id = "../evil".to_string();
    assert!(matches!(
        store.install_begin(&bad_id),
        Err(AppError::InvalidState(_))
    ));
}

#[test]
fn double_install_is_a_conflict() {
    let (store, _path) = temp_store("conflict");
    let tx = store.install_begin(&demo_request()).expect("begin");
    store.install_commit(&tx.install_id).expect("commit");
    assert_eq!(
        err_code(store.install_begin(&demo_request())),
        "APP_CONFLICT"
    );

    // committing twice is also a conflict
    assert_eq!(
        err_code(store.install_commit(&tx.install_id)),
        "APP_CONFLICT"
    );
}

#[test]
fn commit_and_abort_require_a_known_transaction() {
    let (store, _path) = temp_store("notfound");
    assert_eq!(err_code(store.install_commit("missing")), "APP_NOT_FOUND");
    assert_eq!(
        err_code(store.install_abort("missing", "APP_IO", "no such install")),
        "APP_NOT_FOUND"
    );
    assert_eq!(err_code(store.uninstall("missing")), "APP_NOT_FOUND");
}

#[test]
fn abort_records_explicit_error_state() {
    let (store, _path) = temp_store("abort");
    let tx = store.install_begin(&demo_request()).expect("begin");
    let failed = store
        .install_abort(&tx.install_id, "HASH_MISMATCH", "artifact changed")
        .expect("abort");
    assert_eq!(failed.state, install_state::FAILED);
    assert_eq!(failed.error_code.as_deref(), Some("HASH_MISMATCH"));
    assert!(failed.completed_at.is_some());
    // a failed install never creates an app row
    assert!(store.apps().expect("list").is_empty());
}
