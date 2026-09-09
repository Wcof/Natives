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
        drop(self.store);
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Isolated temp store + temp app root (A2/A5 tests never touch the real
/// `~/.natives` or the browser manifest dir).
fn temp_store(tag: &str) -> StoreEnv {
    let base = std::env::temp_dir().join(format!(
        "natives-app-test-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("apps")).expect("create temp store base");
    let store = AppStore::open_at(&base.join("natives.db"), base.join("apps")).expect("open store");
    store.set_caller_origin(Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"));
    store.set_manifest_dir(base.join("nm-hosts"));
    StoreEnv { store, base }
}

fn err_code<T>(result: Result<T, AppError>) -> &'static str {
    match result {
        Ok(_) => panic!("expected an AppError"),
        Err(error) => error.code(),
    }
}

fn demo_request() -> InstallRequest {
    let json_bytes = b"{\"message\":\"hello from demo data\"}";
    data_request(
        "com.natives.app.demo",
        "1.0.0",
        json_bytes.len() as u64,
        &crate::app_install::hex_sha256(json_bytes),
    )
}

/// Helper for v2 pure data/resource requests
fn data_request(
    app_id: &str,
    version: &str,
    payload_size: u64,
    payload_sha256: &str,
) -> InstallRequest {
    let name = if app_id.contains("demo") {
        "Demo"
    } else {
        "Resource Test"
    };
    InstallRequest {
        app: super::types::AppMeta {
            app_id: app_id.to_string(),
            kind: "extension_app".to_string(),
            name: name.to_string(),
            version: version.to_string(),
            enabled: true,
            show_in_sidebar: true,
            sidebar_order: 0,
            runtime_spec: serde_json::json!({}),
            surface: serde_json::json!({ "route": format!("app.html?app={app_id}") }),
            manifest: serde_json::json!({ "schemaVersion": 2 }),
        },
        packages: vec![super::types::PackageMeta {
            package_id: "demo-data".to_string(),
            kind: "data".to_string(),
            version: version.to_string(),
            platform: "any".to_string(),
            arch: "any".to_string(),
            wire_size: payload_size as i64,
            payload_size: payload_size as i64,
            artifact_sha256: "ab".repeat(32),
            payload_sha256: payload_sha256.to_string(),
            required: true,
        }],
        permissions: vec!["app.lifecycle".to_string()],
        min_host_version: None,
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
    let req = demo_request();
    let json_bytes = b"{\"message\":\"hello from demo data\"}";
    let tx = store.install_begin(&req).expect("begin");
    assert_eq!(tx.state, install_state::CATALOG_RESOLVED);
    assert!(!tx.install_id.is_empty());

    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(json_bytes);
    store
        .install_package(&tx.install_id, "demo-data", &data)
        .expect("stage data");
    // commit → installed app row
    let app = store.install_commit(&tx.install_id).expect("commit");
    assert_eq!(app.app_id, "com.natives.app.demo");
    assert_eq!(app.kind, "extension_app");
    assert!(app.enabled);
    assert!(app.show_in_sidebar);
    assert_eq!(app.sidebar_order, 0);
    assert!(
        !app.host_registered,
        "v2 resource installs must not claim a child Host registration"
    );

    // resource reading (ADR-0026)
    let res = store
        .read_resource(&app.app_id, "demo-data", None, None)
        .expect("read resource");
    assert!(res.ok);
    assert_eq!(res.format, "json");
    assert_eq!(res.total_size, json_bytes.len() as u64);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&res.data)
        .unwrap();
    assert_eq!(decoded, json_bytes);

    // query
    let apps = store.apps().expect("list apps");
    assert_eq!(apps.len(), 1);
    let detail = store.app_detail(&app.app_id).expect("app detail");
    assert_eq!(detail.app.name, "Demo");
    assert_eq!(detail.permissions.len(), 1);
    assert_eq!(detail.permissions[0].permission, "app.lifecycle");
    assert_eq!(detail.packages.len(), 1);

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
    let req = demo_request();
    let json_bytes = b"{\"message\":\"hello from demo data\"}";
    let tx = store.install_begin(&req).expect("begin");
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(json_bytes);
    store
        .install_package(&tx.install_id, "demo-data", &data)
        .expect("stage data");
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
            version: version.to_string(),
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
            platform: super::types::platform().to_string(),
            arch: super::types::architecture().to_string(),
            wire_size: payload_size as i64,
            payload_size: payload_size as i64,
            artifact_sha256: "ab".repeat(32),
            payload_sha256: payload_sha256.to_string(),
            required: true,
        }],
        permissions: vec![],
        min_host_version: None,
    }
}

