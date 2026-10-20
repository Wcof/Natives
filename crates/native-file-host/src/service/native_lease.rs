//! Authenticated Native Relay ownership; a quiet Port is still an active owner.
use super::server::ServiceState;
use crate::protocol::Request;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub(crate) fn handle(state: &ServiceState, request: &Request) -> Option<Result<Value, String>> {
    if request.method == "native:bootstrap" {
        return Some(
            state
                .auth
                .lock()
                .map_err(|_| "ERR_SESSION_UNAVAILABLE".to_string())
                .and_then(|mut auth| {
                    auth.generate_bootstrap_token()
                        .map(|token| json!({"bootstrapToken": token}))
                        .map_err(String::from)
                }),
        );
    }
    if !matches!(request.method.as_str(), "native:lease" | "native:release") {
        return None;
    }
    let id = request
        .params
        .get("surfaceId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !id.starts_with("native_") || id.len() < 29 || id.len() > 80 {
        return Some(Err("ERR_NATIVE_SURFACE_INVALID".into()));
    }
    let mut leases = match state.native_surfaces.lock() {
        Ok(leases) => leases,
        Err(_) => return Some(Err("ERR_NATIVE_SURFACE_UNAVAILABLE".into())),
    };
    if request.method == "native:release" {
        let removed = leases.remove(id).is_some();
        drop(leases);
        if removed {
            state.runtime_manager.release_surface(id);
            state.model_manager.release_surface(id);
            state.files.release(id);
        }
    } else {
        if leases.len() >= 128 && !leases.contains_key(id) {
            return Some(Err("ERR_NATIVE_SURFACE_LIMIT".into()));
        }
        leases.insert(id.into(), Instant::now());
        state.files.register(id);
    }
    Some(Ok(json!({"ok": true})))
}

pub(crate) fn reap(state: &ServiceState) {
    let expired = if let Ok(mut leases) = state.native_surfaces.lock() {
        let expired = leases
            .iter()
            .filter(|(_, renewed)| renewed.elapsed() > Duration::from_secs(75))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &expired {
            leases.remove(id);
        }
        expired
    } else {
        Vec::new()
    };
    for id in expired {
        state.runtime_manager.release_surface(&id);
        state.model_manager.release_surface(&id);
        state.files.release(&id);
    }
}
