//! `apps:*` protocol dispatch (ADR-0025 D47).
//!
//! Registry queries, verified package transfer, activation, recovery and cleanup.

use crate::app_store::AppStore;
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
                "appsProtocolVersion": 4,
                "platform": if cfg!(target_os = "macos") { "darwin" } else { std::env::consts::OS },
                "arch": match std::env::consts::ARCH { "aarch64" => "arm64", "x86_64" => "x64", other => other },
                "version": env!("CARGO_PKG_VERSION")
            }))
        }
        // Single-product route (ADR-0027/0029, 2026-09-12 convergence): the
        // module-distribution chain is retired. Every one of these methods is
        // rejected BEFORE any storage, network or registry change; the
        // store-level transaction primitives remain available only to the
        // product install/update layer (plan §3.4). Old clients are told to
        // update Natives / reload the extension, never silently re-routed.
        "apps:suite_prepare"
        | "apps:install_begin"
        | "apps:install_chunk"
        | "apps:install_finish"
        | "apps:install_commit"
        | "apps:install_abort"
        | "apps:uninstall"
        | "apps:rollback" => Err(format!(
            "APP_RETIRED_METHOD: {} is retired; built-in modules ship inside the complete Natives product — update Natives and reload the extension",
            request.method
        )),
        "apps:list" => {
            let apps = store.apps().map_err(|error| error.to_string())?;
            let modules = store.module_projections().map_err(|error| error.to_string())?;
            let pending_resets = store.pending_data_resets().map_err(|error| error.to_string())?;
            let product = store.product_status().map_err(|error| error.to_string())?;
            let revision = store.global_revision().map_err(|error| error.to_string())?;
            let retained = store.retained_data().map_err(|error| error.to_string())?;
            Ok(serde_json::json!({
                "apps": apps,
                "modules": modules,
                "product": {
                    "version": product.version,
                    "generation": product.generation,
                    "configured": product.configured,
                    "sourcePresent": product.source_present,
                    "sourceVersion": product.source_version,
                },
                "pendingDataResets": pending_resets,
                "revision": revision,
                "retainedData": retained,
            }))
        }
        "apps:product_status" => {
            let status = store.product_status().map_err(|error| error.to_string())?;
            serde_json::to_value(status).map_err(|error| error.to_string())
        }
        "apps:product_configure" => {
            // Plan §3.3: the "Finish Natives setup" action runs only on the
            // foreground connection whose origin Chrome itself supplied and
            // the page already verified via apps:handshake.
            let origin = caller_origin.ok_or(
                "APP_ORIGIN_MISMATCH: product configuration requires a Chrome-verified foreground connection",
            )?;
            if store.caller_origin().as_deref() != Some(origin) {
                return Err(
                    "APP_ORIGIN_MISMATCH: product configuration requires a verified handshake on this connection"
                        .into(),
                );
            }
            let status = store
                .product_configure(origin)
                .map_err(|error| format!("{}: {error}", error.code()))?;
            serde_json::to_value(status).map_err(|error| error.to_string())
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
        "apps:clear_data" => {
            // Plan §4.3: data reset is fully separate from uninstall — a
            // restricted, receipt-journaled scope deletion that preserves
            // code, registration, activation and user preferences.
            let app_id = params
                .get("appId")
                .and_then(Value::as_str)
                .ok_or("appId is required")?;
            let request_id = params
                .get("requestId")
                .and_then(Value::as_str)
                .ok_or("requestId is required")?;
            if params.get("confirmPurge").and_then(Value::as_bool) != Some(true) {
                return Err(
                    "APP_CONFIRMATION_REQUIRED: data deletion requires confirmation".into(),
                );
            }
            let scope = crate::app_store::ClearDataScope {
                imports: params.get("deleteImports").and_then(Value::as_bool).unwrap_or(false),
                cache: params.get("deleteCache").and_then(Value::as_bool).unwrap_or(false),
                logs: params.get("deleteLogs").and_then(Value::as_bool).unwrap_or(false),
                credentials: params
                    .get("deleteCredentials")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            let receipt: crate::app_store::ClearDataReceipt = store
                .clear_module_data(app_id, request_id, scope)
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
    use crate::app_store::types::AppError;

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

    fn temp_store(name: &str) -> (std::path::PathBuf, AppStore) {
        let root = std::env::temp_dir().join(format!(
            "natives-app-dispatch-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = AppStore::open_at(&root.join("test.db"), root.join("apps")).unwrap();
        (root, store)
    }

    fn dispatch_json(store: &AppStore, value: serde_json::Value) -> Result<Value, String> {
        let request: Request = serde_json::from_value(value).unwrap();
        app_dispatch(store, &request, store.caller_origin().as_deref())
    }

    #[test]
    fn retired_distribution_methods_reject_before_any_write() {
        let (root, store) = temp_store("retired");
        for method in [
            "apps:suite_prepare",
            "apps:install_begin",
            "apps:install_chunk",
            "apps:install_finish",
            "apps:install_commit",
            "apps:install_abort",
            "apps:uninstall",
            "apps:rollback",
        ] {
            // Full-looking parameters must not matter: rejection happens
            // before any parse, storage, network or registry change.
            let error = dispatch_json(
                &store,
                serde_json::json!({
                    "id": "retired", "method": method,
                    "params": { "appId": "fund", "catalogBase64": "x", "signature": "y", "purgeData": true }
                }),
            )
            .unwrap_err();
            assert!(error.starts_with("APP_RETIRED_METHOD"), "{method}: {error}");
        }
        assert!(store.apps().unwrap().is_empty(), "no app record written");
        assert_eq!(store.global_revision().unwrap(), 0, "revision untouched");
        assert!(store.pending_data_resets().unwrap().is_empty());

        // The list still projects the fixed built-in modules from the
        // product manifest: an empty install table must show fund without
        // any install semantics.
        let response = dispatch_json(
            &store,
            serde_json::json!({ "id": "l", "method": "apps:list", "params": {} }),
        )
        .unwrap();
        assert_eq!(response["modules"][0]["appId"], "fund");
        assert_eq!(response["modules"][0]["present"], false);
        assert_eq!(response["modules"][0]["configured"], false);
        assert_eq!(response["modules"][0]["enabled"], true);
        assert_eq!(response["product"]["configured"], false);
        assert_eq!(response["product"]["sourcePresent"], false);
        assert_eq!(response["product"]["version"], "");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_data_reset_preserves_code_and_replays_completed_receipt() {
        let (root, store) = temp_store("clear-data");
        let apps = root.join("apps");
        let base = apps.join("fund");
        for dir in ["data", "imports", "cache", "runtime/1.0.0", "packages"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        std::fs::write(base.join("data/ledger.json"), b"{}").unwrap();
        std::fs::write(base.join("imports/x.csv"), b"a,b\n").unwrap();
        std::fs::write(base.join("cache/t.json"), b"{}").unwrap();
        std::fs::write(base.join("runtime/1.0.0/app"), b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(base.join("packages/fund.nap"), b"payload").unwrap();
        std::fs::write(base.join("activation.json"), b"{}").unwrap();
        store
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO apps (app_id, kind, name, version, enabled, show_in_sidebar, sidebar_order, runtime_spec_json, surface_json, manifest_json, installed_at, updated_at)
                     VALUES ('fund', 'managed_local', '基金', '1.0.0', 1, 1, 0, '{}', '{}', '{}', 0, 0)",
                    [],
                )
                .map_err(AppError::Sql)
            })
            .unwrap();

        let response = dispatch_json(
            &store,
            serde_json::json!({
                "id": "c1", "method": "apps:clear_data",
                "params": { "appId": "fund", "requestId": "reset-1", "confirmPurge": true, "deleteImports": true }
            }),
        )
        .unwrap();
        assert_eq!(response["receipt"]["state"], "completed");
        let cleared: Vec<String> = response["receipt"]["cleared"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(cleared.contains(&"data".into()));
        assert!(cleared.contains(&"imports".into()));
        assert!(!cleared.contains(&"cache".into()));
        assert!(!base.join("data").exists(), "data scope cleared");
        assert!(!base.join("imports").exists(), "imports scope cleared");
        assert!(base.join("cache").exists(), "unselected scope preserved");
        assert!(base.join("runtime/1.0.0/app").exists(), "code preserved");
        assert!(
            base.join("packages/fund.nap").exists(),
            "receipts preserved"
        );
        assert!(
            base.join("activation.json").exists(),
            "activation preserved"
        );
        let apps = store.apps().unwrap();
        assert_eq!(apps.len(), 1, "app record preserved");
        assert!(apps[0].enabled, "preferences preserved");

        // Completed requestId replay returns the original result and must
        // not clear data created after the reset. The same confirmed scope
        // is part of the requestId identity: a different scope conflicts.
        std::fs::create_dir_all(base.join("data")).unwrap();
        std::fs::write(base.join("data/new.json"), b"{}").unwrap();
        let replay = dispatch_json(
            &store,
            serde_json::json!({
                "id": "c2", "method": "apps:clear_data",
                "params": { "appId": "fund", "requestId": "reset-1", "confirmPurge": true, "deleteImports": true }
            }),
        )
        .unwrap();
        assert_eq!(replay["receipt"]["state"], "completed");
        assert_eq!(replay["receipt"]["cleared"], response["receipt"]["cleared"]);
        assert!(base.join("data/new.json").exists(), "replay keeps new data");

        // A different scope under the same requestId must conflict instead
        // of silently returning a result the caller did not ask for.
        let error = dispatch_json(
            &store,
            serde_json::json!({
                "id": "c2b", "method": "apps:clear_data",
                "params": { "appId": "fund", "requestId": "reset-1", "confirmPurge": true }
            }),
        )
        .unwrap_err();
        assert!(error.starts_with("APP_CONFLICT"), "{error}");

        // Confirmation is mandatory; uninstall is retired, not an alias.
        let error = dispatch_json(
            &store,
            serde_json::json!({
                "id": "c3", "method": "apps:clear_data",
                "params": { "appId": "fund", "requestId": "reset-2" }
            }),
        )
        .unwrap_err();
        assert!(error.starts_with("APP_CONFIRMATION_REQUIRED"), "{error}");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_data_reset_surfaces_in_list() {
        let (root, store) = temp_store("pending-reset");
        store
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO app_data_reset_receipts (request_id, app_id, scope_json, state, cleared_json, created_at)
                     VALUES ('reset-p', 'fund', '{}', 'pending', '[]', 0)",
                    [],
                )
                .map_err(AppError::Sql)
            })
            .unwrap();
        let response = dispatch_json(
            &store,
            serde_json::json!({ "id": "l", "method": "apps:list", "params": {} }),
        )
        .unwrap();
        assert_eq!(response["pendingDataResets"][0], "fund");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