const HEALTH_OK_STUB: &str = "#!/bin/sh\nprintf '{\"status\":\"ok\",\"version\":\"1.0.0\"}'\n";

#[test]
fn install_cannot_commit_missing_packages_or_an_aborted_transaction() {
    let env = a5_env("incomplete");
    let (request, _bytes) = version_request("1.0.0");
    let tx = env.store.install_begin(&request).unwrap();
    assert!(env.store.install_commit(&tx.install_id).is_err());
    env.store
        .install_abort(&tx.install_id, "APP_CANCELLED", "cancelled")
        .unwrap();
    assert!(env.store.install_commit(&tx.install_id).is_err());
    assert!(env.store.apps().unwrap().is_empty());
    env.drop();
}

#[test]
fn update_can_begin_without_removing_the_installed_app() {
    let env = a5_env("update-begin");
    let (mut request, bytes) = version_request("1.0.0");
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(&env, &tx.install_id, &bytes);
    env.store.install_commit(&tx.install_id).unwrap();
    request.app.version = "1.1.0".into();
    request.packages[0].version = "1.1.0".into();
    let update = env
        .store
        .install_begin(&request)
        .expect("an update preserves the old app");
    assert_eq!(update.from_version, "1.0.0");
    assert_eq!(env.store.app("demo").unwrap().version, "1.0.0");
    env.store
        .install_abort(&update.install_id, "APP_CANCELLED", "cancelled")
        .unwrap();
    assert_eq!(env.store.app("demo").unwrap().version, "1.0.0");
    env.drop();
}

fn version_request(version: &str) -> (InstallRequest, Vec<u8>) {
    let json_bytes = format!("{{\"message\":\"version {}\"}}", version).into_bytes();
    (
        data_request(
            "demo",
            version,
            json_bytes.len() as u64,
            &crate::app_install::hex_sha256(&json_bytes),
        ),
        json_bytes,
    )
}

fn stage_fake_binary(env: &StoreEnv, install_id: &str, content: &[u8]) {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(content);
    let staged = env
        .store
        .install_package(install_id, "demo-data", &data)
        .expect("install_package stages the payload");
    assert_eq!(staged.state, "staging");
    assert_eq!(staged.payload_size as usize, content.len());
    assert_eq!(
        staged.payload_sha256,
        crate::app_install::hex_sha256(content)
    );
}

fn install_version(env: &StoreEnv, version: &str) {
    let (request, bytes) = version_request(version);
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(env, &tx.install_id, &bytes);
    env.store.install_commit(&tx.install_id).unwrap();
}

mod updates;
mod v2;

#[test]
fn install_owner_blocks_other_connections_and_abort_removes_staging() {
    let env = a5_env("install-owner");
    let (request, bytes) = version_request("1.0.0");
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(&env, &tx.install_id, &bytes);
    let other = AppStore::open_at(&env.db(), env.app_root()).unwrap();
    other.set_caller_origin(env.store.caller_origin().as_deref());
    other.set_manifest_dir(env.base.join("nm-hosts"));
    assert_eq!(other.transaction(&tx.install_id).unwrap().state, "staging");
    assert!(other.install_begin(&request).is_err());
    assert!(other.install_commit(&tx.install_id).is_err());
    assert!(other
        .install_abort(&tx.install_id, "cancel", "other port")
        .is_err());
    env.store
        .install_abort(&tx.install_id, "APP_CANCELLED", "cancelled")
        .unwrap();
    assert!(
        !crate::app_install::staging_dir(env.app_root(), "demo", &tx.install_id)
            .unwrap()
            .exists()
    );
    assert!(other.install_begin(&request).is_ok());
    drop(other);
    env.drop();
}

