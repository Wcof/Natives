//! `apps:*` protocol dispatch (ADR-0025 D47).
//!
//! Domain-namespace methods live here, not in the legacy flat command
//! table. Phase A2 covers the metadata-only cycle: list/get/health plus
//! install_begin/install_commit/install_abort and the registry mutations.
//! Artifact transfer, host registration and repair land in Phase A5/A6.

use crate::app_store::{AppStore, UninstallReceipt};
use crate::protocol::Request;
use serde_json::Value;

/// Maximum base64 size of a metadata install request payload (well under
/// the 1 MiB incoming frame limit; package bytes never travel here).
const MAX_INSTALL_REQUEST_BASE64: usize = 256 * 1024;

/// `caller_origin`: the real extension origin captured by the
/// `apps:handshake` of THIS port (ADR-0025 D16). `None` = no handshake on
/// this connection, in which case host registration is explicitly skipped
/// — the host never fabricates an origin.
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
                Err(error) => Err(error.to_string()),
            }
        };
    }
    match request.method.as_str() {
        "apps:handshake" => {
            // Record the caller origin for host-manifest registration.
            // Validation: exactly chrome-extension://<32 hex> (the real
            // origin Chrome hands the page). Anything else is rejected —
            // an invalid handshake means the connection has NO origin.
            let origin = params
                .get("origin")
                .and_then(Value::as_str)
                .filter(|origin| crate::app_host_manifest::is_chrome_extension_origin(origin))
                .map(str::to_string)
                .ok_or_else(|| "origin must be a chrome-extension:// origin".to_string())?;
            store.set_caller_origin(Some(&origin));
            Ok(serde_json::json!({ "ok": true, "origin": origin }))
        }
        "apps:list" => {
            let apps = store.apps().map_err(|error| error.to_string())?;
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "apps": apps, "revision": revision }))
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
        "apps:health" => {
            // Phase A2: registry health only. Artifact/host health probes
            // arrive with Phase A5 package handling and Phase A6 surfaces.
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({ "status": "ok", "revision": revision }))
        }
        "apps:install_begin" => {
            let request_payload = params
                .get("request")
                .and_then(Value::as_str)
                .ok_or("request (base64 JSON) is required")?;
            if request_payload.len() > MAX_INSTALL_REQUEST_BASE64 {
                return Err("install request is too large".into());
            }
            let bytes = base64_decode(request_payload)?;
            let install_request: crate::app_store::types::InstallRequest =
                serde_json::from_slice(&bytes)
                    .map_err(|error| format!("invalid install request: {error}"))?;
            handle_result!(store.install_begin(&install_request))
        }
        "apps:install_package" => {
            let install_id = params
                .get("installId")
                .and_then(Value::as_str)
                .ok_or("installId is required")?;
            let package_id = params
                .get("packageId")
                .and_then(Value::as_str)
                .ok_or("packageId is required")?;
            let data = params
                .get("data")
                .and_then(Value::as_str)
                .ok_or("data (base64 payload) is required")?;
            handle_result!(store.install_package(install_id, package_id, data))
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
        "apps:uninstall" => {
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let receipt: UninstallReceipt =
                store.uninstall(app_id).map_err(|error| error.to_string())?;
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

fn base64_decode(encoded: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid base64 install request: {error}"))
}
