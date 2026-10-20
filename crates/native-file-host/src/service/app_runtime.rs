//! App Runtime 子进程调度与请求中继（ADR-0042 / F04 / F08）。
//! 负责在独立 Web 模式下按 appId/instanceId 拉起并复用 natives-app-runtime 进程。

use super::runtime_worker::AppRuntimeWorker;
use crate::protocol::{safe_error, Request, Response};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn resolve_runtime_binary() -> io::Result<PathBuf> {
    if let Ok(bin) = std::env::var("NATIVES_APP_RUNTIME_BIN") {
        let p = PathBuf::from(bin);
        if p.is_file() {
            return Ok(p);
        }
    }

    let product_binary =
        crate::app_product::default_system_source().join("hosts/natives-app-runtime");
    if product_binary.is_file() {
        return Ok(product_binary);
    }

    if !cfg!(debug_assertions) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "natives-app-runtime binary not found in product source",
        ));
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(manifest_dir);

    let candidates = [
        repo_root.join("target/debug/natives-app-runtime"),
        repo_root.join("target/release/natives-app-runtime"),
    ];

    for c in candidates {
        if c.is_file() {
            return Ok(c);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "natives-app-runtime binary not found in product source or local target directories",
    ))
}

pub struct AppRuntimeManager {
    web_port: u16,
    origin: Option<String>,
    gates: HashMap<&'static str, Mutex<()>>,
    workers: Mutex<HashMap<String, Arc<AppRuntimeWorker>>>,
    instance_to_app: Mutex<HashMap<String, String>>, // instanceId -> app_id
    surface_to_app: Mutex<HashMap<String, String>>,  // one owner per page/Native Port
    idle_since: Mutex<HashMap<String, Instant>>,
}

impl Default for AppRuntimeManager {
    fn default() -> Self {
        Self {
            web_port: 0,
            origin: None,
            gates: HashMap::from([("fund", Mutex::new(())), ("tokenusage", Mutex::new(()))]),
            workers: Mutex::new(HashMap::new()),
            instance_to_app: Mutex::new(HashMap::new()),
            surface_to_app: Mutex::new(HashMap::new()),
            idle_since: Mutex::new(HashMap::new()),
        }
    }
}

