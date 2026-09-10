//! `apps:*` protocol dispatch (ADR-0025 D47).
//!
//! Registry queries, verified package transfer, activation, recovery and cleanup.

use crate::app_store::{AppStore, UninstallReceipt};
use crate::protocol::Request;
use serde_json::Value;

/// `caller_origin` comes from Chrome's launch argument. Handshakes only
/// verify it; installation rejects a missing origin.
pub(crate) fn app_dispatch(
    store: &AppStore,
    request: &Request,
    caller_origin: Option<&str>,
) -> Result<Value, String> {
    let params = &request.params;
    macro_rules! handle_result {
        ($result:expr) => {
            match $result {
                Ok(value) => serde_json::to_value(value).map_err(|error| error.to_string()),
                Err(error) => Err(format!("{}: {error}", error.code())),
            }
        };
    }
    match request.method.as_str() {
        "apps:handshake" => {
            let origin = params
                .get("origin")
                .and_then(Value::as_str)
                .and_then(crate::app_host_manifest::normalize_chrome_extension_origin)
                .ok_or_else(|| "origin must be a chrome-extension:// origin".to_string())?;
            let trusted =
                caller_origin.and_then(crate::app_host_manifest::normalize_chrome_extension_origin);
            if trusted.as_deref() != Some(origin.as_str()) {
                return Err(
                    "APP_ORIGIN_MISMATCH: caller origin does not match Chrome launch origin".into(),
                );
            }
            Ok(serde_json::json!({
                "ok": true, "origin": origin,
                "appsProtocolVersion": 3,
                "platform": if cfg!(target_os = "macos") { "darwin" } else { std::env::consts::OS },
                "arch": match std::env::consts::ARCH { "aarch64" => "arm64", "x86_64" => "x64", other => other },
                "version": env!("CARGO_PKG_VERSION")
            }))
        }
        "apps:list" => {
            let apps = store.apps().map_err(|error| error.to_string())?;
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            let retained = store.retained_data().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "apps": apps, "revision": revision, "retainedData": retained }))
        }
        "apps:get" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let detail = store
                .app_detail(app_id)
                .map_err(|error| error.to_string())?;
            Ok(serde_json::to_value(detail).map_err(|error| error.to_string())?)
        }
        "apps:recover" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            store
                .recover_install(app_id)
                .map_err(|error| format!("{}: {error}", error.code()))?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "apps:health" => {
            // Registry health only; install probes and App health are separate.
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "status": "ok", "revision": revision }))
        }
        "apps:read_resource" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let package_id = params
                .get("packageId")
                .and_then(Value::as_str)
                .ok_or("packageId is required")?;
            let offset = optional_u64(params, "offset")?;
            let length = optional_u64(params, "length")?;
            handle_result!(store.read_resource(app_id, package_id, offset, length))
        }
        "apps:install_begin" => {
            // Core Apps protocol v4 (contract §4.0): the page uploads the SIGNED
            // catalog plus its Ed25519 signature; Core verifies and selects the
            // package itself. The unsigned whole-request path is removed.
            let catalog = params
                .get("catalogBase64")
                .and_then(Value::as_str)
                .ok_or("catalogBase64 (signed catalog) is required")?;
            let signature = params
                .get("signature")
                .and_then(Value::as_str)
                .ok_or("signature (base64 Ed25519) is required")?;
            handle_result!(store.install_begin_catalog(catalog, signature))
        }
        "apps:install_chunk" => {
            let install_id = params
                .get("installId")
                .and_then(Value::as_str)
                .ok_or("installId is required")?;
            let package_id = params
                .get("packageId")
                .and_then(Value::as_str)
                .ok_or("packageId is required")?;
            let offset = required_u64(params, "offset")?;
            let data = params
                .get("dataBase64")
                .and_then(Value::as_str)
                .ok_or("dataBase64 (chunk payload) is required")?;
            let chunk_sha256 = params
                .get("chunkSha256")
                .and_then(Value::as_str)
                .ok_or("chunkSha256 is required")?;
            handle_result!(store.install_chunk(install_id, package_id, offset, data, chunk_sha256))
        }
        "apps:install_finish" => {
            let install_id = params
                .get("installId")
                .and_then(Value::as_str)
                .ok_or("installId is required")?;
            let package_id = params
                .get("packageId")
                .and_then(Value::as_str)
                .ok_or("packageId is required")?;
            // Contract §4.0: artifactBytes must equal the signed wire size.
            let artifact_bytes = required_u64(params, "artifactBytes")?;
            handle_result!(store.install_finish(install_id, package_id, artifact_bytes))
        }
        "apps:install_commit" => {
            let install_id = params
                .get("installId")
                .and_then(Value::as_str)
                .ok_or("installId is required")?;
            handle_result!(store.install_commit_with_origin(install_id, caller_origin))
        }
        "apps:install_abort" => {
            let install_id = params
                .get("installId")
                .and_then(Value::as_str)
                .ok_or("installId is required")?;
            let error_code = params
                .get("errorCode")
                .and_then(Value::as_str)
                .unwrap_or("APP_INTERNAL");
            let error_message = params
                .get("errorMessage")
                .and_then(Value::as_str)
                .unwrap_or("");
            handle_result!(store.install_abort(install_id, error_code, error_message))
        }
        "apps:uninstall" | "apps:clear_data" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let purge = request.method == "apps:clear_data"
                || params
                    .get("purgeData")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            if purge && params.get("confirmPurge").and_then(Value::as_bool) != Some(true) {
                return Err(
                    "APP_CONFIRMATION_REQUIRED: data deletion requires confirmation".into(),
                );
            }
            let receipt: UninstallReceipt = store
                .uninstall_with_data(app_id, purge)
                .map_err(|error| format!("{}: {error}", error.code()))?;
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "receipt": receipt, "revision": revision }))
        }
        "apps:set_enabled" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let enabled = params
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or("enabled is required")?;
            let app = store
                .set_enabled(app_id, enabled)
                .map_err(|error| error.to_string())?;
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "app": app, "revision": revision }))
        }
        "apps:set_sidebar" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let show = params
                .get("show")
                .and_then(Value::as_bool)
                .ok_or("show is required")?;
            let order = params.get("order").and_then(Value::as_i64);
            let app = store
                .set_sidebar(app_id, show, order)
                .map_err(|error| error.to_string())?;
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "app": app, "revision": revision }))
        }
        other => Err(format!("unsupported apps method: {other}")),
    }
}

