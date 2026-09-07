//! Phase A2 Gate tests: the demo app install metadata → query → enable →
//! uninstall cycle against an in-memory temp DB (no real `~/.natives`
//! side effects), plus the Phase A5 commit + runtime registration gates
//! (ADR-0025 D8/D11/D15/D16/D18).

use super::mutation::AppStore;
use super::types::{install_state, AppError, InstallRequest};
use std::path::Path;

struct StoreEnv {
    store: AppStore,
    base: std::path::PathBuf,
}

impl StoreEnv {
    fn db(&self) -> std::path::PathBuf {
        self.base.join("natives.db")
    }

    fn app_root(&self) -> &Path {
        self.store.app_root()
    }

    fn drop(self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Isolated temp store + temp app root (A2/A5 tests never touch the real
/// `~/.natives` or the browser manifest dir).
fn temp_store(tag: &str) -> StoreEnv {
    let base =
        std::env::temp_dir().join(format!("natives-app-test-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("apps")).expect("create temp store base");
    let store = AppStore::open_at(&base.join("natives.db"), base.join("apps")).expect("open store");
    StoreEnv { store, base }
}

fn err_code<T>(result: Result<T, AppError>) -> &'static str {
    match result {
        Ok(_) => panic!("expected an AppError"),
        Err(error) => error.code(),
    }
}

fn demo_request() -> InstallRequest {
    InstallRequest {
        app: super::types::AppMeta {
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
    let env = temp_store("migrate");
    // A second open on the same file re-runs every migration.
    AppStore::open_at(&env.db(), env.app_root().join("re-migrate"))
        .expect("second open re-migrates");
    env.drop();
}

#[test]
fn demo_install_query_enable_sidebar_uninstall_cycle() {
    let env = temp_store("cycle");
    let store = &env.store;

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
    // metadata-only install: no host registration, explicitly false
    assert!(!app.host_registered);

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
    let reopened = AppStore::open_at(&env.db(), env.app_root()).expect("reopen after restart");
    assert!(reopened.apps().expect("list after restart").is_empty());
    let history = reopened
        .transaction(&tx.install_id)
        .expect("history after restart");
    assert_eq!(history.state, install_state::INSTALLED);
    assert!(reopened.global_revision().expect("revision after restart") >= revision);
    env.drop();
}

#[test]
fn begin_rejects_unknown_kind_and_bad_app_id() {
    let env = temp_store("validate");
    let store = &env.store;

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
    env.drop();
}

#[test]
fn double_install_is_a_conflict() {
    let env = temp_store("conflict");
    let store = &env.store;
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
    env.drop();
}

#[test]
fn commit_and_abort_require_a_known_transaction() {
    let env = temp_store("notfound");
    let store = &env.store;
    assert_eq!(err_code(store.install_commit("missing")), "APP_NOT_FOUND");
    assert_eq!(
        err_code(store.install_abort("missing", "APP_IO", "no such install")),
        "APP_NOT_FOUND"
    );
    assert_eq!(err_code(store.uninstall("missing")), "APP_NOT_FOUND");
    env.drop();
}

#[test]
fn abort_records_explicit_error_state() {
    let env = temp_store("abort");
    let store = &env.store;
    let tx = store.install_begin(&demo_request()).expect("begin");
    let failed = store
        .install_abort(&tx.install_id, "HASH_MISMATCH", "artifact changed")
        .expect("abort");
    assert_eq!(failed.state, install_state::FAILED);
    assert_eq!(failed.error_code.as_deref(), Some("HASH_MISMATCH"));
    assert!(failed.completed_at.is_some());
    // a failed install never creates an app row
    assert!(store.apps().expect("list").is_empty());
    env.drop();
}

// ── Phase A5: commit state machine + runtime registration ─────────────
// Manifest-dir isolation: the store redirects host manifests to a
// per-test temp dir (AppStore::set_manifest_dir) — unit tests never
// touch the real browser directory.

fn unique_tag(base: &str) -> String {
    format!(
        "{base}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    )
}

/// A request whose single runtime package will be staged as a fake
/// executable. `payload_sha256` must match the staged bytes (the host
/// verifies it at `install_package` time).
fn runtime_request(
    app_id: &str,
    host: &str,
    version: &str,
    payload_size: u64,
    payload_sha256: &str,
) -> InstallRequest {
    InstallRequest {
        app: super::types::AppMeta {
            app_id: app_id.to_string(),
            kind: "extension_app".to_string(),
            name: "A5 Test".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            show_in_sidebar: false,
            sidebar_order: 0,
            runtime_spec: serde_json::json!({ "host": host }),
            surface: serde_json::json!({ "route": format!("app.html?app={app_id}") }),
            manifest: serde_json::json!({ "schemaVersion": 1 }),
        },
        packages: vec![super::types::PackageMeta {
            package_id: "host".to_string(),
            kind: "runtime".to_string(),
            version: version.to_string(),
            platform: "darwin".to_string(),
            arch: "arm64".to_string(),
            wire_size: payload_size as i64,
            payload_size: payload_size as i64,
            artifact_sha256: "ab".repeat(32),
            payload_sha256: payload_sha256.to_string(),
        }],
        permissions: vec![],
    }
}

const HEALTH_OK_STUB: &str = "#!/bin/sh\nprintf '{\"status\":\"ok\"}'\n";

/// Isolated store with its manifest dir redirected into `base/nm-hosts`.
fn a5_env(tag: &str) -> StoreEnv {
    let env = temp_store(&unique_tag(tag));
    let manifest_dir = env.base.join("nm-hosts");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    env.store.set_manifest_dir(manifest_dir);
    env
}

/// Stage the fake runtime payload through the real `install_package`
/// gate (base64 + size + payload hash against the signed snapshot).
fn stage_fake_binary(env: &StoreEnv, install_id: &str, content: &[u8]) {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(content);
    let staged = env
        .store
        .install_package(install_id, "host", &data)
        .expect("install_package stages the payload");
    assert_eq!(staged.state, "staging");
    assert_eq!(staged.payload_size as usize, content.len());
    assert_eq!(
        staged.payload_sha256,
        crate::app_install::hex_sha256(content)
    );
}

#[test]
fn a5_commit_registers_runtime_with_real_origin() {
    let env = a5_env("a5-reg");
    let manifest_dir = env.base.join("nm-hosts");
    let app_id = "com.natives.app.a5";
    let host = "com.natives.app.a5.host";
    let origin = "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";

    let stub = HEALTH_OK_STUB.as_bytes();
    let tx = env
        .store
        .install_begin(&runtime_request(
            app_id,
            host,
            "1.0.0",
            stub.len() as u64,
            &crate::app_install::hex_sha256(stub),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, stub);

    // handshake → commit with the real caller origin
    env.store.set_caller_origin(Some(origin));
    let app = env
        .store
        .install_commit_with_origin(&tx.install_id, Some(origin))
        .expect("commit with origin");

    assert!(
        app.host_registered,
        "manifest must be registered with a real origin"
    );
    // D15 layout: apps/<app_id>/runtime/1.0.0/host + current pointer
    let installed = env
        .app_root()
        .join(app_id)
        .join("runtime")
        .join("1.0.0")
        .join("host");
    assert!(
        installed.is_file(),
        "installed runtime missing: {installed:?}"
    );
    let current = env.app_root().join(app_id).join("runtime").join("current");
    assert_eq!(
        std::fs::read_to_string(&current)
            .expect("current pointer")
            .trim(),
        "1.0.0"
    );
    // manifest: real origin only, nothing else (D16/D18)
    let manifest = manifest_dir.join(format!("{host}.json"));
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).expect("manifest written"))
            .expect("manifest json");
    assert_eq!(parsed["name"], host);
    assert_eq!(parsed["type"], "stdio");
    assert_eq!(parsed["allowed_origins"][0], origin);
    assert_eq!(parsed["path"], installed.to_string_lossy().into_owned());
    for key in ["token", "secret", "api_key", "account"] {
        assert!(parsed.get(key).is_none(), "manifest must not carry {key}");
    }
    // package receipt carries the Core-decided path
    let detail = env.store.app_detail(app_id).expect("detail");
    assert_eq!(detail.packages.len(), 1);
    assert_eq!(
        detail.packages[0].installed_path,
        installed.to_string_lossy()
    );
    // staging cleared after commit (D16)
    let staging = crate::app_install::staging_dir(env.app_root(), app_id, &tx.install_id)
        .expect("staging path");
    assert!(!staging.exists(), "staging must be cleared after commit");
    env.drop();
}

#[test]
fn a5_commit_without_origin_skips_registration_explicitly() {
    let env = a5_env("a5-skip");
    let manifest_dir = env.base.join("nm-hosts");
    let app_id = "com.natives.app.a5skip";
    let host = "com.natives.app.a5skip.host";

    let stub = HEALTH_OK_STUB.as_bytes();
    let tx = env
        .store
        .install_begin(&runtime_request(
            app_id,
            host,
            "1.0.0",
            stub.len() as u64,
            &crate::app_install::hex_sha256(stub),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, stub);

    // No handshake on this connection → registration explicitly skipped.
    let app = env
        .store
        .install_commit_with_origin(&tx.install_id, None)
        .expect("commit without origin");
    assert!(
        !app.host_registered,
        "no origin → explicit host_registered = 0"
    );
    // The runtime binary IS installed (the app works in-process); only the
    // manifest is missing — an explicit state, never fabricated.
    let installed = env
        .app_root()
        .join(app_id)
        .join("runtime")
        .join("1.0.0")
        .join("host");
    assert!(installed.is_file());
    assert!(
        !manifest_dir.join(format!("{host}.json")).exists(),
        "manifest must NOT be fabricated"
    );
    env.drop();
}

#[test]
fn a5_bad_health_rolls_back_to_failed_without_app_row() {
    let env = a5_env("a5-health");
    let manifest_dir = env.base.join("nm-hosts");
    let app_id = "com.natives.app.a5bad";
    let host = "com.natives.app.a5bad.host";
    let origin = "chrome-extension://b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";

    // A binary that FAILS --health (exit 1): the whole commit must roll
    // back to `failed` — no app row, no runtime dir, no manifest.
    let stub = b"#!/bin/sh\nexit 1\n";
    let tx = env
        .store
        .install_begin(&runtime_request(
            app_id,
            host,
            "1.0.0",
            stub.len() as u64,
            &crate::app_install::hex_sha256(stub),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, stub);

    let result = env
        .store
        .install_commit_with_origin(&tx.install_id, Some(origin));
    assert!(result.is_err(), "health probe failure must fail the commit");

    // transaction: failed with an explicit error (D14 state machine)
    let failed = env
        .store
        .transaction(&tx.install_id)
        .expect("failed record");
    assert_eq!(failed.state, install_state::FAILED);
    assert!(
        failed.error_code.is_some(),
        "failed install records an explicit code"
    );
    // no app row (D14: registry row only on successful commit)
    assert!(env.store.apps().expect("list").is_empty());
    // file side rolled back: no runtime dir, no manifest
    assert!(!env.app_root().join(app_id).join("runtime").exists());
    assert!(!manifest_dir.join(format!("{host}.json")).exists());
    // staging cleared
    let staging = crate::app_install::staging_dir(env.app_root(), app_id, &tx.install_id)
        .expect("staging path");
    assert!(!staging.exists());
    env.drop();
}

#[test]
fn a5_uninstall_removes_manifest_and_runtime_keeps_data() {
    let env = a5_env("a5-uninstall");
    let manifest_dir = env.base.join("nm-hosts");
    let app_id = "com.natives.app.a5un";
    let host = "com.natives.app.a5un.host";
    let origin = "chrome-extension://c3d4e5f6a7b8c9d0e1f2a3b4c5d6ef00";

    let stub = HEALTH_OK_STUB.as_bytes();
    let tx = env
        .store
        .install_begin(&runtime_request(
            app_id,
            host,
            "1.0.0",
            stub.len() as u64,
            &crate::app_install::hex_sha256(stub),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, stub);
    env.store
        .install_commit_with_origin(&tx.install_id, Some(origin))
        .expect("commit");

    // personal data created AFTER install must survive uninstall (D32)
    let data_dir = env.app_root().join(app_id).join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::write(data_dir.join("notes.json"), b"{}").expect("data file");

    let receipt = env.store.uninstall(app_id).expect("uninstall");
    assert!(receipt.data_preserved);

    // manifest gone, runtime gone, data preserved
    assert!(!manifest_dir.join(format!("{host}.json")).exists());
    assert!(!env.app_root().join(app_id).join("runtime").exists());
    assert!(
        data_dir.join("notes.json").exists(),
        "personal data must be preserved"
    );
    assert!(env.store.apps().expect("list").is_empty());
    env.drop();
}