#[test]
fn uninstall_preserves_data_by_default_and_later_purge_wipes_only_owned_data() {
    let env = a5_env("purge-data");
    install_version(&env, "1.0.0");
    for sub in ["data", "cache", "imports"] {
        let dir = env.app_root().join("demo").join(sub);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("personal"), b"personal").unwrap();
    }
    let logs = env.base.join("logs/apps/demo");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("runtime.log"), b"log").unwrap();
    std::fs::create_dir_all(env.app_root().join("other/data")).unwrap();
    std::fs::write(env.app_root().join("other/data/keep"), b"other").unwrap();
    env.store.uninstall("demo").unwrap();
    assert!(env.app_root().join("demo/data/personal").exists());
    assert!(logs.exists());
    assert_eq!(env.store.retained_data().unwrap().len(), 1);
    let receipt = env.store.uninstall_with_data("demo", true).unwrap();
    assert!(!receipt.data_preserved);
    assert!(!env.app_root().join("demo").exists());
    assert!(!logs.exists());
    assert!(env.store.retained_data().unwrap().is_empty());
    assert_eq!(
        std::fs::read(env.app_root().join("other/data/keep")).unwrap(),
        b"other"
    );
    env.drop();
}

#[cfg(unix)]
#[test]
fn failed_cleanup_is_visible_and_retryable_without_following_symlinks() {
    let env = a5_env("cleanup-retry");
    install_version(&env, "1.0.0");
    let logs = env.base.join("logs/apps");
    std::fs::create_dir_all(&logs).unwrap();
    let outside = env.base.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep"), b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, logs.join("demo")).unwrap();
    assert!(env.store.uninstall_with_data("demo", true).is_err());
    assert!(env.store.retained_data().unwrap()[0].cleanup_pending);
    assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"outside");
    std::fs::remove_file(logs.join("demo")).unwrap();
    env.store.uninstall_with_data("demo", true).unwrap();
    assert!(env.store.retained_data().unwrap().is_empty());
    env.drop();
}

#[test]
fn metadata_only_and_incompatible_package_sets_are_rejected() {
    let env = a5_env("package-contract");
    let (mut request, _) = version_request("1.0.0");
    request.packages[0].arch = "unsupported".into();
    assert!(env.store.install_begin(&request).is_err());
    let mut runtime_pkg = request.clone();
    runtime_pkg.packages[0].kind = "runtime".into();
    assert!(
        env.store.install_begin(&runtime_pkg).is_err(),
        "runtime packages are forbidden"
    );
    env.drop();
}

/// Isolated store with its manifest dir redirected into `base/nm-hosts`.
fn a5_env(tag: &str) -> StoreEnv {
    let env = temp_store(&unique_tag(tag));
    let manifest_dir = env.base.join("nm-hosts");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    env.store.set_manifest_dir(manifest_dir);
    env
}

