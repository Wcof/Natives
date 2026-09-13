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

static TEST_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Isolated temp store + temp app root (A2/A5 tests never touch the real
/// `~/.natives` or the browser manifest dir).
fn temp_store(tag: &str) -> StoreEnv {
    let seq = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "natives-app-test-{}-{}-{:?}-{}",
        tag,
        std::process::id(),
        std::thread::current().id(),
        seq
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
            // ADR-0027: managed_local is the only accepted app kind; data
            // packages remain valid payloads inside a managed_local app.
            kind: super::types::KIND_MANAGED_LOCAL.to_string(),
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
    assert_eq!(app.kind, super::types::KIND_MANAGED_LOCAL);
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
            kind: super::types::KIND_MANAGED_LOCAL.to_string(),
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
    let tx = store_install_version_begin(env, version);
    env.store.install_commit(&tx.install_id).unwrap();
}

fn store_install_version_begin(env: &StoreEnv, version: &str) -> super::types::InstallTransaction {
    let (request, bytes) = version_request(version);
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(env, &tx.install_id, &bytes);
    let _ = version;
    tx
}

// ── Core Apps protocol v4 (ADR-0027): signed catalog → chunks → finish ──

fn openssl_sign(catalog: &[u8]) -> Option<String> {
    const DEV_PRIVATE_PEM: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/apps/keys/catalog-trust-dev.private.pem"
    );
    if !std::path::Path::new(DEV_PRIVATE_PEM).exists() {
        return None;
    }
    let input = std::env::temp_dir().join(format!(
        "v4-sign-{}-{}",
        std::process::id(),
        crate::workspace_store::schema::uuid_v4()
    ));
    std::fs::write(&input, catalog).ok()?;
    let output = std::process::Command::new("openssl")
        .args(["pkeyutl", "-sign", "-inkey"])
        .arg(DEV_PRIVATE_PEM)
        .args(["-rawin", "-in"])
        .arg(&input)
        .output()
        .ok()?;
    let _ = std::fs::remove_file(&input);
    if !output.status.success() {
        return None;
    }
    use base64::Engine as _;
    Some(base64::engine::general_purpose::STANDARD.encode(&output.stdout))
}

/// A managed_local catalog whose executable payload is the gzip of `payload`.
fn managed_catalog(
    app_id: &str,
    version: &str,
    payload: &[u8],
) -> (Vec<u8>, Vec<u8>, InstallRequest) {
    use std::io::Write as _;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload).unwrap();
    let artifact = encoder.finish().unwrap();
    let request = InstallRequest {
        app: super::types::AppMeta {
            app_id: app_id.to_string(),
            kind: super::types::KIND_MANAGED_LOCAL.to_string(),
            name: "Managed Demo".to_string(),
            version: version.to_string(),
            enabled: true,
            show_in_sidebar: true,
            sidebar_order: 0,
            runtime_spec: serde_json::json!({}),
            surface: serde_json::json!({ "route": format!("app.html?app={app_id}") }),
            manifest: serde_json::json!({ "schemaVersion": 3 }),
        },
        packages: vec![super::types::PackageMeta {
            package_id: "app-exec".to_string(),
            kind: super::types::KIND_MANAGED_LOCAL.to_string(),
            version: version.to_string(),
            platform: super::types::platform().to_string(),
            arch: super::types::architecture().to_string(),
            wire_size: artifact.len() as i64,
            payload_size: payload.len() as i64,
            artifact_sha256: crate::app_install::hex_sha256(&artifact),
            payload_sha256: crate::app_install::hex_sha256(payload),
            required: true,
        }],
        permissions: vec!["app.lifecycle".to_string()],
        min_host_version: None,
    };
    (artifact, payload.to_vec(), request)
}

/// Production-shaped Catalog v3 wrapper for the Apps v4 entrypoint.  The
/// older `InstallRequest` fixtures above exercise only the compatibility
/// parser; A3's security checks must also cover Core's actual appId selection.
fn managed_catalog_v3(app_id: &str, version: &str, payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (artifact, _, request) = managed_catalog(app_id, version, payload);
    let app = request.app;
    let package = request.packages.into_iter().next().unwrap();
    let catalog = serde_json::json!({
        "catalogVersion": 3,
        "publishedAt": "2026-09-10T00:00:00Z",
        "apps": [{
            "app_id": app.app_id,
            "kind": app.kind,
            "name": app.name,
            "version": app.version,
            "appProtocolVersion": 1,
            "minHostVersion": "0.1.0",
            "permissions": request.permissions,
            "runtime_spec": app.runtime_spec,
            "surface": app.surface,
            "manifest": { "schemaVersion": 1, "fixture": true },
            "packages": [{
                "package_id": package.package_id,
                "kind": package.kind,
                "version": package.version,
                "platform": package.platform,
                "arch": package.arch,
                "wire_size": package.wire_size,
                "payload_size": package.payload_size,
                "artifact_sha256": package.artifact_sha256,
                "payload_sha256": package.payload_sha256,
                "required": package.required,
                "url": "https://example.invalid/managed-demo.nap"
            }],
            "published": true
        }]
    });
    (serde_json::to_vec(&catalog).unwrap(), artifact)
}