fn optional_u64(params: &Value, key: &str) -> Result<Option<u64>, String> {
    match params.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a non-negative integer")),
    }
}

fn required_u64(params: &Value, key: &str) -> Result<u64, String> {
    optional_u64(params, key)?.ok_or_else(|| format!("{key} is required"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_ranges_reject_negative_fractional_and_string_values() {
        for value in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("1"),
        ] {
            assert!(optional_u64(&serde_json::json!({ "offset": value }), "offset").is_err());
        }
        assert_eq!(
            optional_u64(&serde_json::json!({ "offset": 0 }), "offset").unwrap(),
            Some(0)
        );
    }

    #[test]
    fn handshake_cannot_grant_an_origin_not_supplied_by_chrome() {
        let root = std::env::temp_dir().join(format!(
            "natives-app-origin-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = AppStore::open_at(&root.join("test.db"), root.join("apps")).unwrap();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let request: Request = serde_json::from_value(serde_json::json!({
            "id": "origin-check", "method": "apps:handshake", "params": { "origin": origin }
        }))
        .unwrap();
        assert!(app_dispatch(&store, &request, None).is_err());
        assert!(store.caller_origin().is_none());
        store.set_caller_origin(Some(origin));
        let response = app_dispatch(&store, &request, store.caller_origin().as_deref()).unwrap();
        assert_eq!(response["origin"], origin);
        assert!(response["platform"].is_string());
        assert!(response["arch"].is_string());
        let spoofed: Request = serde_json::from_value(serde_json::json!({
            "id": "spoof", "method": "apps:handshake",
            "params": { "origin": "chrome-extension://pppppppppppppppppppppppppppppppp/" }
        }))
        .unwrap();
        assert!(app_dispatch(&store, &spoofed, store.caller_origin().as_deref()).is_err());
        assert_eq!(store.caller_origin().as_deref(), Some(origin));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