impl AppRuntimeManager {
    pub(crate) fn begin_maintenance(
        &self,
        app_id: &str,
        close_surfaces: bool,
    ) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        let gate = self
            .gates
            .get(app_id)
            .ok_or("APP_UNKNOWN")?
            .lock()
            .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?;
        if !close_surfaces
            && self
                .surface_to_app
                .lock()
                .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?
                .values()
                .any(|app| app == app_id)
        {
            return Err("APP_BUSY".into());
        }
        let worker = self
            .workers
            .lock()
            .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?
            .get(app_id)
            .cloned();
        if let Some(worker) = worker {
            if worker.is_alive() {
                let status = worker.forward_frame(&Request {
                    id: "maintenance-status".into(),
                    method: "app:status".into(),
                    params: serde_json::json!({}),
                })?;
                if status["activeRequests"].as_u64().unwrap_or(1) > 0
                    || status["activeTasks"].as_u64().unwrap_or(1) > 0
                {
                    return Err("APP_BUSY".into());
                }
                worker.forward_frame(&Request {
                    id: "maintenance-stop".into(),
                    method: "app:stop".into(),
                    params: serde_json::json!({}),
                })?;
            }
            self.workers
                .lock()
                .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?
                .remove(app_id);
            drop(worker);
        }
        self.surface_to_app
            .lock()
            .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?
            .retain(|_, app| app != app_id);
        self.instance_to_app
            .lock()
            .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?
            .retain(|_, app| app != app_id);
        self.idle_since
            .lock()
            .map_err(|_| "APP_RUNTIME_UNAVAILABLE")?
            .remove(app_id);
        Ok(gate)
    }

    pub(crate) fn for_service(web_port: u16, origin: Option<String>) -> Self {
        Self {
            web_port,
            origin,
            ..Self::default()
        }
    }

    pub(crate) fn dispatch_request<'a>(&self, request: &'a Request) -> Response<'a> {
        let surface_id = match validated_surface(request) {
            Ok(id) => id,
            Err(error) => return self.error(request, error),
        };
        if request.method == "app:release" || request.method == "app:stop" {
            self.release_surface(surface_id);
            return Response {
                id: &request.id,
                ok: true,
                result: Some(serde_json::json!({"stopped": true})),
                error: None,
            };
        }
        if !matches!(
            request.method.as_str(),
            "app:handshake"
                | "app:start"
                | "app:status"
                | "app:data_status"
                | "app:card_snapshot"
                | "app:session"
                | "app:secret"
                | "app:invalidate"
        ) {
            return self.error(request, "APP_METHOD_NOT_ALLOWED");
        }
        let app_id_from_params = request
            .params
            .get("appId")
            .and_then(Value::as_str)
            .map(String::from);

        let instance_id_from_params = request.params.get("instanceId").and_then(Value::as_str);

        let mapped_app = instance_id_from_params
            .and_then(|inst| self.instance_to_app.lock().ok()?.get(inst).cloned());
        if instance_id_from_params.is_some() && mapped_app.is_none() {
            return self.error(request, "APP_SESSION_INVALID");
        }
        if let (Some(explicit), Some(mapped)) = (&app_id_from_params, &mapped_app) {
            if explicit != mapped {
                return Response {
                    id: &request.id,
                    ok: false,
                    result: None,
                    error: Some("instanceId does not belong to appId".into()),
                };
            }
        }
        let Some(app_id) = app_id_from_params.or(mapped_app) else {
            return Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some("appId or known instanceId is required".into()),
            };
        };
        if !matches!(app_id.as_str(), "fund" | "tokenusage") {
            return Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some("unknown appId".into()),
            };
        }
        let Ok(_gate) = self.gates[app_id.as_str()].lock() else {
            return self.error(request, "APP_RUNTIME_UNAVAILABLE");
        };

        let owner_app = self
            .surface_to_app
            .lock()
            .ok()
            .and_then(|owners| owners.get(surface_id).cloned());
        if request.method != "app:handshake" && owner_app.as_deref() != Some(&app_id) {
            return self.error(request, "APP_SURFACE_NOT_BOUND");
        }
        if owner_app.as_ref().is_some_and(|bound| bound != &app_id) {
            return self.error(request, "APP_SURFACE_WRONG_APP");
        }

        if request.method == "app:invalidate" {
            let origin = request
                .params
                .get("originId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if origin.is_empty()
                || origin.len() > 64
                || !origin
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            {
                return self.error(request, "APP_FRAME_INVALID");
            }
            return Response {
                id: &request.id,
                ok: true,
                result: Some(serde_json::json!({"appId": app_id, "originId": origin})),
                error: None,
            };
        }

        let mut worker = self
            .workers
            .lock()
            .ok()
            .and_then(|workers| workers.get(&app_id).cloned());
        if worker.as_ref().is_some_and(|worker| !worker.is_alive()) {
            self.workers.lock().unwrap().remove(&app_id);
            self.surface_to_app
                .lock()
                .unwrap()
                .retain(|_, value| value != &app_id);
            self.instance_to_app
                .lock()
                .unwrap()
                .retain(|_, value| value != &app_id);
            worker = None;
        }

        if worker.is_none() {
            if request.method != "app:handshake" {
                return self.error(request, "APP_RUNTIME_NOT_STARTED");
            }
            let Some(origin) = self.origin.as_deref() else {
                return self.error(request, "APP_PRODUCT_ORIGIN_UNAVAILABLE");
            };
            match AppRuntimeWorker::spawn(&app_id, self.web_port, origin) {
                Ok(w) => {
                    let w = Arc::new(w);
                    self.workers
                        .lock()
                        .unwrap()
                        .insert(app_id.clone(), w.clone());
                    worker = Some(w);
                }
                Err(err) => {
                    return Response {
                        id: &request.id,
                        ok: false,
                        result: None,
                        error: Some(format!("failed to spawn runtime for {app_id}: {err}")),
                    }
                }
            }
        }

        let worker = worker.unwrap();
        match worker.forward_frame(request) {
            Ok(result) => {
                if request.method == "app:handshake" {
                    self.surface_to_app
                        .lock()
                        .unwrap()
                        .insert(surface_id.into(), app_id.clone());
                    self.idle_since.lock().unwrap().remove(&app_id);
                }
                if let Some(inst) = result.get("instanceId").and_then(Value::as_str) {
                    if let Ok(mut map) = self.instance_to_app.lock() {
                        map.insert(inst.to_string(), app_id);
                    }
                }
                Response {
                    id: &request.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                }
            }
            Err(err) => Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some(safe_error(err)),
            },
        }
    }

    fn error<'a>(&self, request: &'a Request, message: &str) -> Response<'a> {
        Response {
            id: &request.id,
            ok: false,
            result: None,
            error: Some(message.into()),
        }
    }

    pub(crate) fn release_surface(&self, surface_id: &str) {
        let app_id = self
            .surface_to_app
            .lock()
            .ok()
            .and_then(|owners| owners.get(surface_id).cloned());
        let Some(app_id) = app_id else {
            return;
        };
        if let Ok(_gate) = self.gates[app_id.as_str()].lock() {
            self.release_surface_inner(surface_id);
        }
    }

    pub(crate) fn release_missing_web_surfaces(&self, active: &HashSet<String>) {
        let missing = self
            .surface_to_app
            .lock()
            .map(|owners| {
                owners
                    .keys()
                    .filter(|id| id.starts_with("surf_") && !active.contains(*id))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for surface_id in missing {
            self.release_surface(&surface_id);
        }
    }

    pub(crate) fn reap_idle_workers(&self) {
        let expired = self
            .idle_since
            .lock()
            .map(|idle| {
                idle.iter()
                    .filter(|(_, since)| since.elapsed() >= Duration::from_secs(60))
                    .map(|(app, _)| app.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for app_id in expired {
            let Ok(_gate) = self.gates[app_id.as_str()].lock() else {
                continue;
            };
            if !self.idle_since.lock().is_ok_and(|idle| {
                idle.get(&app_id)
                    .is_some_and(|since| since.elapsed() >= Duration::from_secs(60))
            }) {
                continue;
            }
            let worker = self
                .workers
                .lock()
                .ok()
                .and_then(|workers| workers.get(&app_id).cloned());
            if let Some(worker) = worker {
                let status = Request {
                    id: "idle-status".into(),
                    method: "app:status".into(),
                    params: serde_json::json!({}),
                };
                if worker.forward_frame(&status).ok().is_some_and(|status| {
                    status["activeRequests"].as_u64().unwrap_or(0) > 0
                        || status["activeTasks"].as_u64().unwrap_or(0) > 0
                }) {
                    continue;
                }
                let stop = Request {
                    id: "last-surface-stop".into(),
                    method: "app:stop".into(),
                    params: serde_json::json!({}),
                };
                let _ = worker.forward_frame(&stop);
            }
            self.workers.lock().unwrap().remove(&app_id);
            self.idle_since.lock().unwrap().remove(&app_id);
            self.instance_to_app
                .lock()
                .unwrap()
                .retain(|_, value| value != &app_id);
        }
    }

    pub(crate) fn has_workers(&self) -> bool {
        self.workers
            .lock()
            .map(|workers| !workers.is_empty())
            .unwrap_or(true)
    }

    fn release_surface_inner(&self, surface_id: &str) {
        let Some(app_id) = self
            .surface_to_app
            .lock()
            .ok()
            .and_then(|mut owners| owners.remove(surface_id))
        else {
            return;
        };
        let last_owner = self
            .surface_to_app
            .lock()
            .is_ok_and(|owners| !owners.values().any(|value| value == &app_id));
        let worker = self
            .workers
            .lock()
            .ok()
            .and_then(|workers| workers.get(&app_id).cloned());
        if let Some(worker) = worker {
            let request = Request {
                id: "surface-release".into(),
                method: "app:session".into(),
                params: serde_json::json!({"op": "revoke_surface", "surfaceId": surface_id}),
            };
            let _ = worker.forward_frame(&request);
            if last_owner {
                self.idle_since
                    .lock()
                    .unwrap()
                    .insert(app_id, Instant::now());
            }
        }
    }
}

fn validated_surface(request: &Request) -> Result<&str, &'static str> {
    if request.id.is_empty() || request.id.len() > 128 {
        return Err("APP_FRAME_INVALID");
    }
    let params = request.params.as_object().ok_or("APP_FRAME_INVALID")?;
    let allowed: &[&str] = match request.method.as_str() {
        "app:handshake" => &[
            "surfaceId",
            "protocolVersion",
            "appId",
            "productVersion",
            "activationGeneration",
        ],
        "app:start" => &[
            "surfaceId",
            "appId",
            "requestId",
            "expectedActivationGeneration",
        ],
        "app:status" | "app:data_status" => &["surfaceId", "appId", "instanceId"],
        "app:card_snapshot" => &["surfaceId", "instanceId", "generation"],
        "app:session" => &["surfaceId", "instanceId", "op", "challenge"],
        "app:secret" => &[
            "surfaceId",
            "instanceId",
            "generation",
            "op",
            "accountId",
            "value",
            "confirm",
        ],
        "app:stop" => &["surfaceId", "instanceId", "reason", "requestId"],
        "app:release" => &["surfaceId"],
        "app:invalidate" => &["surfaceId", "appId", "originId"],
        _ => return Err("APP_METHOD_NOT_ALLOWED"),
    };
    if params.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("APP_FRAME_INVALID");
    }
    let id = params
        .get("surfaceId")
        .and_then(Value::as_str)
        .ok_or("APP_SURFACE_INVALID")?;
    let random = id
        .strip_prefix("native_")
        .or_else(|| id.strip_prefix("surf_"))
        .ok_or("APP_SURFACE_INVALID")?;
    if random.len() != 22
        || !random
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("APP_SURFACE_INVALID");
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maintenance_is_scoped_and_clear_requires_all_surfaces_closed() {
        let manager = AppRuntimeManager::default();
        manager.surface_to_app.lock().unwrap().extend([
            ("e".into(), "fund".into()),
            ("w".into(), "fund".into()),
            ("other".into(), "tokenusage".into()),
        ]);
        assert!(manager.begin_maintenance("fund", false).is_err());
        assert_eq!(manager.surface_to_app.lock().unwrap().len(), 3);
        assert!(manager.begin_maintenance("unknown", true).is_err());
        drop(manager.begin_maintenance("fund", true).unwrap());
        let owners = manager.surface_to_app.lock().unwrap();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners.get("other").unwrap(), "tokenusage");
        drop(owners);
        drop(manager.begin_maintenance("fund", false).unwrap());
    }
    use serde_json::json;

    #[test]
    fn invalidation_requires_own_module_binding_and_never_starts_worker() {
        let manager = AppRuntimeManager::default();
        let mut request = Request {
            id: "change".into(),
            method: "app:invalidate".into(),
            params: json!({"appId":"fund","originId":"sender", "surfaceId":"native_aaaaaaaaaaaaaaaaaaaaaa"}),
        };
        assert!(!manager.dispatch_request(&request).ok);
        manager
            .surface_to_app
            .lock()
            .unwrap()
            .insert("native_aaaaaaaaaaaaaaaaaaaaaa".into(), "fund".into());
        assert!(manager.dispatch_request(&request).ok);
        request.params["appId"] = json!("tokenusage");
        assert!(!manager.dispatch_request(&request).ok);
        request.params["appId"] = json!("fund");
        request.params["path"] = json!("/untrusted");
        assert!(!manager.dispatch_request(&request).ok);
        request.params.as_object_mut().unwrap().remove("path");
        request.params["originId"] = json!("invalid sender");
        assert!(!manager.dispatch_request(&request).ok);
        assert!(manager.workers.lock().unwrap().is_empty());
    }

    #[test]
    fn unknown_or_unowned_app_does_not_spawn_worker() {
        let manager = AppRuntimeManager::default();
        let unknown = Request {
            id: "unknown".into(),
            method: "app:handshake".into(),
            params: json!({"appId": "unregistered", "surfaceId": "native_aaaaaaaaaaaaaaaaaaaaaa"}),
        };
        assert!(!manager.dispatch_request(&unknown).ok);
        let unowned = Request {
            id: "unowned".into(),
            method: "app:session".into(),
            params: json!({"instanceId": "unknown-instance", "surfaceId": "native_aaaaaaaaaaaaaaaaaaaaaa"}),
        };
        assert!(!manager.dispatch_request(&unowned).ok);
        assert!(manager.workers.lock().unwrap().is_empty());
    }

    #[test]
    fn instance_cannot_be_routed_to_another_module() {
        let manager = AppRuntimeManager::default();
        manager
            .instance_to_app
            .lock()
            .unwrap()
            .insert("i1".into(), "fund".into());
        let request = Request {
            id: "cross-app".into(),
            method: "app:status".into(),
            params: json!({"instanceId": "i1", "appId": "tokenusage", "surfaceId": "native_aaaaaaaaaaaaaaaaaaaaaa"}),
        };
        assert_eq!(
            manager.dispatch_request(&request).error.as_deref(),
            Some("instanceId does not belong to appId")
        );
        assert!(manager.workers.lock().unwrap().is_empty());
    }
}