fn begin_catalog_v3(
    store: &AppStore,
    catalog: &[u8],
    app_id: &str,
) -> super::types::InstallBeginResult {
    use base64::Engine as _;
    let signature = openssl_sign(catalog).expect("development catalog signing key is available");
    store
        .install_begin_catalog_for(
            &base64::engine::general_purpose::STANDARD.encode(catalog),
            &signature,
            app_id,
        )
        .expect("signed Catalog v3 begins the selected app")
}

fn chunk<T: AsRef<[u8]>>(data: T) -> (String, String) {
    use base64::Engine as _;
    (
        base64::engine::general_purpose::STANDARD.encode(data.as_ref()),
        crate::app_install::hex_sha256(data.as_ref()),
    )
}

#[test]
fn v4_rejects_platform_mismatch_and_payload_hash_mismatch() {
    let env = temp_store("v4-rejects");
    let store = &env.store;
    let payload = b"#!/bin/sh\necho x\n";
    let (_artifact, _payload, mut request) =
        managed_catalog("com.natives.app.demo", "1.0.0", payload);
    // Platform mismatch: the signed catalog targets another OS.
    request.packages[0].platform = "windows".to_string();
    let catalog_json = serde_json::to_vec(&request).unwrap();
    let Some(signature) = openssl_sign(&catalog_json) else {
        env.drop();
        return;
    };
    use base64::Engine as _;
    let catalog_b64 = base64::engine::general_purpose::STANDARD.encode(&catalog_json);
    assert_eq!(
        err_code(store.install_begin_catalog(&catalog_b64, &signature)),
        "APP_INVALID_STATE"
    );

    // Payload hash mismatch: signature is valid but the decompressed bytes
    // do not match the declared payload_sha256 — finish must fail closed.
    let (artifact2, _p, request2) = managed_catalog("com.natives.app.demo", "1.0.0", payload);
    let mut request2 = request2;
    request2.packages[0].payload_sha256 = crate::app_install::hex_sha256(b"tampered");
    let catalog2 = serde_json::to_vec(&request2).unwrap();
    let Some(signature2) = openssl_sign(&catalog2) else {
        env.drop();
        return;
    };
    let catalog2_b64 = base64::engine::general_purpose::STANDARD.encode(&catalog2);
    let begun = store
        .install_begin_catalog(&catalog2_b64, &signature2)
        .expect("valid signature begins");
    let (first, first_hash) = chunk(&artifact2);
    store
        .install_chunk(&begun.install_id, &begun.package_id, 0, &first, &first_hash)
        .expect("chunk");
    let error = store
        .install_finish(&begun.install_id, &begun.package_id, artifact2.len() as u64)
        .expect_err("payload hash mismatch must reject");
    assert_eq!(error.code(), "APP_INVALID_STATE");
    assert!(error.to_string().contains("APP_PAYLOAD_HASH_MISMATCH"));
    env.drop();
}