#[test]
fn a5_commit_registers_resource_package_and_allows_read() {
    let env = a5_env("a5-reg");
    let app_id = "com.natives.app.a5";
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

    let json_bytes = b"{\"name\":\"hello\",\"version\":\"1.0.0\"}";
    let tx = env
        .store
        .install_begin(&data_request(
            app_id,
            "1.0.0",
            json_bytes.len() as u64,
            &crate::app_install::hex_sha256(json_bytes),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, json_bytes);

    // handshake → commit with the real caller origin
    env.store.set_caller_origin(Some(origin));
    let app = env
        .store
        .install_commit_with_origin(&tx.install_id, Some(origin))
        .expect("commit with origin");

    assert_eq!(app.app_id, app_id);
    let installed = env
        .app_root()
        .join(app_id)
        .join("packages")
        .join("1.0.0")
        .join("demo-data");
    assert!(installed.is_file(), "installed data missing: {installed:?}");

    // read_resource works
    let res = env
        .store
        .read_resource(app_id, "demo-data", None, None)
        .expect("read");
    assert_eq!(res.format, "json");
    assert_eq!(res.total_size, json_bytes.len() as u64);

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
fn a5_commit_without_origin_refuses_registration() {
    let env = a5_env("a5-skip");
    let app_id = "com.natives.app.a5skip";

    let json_bytes = b"{\"hello\":true}";
    let tx = env
        .store
        .install_begin(&data_request(
            app_id,
            "1.0.0",
            json_bytes.len() as u64,
            &crate::app_install::hex_sha256(json_bytes),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, json_bytes);

    // No handshake on this connection → registration explicitly skipped.
    let result = env.store.install_commit_with_origin(&tx.install_id, None);
    assert!(
        result.is_err(),
        "missing origin must not produce an installed app"
    );
    let installed = env
        .app_root()
        .join(app_id)
        .join("packages")
        .join("1.0.0")
        .join("demo-data");
    assert!(!installed.exists());
    env.drop();
}

#[test]
fn a5_bad_payload_rolls_back_to_failed_without_app_row() {
    let env = a5_env("a5-bad");
    let app_id = "com.natives.app.a5bad";
    let origin = "chrome-extension://bcdefghijklmnopabcdefghijklmnopa/";
    env.store.set_caller_origin(Some(origin));

    // Executable ELF/Mach-O or script is rejected in resource validation
    let bad_bytes = b"\x7fELFfakeexecutable";
    let tx = env
        .store
        .install_begin(&data_request(
            app_id,
            "1.0.0",
            bad_bytes.len() as u64,
            &crate::app_install::hex_sha256(bad_bytes),
        ))
        .expect("begin");

    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bad_bytes);
    let stage_result = env
        .store
        .install_package(&tx.install_id, "demo-data", &encoded);
    assert!(stage_result.is_err(), "executable payload must be rejected");

    let failed = env
        .store
        .transaction(&tx.install_id)
        .expect("failed record");
    assert_eq!(failed.state, install_state::FAILED);
    assert!(env.store.apps().expect("list").is_empty());
    env.drop();
}

#[test]
fn a5_uninstall_removes_packages_keeps_data() {
    let env = a5_env("a5-uninstall");
    let app_id = "com.natives.app.a5un";
    let origin = "chrome-extension://cdefghijklmnopabcdefghijklmnopab/";
    env.store.set_caller_origin(Some(origin));

    let json_bytes = b"{\"test\":1}";
    let tx = env
        .store
        .install_begin(&data_request(
            app_id,
            "1.0.0",
            json_bytes.len() as u64,
            &crate::app_install::hex_sha256(json_bytes),
        ))
        .expect("begin");
    stage_fake_binary(&env, &tx.install_id, json_bytes);
    env.store
        .install_commit_with_origin(&tx.install_id, Some(origin))
        .expect("commit");

    // personal data created AFTER install must survive uninstall (D32)
    let data_dir = env.app_root().join(app_id).join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::write(data_dir.join("notes.json"), b"{}").expect("data file");

    let receipt = env.store.uninstall(app_id).expect("uninstall");
    assert!(receipt.data_preserved);

    assert!(!env.app_root().join(app_id).join("packages").exists());
    assert!(
        data_dir.join("notes.json").exists(),
        "personal data must be preserved"
    );
    assert!(env.store.apps().expect("list").is_empty());
    env.drop();
}