#[test]
fn v4_signed_catalog_chunked_install_reaches_staged() {
    let env = temp_store("v4-chunked");
    let store = &env.store;
    let payload = b"#!/bin/sh\necho managed demo\n";
    let (artifact, _payload, request) = managed_catalog("com.natives.app.demo", "1.0.0", payload);
    let catalog_json = serde_json::to_vec(&request).unwrap();
    let Some(signature) = openssl_sign(&catalog_json) else {
        env.drop();
        return; // openssl/key unavailable in this environment
    };
    use base64::Engine as _;
    let catalog_b64 = base64::engine::general_purpose::STANDARD.encode(&catalog_json);

    // Tampered catalog is rejected before any state is created.
    let mut tampered = catalog_json.clone();
    tampered[0] ^= 0x01;
    let tampered_b64 = base64::engine::general_purpose::STANDARD.encode(&tampered);
    assert_eq!(
        err_code(store.install_begin_catalog(&tampered_b64, &signature)),
        "APP_INVALID_STATE"
    );

    let begun = store
        .install_begin_catalog(&catalog_b64, &signature)
        .expect("signed catalog begins an install");
    assert_eq!(begun.chunk_size, super::types::INSTALL_CHUNK_SIZE);
    assert_eq!(begun.next_offset, 0);

    // Sequential chunks in two halves.
    let mid = artifact.len() / 2;
    let (first, first_hash) = chunk(&artifact[..mid]);
    let (second, second_hash) = chunk(&artifact[mid..]);
    let r1 = store
        .install_chunk(&begun.install_id, &begun.package_id, 0, &first, &first_hash)
        .expect("first chunk");
    assert_eq!(r1.next_offset, mid as u64);
    // Out-of-order (gap) is rejected.
    assert_eq!(
        err_code(store.install_chunk(
            &begun.install_id,
            &begun.package_id,
            mid as u64 + 1,
            &second,
            &second_hash
        )),
        "APP_PACKAGE_INVALID"
    );
    // Idempotent resend of the confirmed first chunk returns the same offset.
    let resend = store
        .install_chunk(&begun.install_id, &begun.package_id, 0, &first, &first_hash)
        .expect("idempotent resend");
    assert_eq!(resend.next_offset, mid as u64);
    let r2 = store
        .install_chunk(
            &begun.install_id,
            &begun.package_id,
            mid as u64,
            &second,
            &second_hash,
        )
        .expect("second chunk");
    assert_eq!(r2.next_offset, artifact.len() as u64);

    // finish: length must equal the signed wire size; then streams gunzip.
    let wrong = store.install_finish(
        &begun.install_id,
        &begun.package_id,
        artifact.len() as u64 + 1,
    );
    assert_eq!(err_code(wrong), "APP_PACKAGE_INVALID");
    let finished = store
        .install_finish(&begun.install_id, &begun.package_id, artifact.len() as u64)
        .expect("finish stages the payload");
    assert!(finished.ready);
    assert_eq!(
        finished.payload_sha256,
        crate::app_install::hex_sha256(payload)
    );
    env.drop();
}

#[test]
fn v4_catalog_v3_rejects_bad_signature_platform_and_oversize() {
    let env = temp_store("v4-v3-catalog-rejects");
    let app_id = "com.natives.app.catalog";
    let (catalog, _) = managed_catalog_v3(app_id, "1.0.0", b"managed payload");
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&catalog);

    assert_eq!(
        err_code(
            env.store
                .install_begin_catalog_for(&encoded, "AA==", app_id)
        ),
        "APP_INVALID_STATE"
    );

    let mut wrong_platform: serde_json::Value = serde_json::from_slice(&catalog).unwrap();
    wrong_platform["apps"][0]["packages"][0]["platform"] = serde_json::json!("windows");
    let wrong_platform = serde_json::to_vec(&wrong_platform).unwrap();
    let signature = openssl_sign(&wrong_platform).expect("development signing key");
    assert_eq!(
        err_code(env.store.install_begin_catalog_for(
            &base64::engine::general_purpose::STANDARD.encode(wrong_platform),
            &signature,
            app_id,
        )),
        "APP_INVALID_STATE"
    );

    let oversize =
        base64::engine::general_purpose::STANDARD
            .encode(vec![b'x'; super::types::CATALOG_MAX_BYTES + 1]);
    assert_eq!(
        err_code(
            env.store
                .install_begin_catalog_for(&oversize, "AA==", app_id)
        ),
        "APP_INVALID_STATE"
    );
    assert!(env.store.apps().unwrap().is_empty());
    env.drop();
}

#[test]
fn v4_catalog_v3_artifact_hash_failure_aborts_without_an_app_row() {
    let env = temp_store("v4-v3-artifact-hash");
    let app_id = "com.natives.app.artifact";
    let (catalog, artifact) = managed_catalog_v3(app_id, "1.0.0", b"managed payload");
    let mut catalog: serde_json::Value = serde_json::from_slice(&catalog).unwrap();
    catalog["apps"][0]["packages"][0]["artifact_sha256"] = serde_json::json!("00".repeat(32));
    let catalog = serde_json::to_vec(&catalog).unwrap();
    let begun = begin_catalog_v3(&env.store, &catalog, app_id);
    let (data, digest) = chunk(&artifact);
    env.store
        .install_chunk(&begun.install_id, &begun.package_id, 0, &data, &digest)
        .unwrap();

    assert_eq!(
        err_code(env.store.install_finish(
            &begun.install_id,
            &begun.package_id,
            artifact.len() as u64,
        )),
        "APP_PACKAGE_INVALID"
    );
    assert_eq!(
        env.store.transaction(&begun.install_id).unwrap().state,
        install_state::FAILED
    );
    assert!(env.store.apps().unwrap().is_empty());
    assert!(
        !crate::app_install::staging_dir(env.app_root(), app_id, &begun.install_id)
            .unwrap()
            .exists()
    );
    env.drop();
}

#[test]
fn v4_catalog_v3_chunks_are_ordered_idempotent_and_abortable() {
    let env = temp_store("v4-v3-chunks");
    let app_id = "com.natives.app.chunks";
    let payload: Vec<u8> = (0..4096).map(|n| ((n * 73) % 251) as u8).collect();
    let (catalog, artifact) = managed_catalog_v3(app_id, "1.0.0", &payload);
    let begun = begin_catalog_v3(&env.store, &catalog, app_id);
    let mid = (artifact.len() / 2).max(1);
    let (first, first_hash) = chunk(&artifact[..mid]);
    let (second, second_hash) = chunk(&artifact[mid..]);

    assert_eq!(
        env.store
            .install_chunk(&begun.install_id, &begun.package_id, 0, &first, &first_hash)
            .unwrap()
            .next_offset,
        mid as u64
    );
    assert_eq!(
        err_code(env.store.install_chunk(
            &begun.install_id,
            &begun.package_id,
            mid as u64 + 1,
            &second,
            &second_hash,
        )),
        "APP_PACKAGE_INVALID"
    );
    assert_eq!(
        env.store
            .install_chunk(&begun.install_id, &begun.package_id, 0, &first, &first_hash)
            .unwrap()
            .next_offset,
        mid as u64,
        "a confirmed byte range can be resent after reconnect"
    );
    assert_eq!(
        env.store
            .install_chunk(
                &begun.install_id,
                &begun.package_id,
                mid as u64,
                &second,
                &second_hash,
            )
            .unwrap()
            .next_offset,
        artifact.len() as u64
    );

    let aborted = env
        .store
        .install_abort(&begun.install_id, "APP_CANCELLED", "user cancelled")
        .unwrap();
    assert_eq!(aborted.state, install_state::FAILED);
    assert!(env.store.apps().unwrap().is_empty());
    assert!(
        !crate::app_install::staging_dir(env.app_root(), app_id, &begun.install_id)
            .unwrap()
            .exists()
    );
    env.drop();
}

#[test]
fn v4_catalog_v3_connection_close_aborts_without_touching_user_data() {
    let env = temp_store("v4-v3-recover");
    let app_id = "com.natives.app.recover";
    let (catalog, artifact) = managed_catalog_v3(app_id, "1.0.0", b"interrupted transfer");
    let begun = begin_catalog_v3(&env.store, &catalog, app_id);
    let first_len = (artifact.len() / 2).max(1);
    let (first, digest) = chunk(&artifact[..first_len]);
    env.store
        .install_chunk(&begun.install_id, &begun.package_id, 0, &first, &digest)
        .unwrap();
    let data = env.app_root().join(app_id).join("data").join("keep");
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::write(&data, b"personal data").unwrap();

    let StoreEnv { store, base } = env;
    let db = base.join("natives.db");
    let root = store.app_root().to_owned();
    drop(store);
    let reopened = AppStore::open_at(&db, &root).unwrap();
    reopened.set_caller_origin(Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"));
    reopened.recover_install(app_id).unwrap();

    assert_eq!(
        reopened.transaction(&begun.install_id).unwrap().state,
        install_state::FAILED
    );
    assert_eq!(
        reopened
            .transaction(&begun.install_id)
            .unwrap()
            .error_code
            .as_deref(),
        Some("APP_CANCELLED")
    );
    assert!(reopened.apps().unwrap().is_empty());
    assert_eq!(std::fs::read(&data).unwrap(), b"personal data");
    assert!(
        !crate::app_install::staging_dir(&root, app_id, &begun.install_id)
            .unwrap()
            .exists()
    );
    drop(reopened);
    let _ = std::fs::remove_dir_all(base);
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

// ── Phase 2: Unified Package Source (SuiteSeed & Remote) ───────────────

fn seed_fixture(
    app_id: &str,
    version: &str,
    payload: &[u8],
) -> (std::path::PathBuf, super::types::SuiteManifestEntry) {
    let seq = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "natives-seed-test-{}-{}-{}",
        app_id,
        std::process::id(),
        seq
    ));
    let _ = std::fs::create_dir_all(&dir);
    // The seed entry's identity comes from a SIGNED Catalog v3, exactly like
    // an online install; the manifest only binds the on-disk artifact name.
    let (catalog_json, artifact) = managed_catalog_v3(app_id, version, payload);
    let signature =
        openssl_sign(&catalog_json).expect("development catalog signing key is available");
    let nap_name = format!("{app_id}-{version}.nap");
    std::fs::write(dir.join(&nap_name), &artifact).unwrap();
    use base64::Engine as _;
    let entry = super::types::SuiteManifestEntry {
        app_id: app_id.to_string(),
        catalog_base64: base64::engine::general_purpose::STANDARD.encode(&catalog_json),
        signature_base64: signature,
        artifact: nap_name,
    };
    (dir, entry)
}

fn write_suite_manifest(dir: &std::path::Path, apps: Vec<super::types::SuiteManifestEntry>) {
    let manifest = super::types::SuiteManifest {
        schema_version: 2,
        suite_id: "natives-suite".into(),
        version: "1.0.0".into(),
        apps,
    };
    std::fs::write(
        dir.join(super::reconcile::SUITE_MANIFEST_NAME),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn suite_seed_unified_install_pipeline_succeeds() {
    let env = temp_store("suite-seed-ok");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));
    let app_id = "fund";
    let (seeds_dir, entry) = seed_fixture(app_id, "1.0.0", b"#!/bin/sh\nexit 0\n");

    let installed = env
        .store
        .install_seed_entry(&seeds_dir, &entry, Some(origin))
        .expect("install seed");

    assert_eq!(installed.app_id, "fund");
    assert_eq!(installed.version, "1.0.0");
    assert!(installed.enabled);
    assert!(installed.host_registered);

    // Verify it exists in store.apps()
    let apps = env.store.apps().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].app_id, "fund");

    env.drop();
}

#[test]
fn suite_seed_corrupted_artifact_hash_fails() {
    let env = temp_store("suite-seed-corrupt-art");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));
    let app_id = "fund";
    let (seeds_dir, entry) = seed_fixture(app_id, "1.0.0", b"#!/bin/sh\nexit 0\n");
    // Tamper with the on-disk bytes AFTER signing: the signed catalog still
    // declares the original hash, so the unified pipeline must reject it.
    std::fs::write(seeds_dir.join(&entry.artifact), b"tampered payload bytes").unwrap();

    let err = env
        .store
        .install_seed_entry(&seeds_dir, &entry, Some(origin))
        .expect_err("tampered artifact must fail hash verification");

    // Truncated/tampered bytes fail the signed wire-size or hash check.
    assert_eq!(err.code(), "APP_PACKAGE_INVALID");
    assert!(env.store.apps().unwrap().is_empty());
    env.drop();
}

#[test]
fn suite_seed_tampered_signature_fails() {
    let env = temp_store("suite-seed-tampered-sig");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));
    let app_id = "fund";
    let (seeds_dir, mut entry) = seed_fixture(app_id, "1.0.0", b"#!/bin/sh\nexit 0\n");
    // A seed manifest entry whose catalog signature does not verify must be
    // rejected by the SAME Ed25519 chain as online installs (AC-03).
    entry.signature_base64 = "AAAA".to_string();

    let err = env
        .store
        .install_seed_entry(&seeds_dir, &entry, Some(origin))
        .expect_err("tampered catalog signature must fail");

    assert!(err.to_string().contains("SIGNATURE") || err.to_string().contains("signature"));
    assert!(env.store.apps().unwrap().is_empty());
    env.drop();
}

#[test]
fn suite_seed_invalid_app_id_fails() {
    let env = temp_store("suite-seed-bad-id");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));
    let (seeds_dir, mut entry) = seed_fixture("fund", "1.0.0", b"#!/bin/sh\nexit 0\n");
    entry.app_id = "../escape".into();

    let err = env
        .store
        .install_seed_entry(&seeds_dir, &entry, Some(origin))
        .expect_err("invalid seed app id must be rejected");
    assert_eq!(err.code(), "APP_INVALID_STATE");
    assert!(env.store.apps().unwrap().is_empty());
    env.drop();
}

#[test]
fn suite_seed_artifact_path_escape_fails() {
    let env = temp_store("suite-seed-path-escape");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));
    let (seeds_dir, mut entry) = seed_fixture("fund", "1.0.0", b"#!/bin/sh\nexit 0\n");
    // AC-03 boundary: the manifest cannot point the install outside the
    // seeds directory.
    entry.artifact = "../../etc/passwd".into();

    let err = env
        .store
        .install_seed_entry(&seeds_dir, &entry, Some(origin))
        .expect_err("path escape must be rejected");
    assert_eq!(err.code(), "APP_INVALID_STATE");
    assert!(env.store.apps().unwrap().is_empty());
    env.drop();
}

#[test]
fn suite_seed_reconciliation_lifecycle() {
    let env = temp_store("reconcile-lifecycle");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));
    let app_id = "fund";

    // Prepare signed suite manifest v0.2.0 in a temp seeds directory
    let seq = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let seeds_dir = std::env::temp_dir().join(format!("natives-reconcile-seeds-{}", seq));
    let _ = std::fs::create_dir_all(&seeds_dir);
    let (artifact, _, _) = managed_catalog(app_id, "0.2.0", b"#!/bin/sh\nexit 0\n");
    std::fs::write(seeds_dir.join("fund-0.2.0.nap"), &artifact).unwrap();
    let (catalog_v2, _) = managed_catalog_v3(app_id, "0.2.0", b"#!/bin/sh\nexit 0\n");
    let sig_v2 = openssl_sign(&catalog_v2).expect("development catalog signing key is available");
    use base64::Engine as _;
    let entry_v2 = super::types::SuiteManifestEntry {
        app_id: app_id.to_string(),
        catalog_base64: base64::engine::general_purpose::STANDARD.encode(&catalog_v2),
        signature_base64: sig_v2,
        artifact: "fund-0.2.0.nap".into(),
    };
    write_suite_manifest(&seeds_dir, vec![entry_v2.clone()]);

    // 1. Fresh Install (app not installed) -> installed!
    let rep1 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("fresh reconcile");
    assert!(rep1.ok);
    assert_eq!(rep1.items.len(), 1);
    assert_eq!(rep1.items[0].status, "installed");

    let apps = env.store.apps().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].version, "0.2.0");

    // 2. Second Startup (Idempotent NOOP) -> up_to_date!
    let rep2 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("second reconcile");
    assert!(rep2.ok);
    assert_eq!(rep2.items[0].status, "up_to_date");

    // 2b. Missing Runtime Binary Repair: delete binary on disk
    let runtime_path = crate::app_install::install_path_for(
        env.app_root(),
        "fund",
        super::types::KIND_MANAGED_LOCAL,
        "0.2.0",
        "app-exec",
    )
    .unwrap();
    assert!(runtime_path.is_file());
    std::fs::remove_file(&runtime_path).unwrap();
    assert!(!runtime_path.exists());

    let rep_repair = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("repair reconcile");
    assert!(rep_repair.ok);
    assert_eq!(rep_repair.items[0].status, "repaired");
    assert!(
        runtime_path.is_file(),
        "Runtime binary must be restored by seed repair"
    );

    // 3. Local Installed > Seed (P4-4): keep newer, never downgrade.
    env.store
        .with_conn(|conn| {
            conn.execute(
                "UPDATE apps SET version = '0.3.0' WHERE app_id = 'fund'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    let rep3 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("local newer reconcile");
    assert!(rep3.ok);
    assert_eq!(rep3.items[0].status, "kept_newer");
    let app = env.store.app("fund").unwrap();
    assert_eq!(app.version, "0.3.0");

    // 4. Seed newer than local (P4-5): upgrade via the signed transaction.
    let (art_v4, _, _) = managed_catalog(app_id, "0.4.0", b"#!/bin/sh\nexit 0\n");
    std::fs::write(seeds_dir.join("fund-0.4.0.nap"), &art_v4).unwrap();
    let (catalog_v4, _) = managed_catalog_v3(app_id, "0.4.0", b"#!/bin/sh\nexit 0\n");
    let sig_v4 = openssl_sign(&catalog_v4).expect("development catalog signing key is available");
    let entry_v4 = super::types::SuiteManifestEntry {
        app_id: app_id.to_string(),
        catalog_base64: base64::engine::general_purpose::STANDARD.encode(&catalog_v4),
        signature_base64: sig_v4,
        artifact: "fund-0.4.0.nap".into(),
    };
    write_suite_manifest(&seeds_dir, vec![entry_v4.clone()]);

    let rep4 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("upgrade reconcile");
    assert!(rep4.ok);
    assert_eq!(rep4.items[0].status, "upgraded");
    let app_v4 = env.store.app("fund").unwrap();
    assert_eq!(app_v4.version, "0.4.0");

    // 5. Corrupted Seed: tamper the on-disk artifact; the signed catalog
    //    still declares the original hash, so the item fails and the
    //    registry keeps 0.4.0.
    std::fs::write(seeds_dir.join(&entry_v4.artifact), b"tampered").unwrap();
    let (catalog_v5, _) = managed_catalog_v3(app_id, "0.5.0", b"#!/bin/sh\nexit 0\n");
    let sig_v5 = openssl_sign(&catalog_v5).expect("development catalog signing key is available");
    std::fs::write(seeds_dir.join("fund-0.5.0.nap"), b"tampered").unwrap();
    let entry_v5 = super::types::SuiteManifestEntry {
        app_id: app_id.to_string(),
        catalog_base64: base64::engine::general_purpose::STANDARD.encode(&catalog_v5),
        signature_base64: sig_v5,
        artifact: "fund-0.5.0.nap".into(),
    };
    write_suite_manifest(&seeds_dir, vec![entry_v5]);

    let rep5 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("corrupted seed reconcile");
    assert!(!rep5.ok);
    assert_eq!(rep5.items[0].status, "failed");
    let app_unchanged = env.store.app("fund").unwrap();
    assert_eq!(app_unchanged.version, "0.4.0");

    // 6. AC-04: user removal intent survives reconciliation. Uninstall, then
    //    reconcile with the (still-present) seed: the app must NOT come back.
    env.store.uninstall("fund").expect("uninstall");
    write_suite_manifest(&seeds_dir, vec![entry_v4.clone()]);
    let rep6 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("removed reconcile");
    assert!(rep6.ok);
    assert_eq!(rep6.items[0].status, "kept_removed");
    assert!(
        env.store.apps().unwrap().is_empty(),
        "removed app must not be reinstalled by seed reconciliation"
    );

    // 7. Only an explicit user action changes the choice back. The retained
    // data of the default uninstall stays cleanup-pending, so the seed
    // reconcile reports the explicit conflict instead of pretending success.
    env.store.set_user_intent("fund", "default").unwrap();
    let rep7 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("restore reconcile");
    assert!(!rep7.ok);
    assert_eq!(rep7.items[0].status, "failed");

    let _ = std::fs::remove_dir_all(&seeds_dir);
    env.drop();
}

#[test]
fn suite_seed_removed_intent_blocks_fresh_install() {
    let env = temp_store("reconcile-removed-fresh");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));

    let seq = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let seeds_dir = std::env::temp_dir().join(format!("natives-reconcile-removed-{}", seq));
    let _ = std::fs::create_dir_all(&seeds_dir);
    let (fixture_dir, entry) = seed_fixture("demo-app", "1.0.0", b"#!/bin/sh\nexit 0\n");
    // The fixture writes both the .nap and the manifest into one directory.
    write_suite_manifest(&fixture_dir, vec![entry]);
    let seeds_dir = fixture_dir;

    // User removed this app before it was ever installed (e.g. intent synced
    // from a previous suite): seed reconciliation must not install it.
    env.store.set_user_intent("demo-app", "removed").unwrap();
    let rep1 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("removed fresh reconcile");
    assert!(rep1.ok);
    assert_eq!(rep1.items[0].status, "kept_removed");
    assert!(env.store.apps().unwrap().is_empty());

    // Explicit user action (restore) allows the install.
    env.store.set_user_intent("demo-app", "default").unwrap();
    let rep2 = env
        .store
        .reconcile_suite_seeds(&seeds_dir)
        .expect("restored fresh reconcile");
    assert!(rep2.ok);
    assert_eq!(rep2.items[0].status, "installed");
    assert_eq!(env.store.apps().unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&seeds_dir);
    env.drop();
}

#[test]
fn suite_prepare_requires_foreground_origin() {
    // AC-07: preparation is a foreground-connection action; without a
    // verified caller origin it must refuse instead of silently doing work.
    let env = temp_store("suite-prepare-no-origin");
    env.store.set_caller_origin(None);
    let err = env
        .store
        .prepare_suite_seeds()
        .expect_err("origin required");
    assert!(err.to_string().contains("APP_ORIGIN_REQUIRED"));
    env.drop();
}

#[test]
fn suite_prepare_reports_conflict_on_same_version_different_hash() {
    // T09 (AC-06): installed 1.0.0 whose signed payload hash differs from the
    // seed's signed catalog is a CONFLICT — never auto-replaced, never
    // reported as up_to_date.
    let env = temp_store("suite-prepare-conflict");
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
    env.store.set_caller_origin(Some(origin));

    // Install v1.0.0 from its own signed catalog.
    let (seeds_dir, entry) = seed_fixture("fund", "1.0.0", b"#!/bin/sh\nexit 0\n");
    env.store
        .install_seed_entry(&seeds_dir, &entry, Some(origin))
        .expect("first install");

    // Build a DIFFERENT signed catalog for the SAME version (different payload).
    let (catalog_alt, _artifact_alt) = managed_catalog_v3("fund", "1.0.0", b"#!/bin/sh\nexit 1\n");
    let sig_alt = openssl_sign(&catalog_alt).expect("development catalog signing key is available");
    use base64::Engine as _;
    let alt_entry = super::types::SuiteManifestEntry {
        app_id: "fund".into(),
        catalog_base64: base64::engine::general_purpose::STANDARD.encode(&catalog_alt),
        signature_base64: sig_alt,
        artifact: entry.artifact.clone(),
    };
    write_suite_manifest(&seeds_dir, vec![alt_entry]);

    // Point the foreground preparation at this test's seeds directory.
    std::env::set_var("NATIVES_SEEDS_DIR", &seeds_dir);
    let report = env.store.prepare_suite_seeds().expect("prepare");
    std::env::remove_var("NATIVES_SEEDS_DIR");
    assert!(!report.ok);
    assert_eq!(report.items[0].status, "conflict");
    // Registry untouched.
    assert_eq!(env.store.app("fund").unwrap().version, "1.0.0");
    let _ = std::fs::remove_dir_all(&seeds_dir);
    env.drop();
}

// ── Per-user product configuration (plan §3.3, single-product route) ────

fn product_test_platform() -> (&'static str, &'static str) {
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    (platform, arch)
}

fn write_product_source(base: &Path, exe_bytes: &[u8]) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let source = base.join("source");
    std::fs::create_dir_all(source.join("modules/fund")).unwrap();
    std::fs::write(source.join("modules/fund/app"), exe_bytes).unwrap();
    let payload_sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(exe_bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let (platform, arch) = product_test_platform();
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "product": "natives",
        "version": "1.0.0",
        "platform": platform,
        "arch": arch,
        "modules": [{
            "appId": "fund", "version": "1.0.0", "entryRoute": "app.html?app=fund",
            "name": {"zh_CN": "基金", "en": "Fund"},
            "artifactPath": "modules/fund/app", "payloadSha256": payload_sha256,
        }],
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(
        source.join(super::product::PRODUCT_MANIFEST_NAME),
        &manifest_bytes,
    )
    .unwrap();
    let signature =
        openssl_sign(&manifest_bytes).expect("development catalog signing key is available");
    std::fs::write(
        source.join(super::product::PRODUCT_MANIFEST_SIGNATURE_NAME),
        signature,
    )
    .unwrap();
    source
}

#[test]
fn product_configure_prepares_fixed_modules_and_replays_idempotently() {
    let env = temp_store("product-ok");
    let manifest_dir = env.base.join("manifests");
    std::fs::create_dir_all(&manifest_dir).unwrap();
    env.store.set_manifest_dir(manifest_dir.clone());
    let exe_bytes = b"#!/bin/sh\nexit 0\n";
    let source = write_product_source(&env.base, exe_bytes);
    env.store.set_product_source(source.clone());
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

    let status = env
        .store
        .product_configure(origin)
        .expect("configure product");
    assert!(status.configured);
    assert_eq!(status.generation, 1);
    assert_eq!(status.version, "1.0.0");
    assert!(status.source_present);

    // Private active payload, registration and activation are in place.
    let exe = env.app_root().join("fund/runtime/1.0.0/app");
    assert!(exe.exists(), "active payload prepared");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(
            exe.metadata().unwrap().permissions().mode() & 0o111 != 0,
            "payload is executable"
        );
    }
    let host = crate::app_activation::runtime_host_name("fund");
    assert!(
        manifest_dir.join(format!("{host}.json")).exists(),
        "native messaging registered"
    );
    let projection = crate::app_activation::read_activation_projection(env.app_root(), "fund")
        .unwrap()
        .expect("activation projection");
    assert_eq!(projection["runtimeHost"], host);
    assert_eq!(projection["activationState"], "ready");

    // The module becomes a configured card with its preferences.
    let apps = env.store.apps().unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].app_id, "fund");
    assert!(apps[0].host_registered);
    assert!(apps[0].enabled);
    let modules = env.store.module_projections().unwrap();
    assert_eq!(modules[0].configured, true);
    assert_eq!(modules[0].present, true);

    // Idempotent replay of the identical manifest keeps the generation.
    let replay = env
        .store
        .product_configure(origin)
        .expect("replay configure");
    assert_eq!(replay.generation, 1, "identical manifest is a no-op");
    env.drop();
}

#[test]
fn product_configure_rejects_bad_signature_without_side_effects() {
    let env = temp_store("product-bad-sig");
    let exe_bytes = b"#!/bin/sh\nexit 0\n";
    let source = write_product_source(&env.base, exe_bytes);
    std::fs::write(
        source.join(super::product::PRODUCT_MANIFEST_SIGNATURE_NAME),
        "AAAA",
    )
    .unwrap();
    env.store.set_product_source(source);
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

    let error = env.store.product_configure(origin).unwrap_err();
    assert!(
        error.to_string().contains("APP_SIGNATURE_INVALID"),
        "{error}"
    );
    assert!(env.store.apps().unwrap().is_empty(), "nothing configured");
    assert_eq!(env.store.product_status().unwrap().generation, 0);
    env.drop();
}

#[test]
fn product_configure_rejects_payload_hash_mismatch() {
    let env = temp_store("product-hash");
    let source = write_product_source(&env.base, b"#!/bin/sh\nexit 0\n");
    // Tamper with the artifact AFTER signing: the manifest still declares
    // the original payload hash.
    std::fs::write(source.join("modules/fund/app"), b"tampered").unwrap();
    env.store.set_product_source(source);
    let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

    let error = env.store.product_configure(origin).unwrap_err();
    assert_eq!(error.code(), "APP_PACKAGE_INVALID", "{error}");
    assert!(env.store.apps().unwrap().is_empty(), "nothing configured");
    assert!(
        !env.app_root().join("fund/runtime").exists(),
        "no payload applied"
    );
    env.drop();
}
