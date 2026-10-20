//! Model Host 子进程调度与请求中继（ADR-0042 / F03）。
//! 负责在独立 Web 模式下将 model_* 方法安全中继至单实例 model-host。

use crate::protocol::{safe_error, Request, Response};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub fn resolve_model_host_binary() -> io::Result<PathBuf> {
    if let Ok(bin) = std::env::var("NATIVES_MODEL_HOST_BIN") {
        let p = PathBuf::from(bin);
        if p.is_file() {
            return Ok(p);
        }
    }

    let product_binary = crate::app_product::default_system_source().join("model-host");
    if product_binary.is_file() {
        return Ok(product_binary);
    }

    if !cfg!(debug_assertions) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "model-host binary not found in product source",
        ));
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(manifest_dir);

    let candidates = [
        repo_root.join("target/debug/model-host"),
        repo_root.join("target/release/model-host"),
    ];

    for c in candidates {
        if c.is_file() {
            return Ok(c);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "model-host binary not found in product source or local target directories",
    ))
}

pub struct ModelHostManager {
    workers:
        Mutex<std::collections::HashMap<String, std::sync::Arc<super::model_worker::ModelWorker>>>,
    changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Default for ModelHostManager {
    fn default() -> Self {
        Self {
            workers: Mutex::new(Default::default()),
            changed: Default::default(),
        }
    }
}
impl ModelHostManager {
    pub(crate) fn take_changed(&self) -> bool {
        self.changed
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }
    pub(crate) fn release_surface(&self, surface: &str) {
        if let Ok(mut workers) = self.workers.lock() {
            if let Some(worker) = workers.remove(surface) {
                worker.stop();
            }
        }
    }
    pub(crate) fn release_missing(&self, active: &std::collections::HashSet<String>) {
        let missing = self
            .workers
            .lock()
            .map(|workers| {
                workers
                    .keys()
                    .filter(|id| !active.contains(*id))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in missing {
            self.release_surface(&id);
        }
    }
    pub(crate) fn dispatch_request<'a>(&self, request: &'a Request, surface: &str) -> Response<'a> {
        let result = (|| {
            if !request.params.is_object() || request.id.len() > 128 || request.method.len() > 80 {
                return Err("MODEL_REQUEST_INVALID".into());
            }
            if surface.is_empty() {
                return Err("ERR_SURFACE_NOT_OWNED".into());
            }
            let worker = {
                let mut workers = self.workers.lock().map_err(|_| "MODEL_HOST_UNAVAILABLE")?;
                if workers
                    .get(surface)
                    .is_some_and(|worker| !worker.is_alive())
                {
                    workers.remove(surface);
                }
                if !workers.contains_key(surface) {
                    if workers.len() >= 24 {
                        return Err("MODEL_SURFACE_LIMIT".into());
                    }
                    let worker = super::model_worker::ModelWorker::spawn(self.changed.clone())
                        .map_err(|_| "MODEL_HOST_UNAVAILABLE")?;
                    workers.insert(surface.into(), std::sync::Arc::new(worker));
                }
                workers
                    .get(surface)
                    .cloned()
                    .ok_or("MODEL_HOST_UNAVAILABLE")?
            };
            if request.method == "model_surface_events" {
                return Ok(worker.events());
            }
            if request.method == "model_oauth_cancel"
                && !worker.owns_oauth(request.params["sessionId"].as_str().unwrap_or(""))
            {
                return Err("MODEL_TASK_NOT_OWNED".into());
            }
            let mut owned = request.clone();
            if matches!(
                request.method.as_str(),
                "model_inference_start" | "model_inference_status" | "model_inference_cancel"
            ) {
                owned.params["owner"] =
                    Value::String(app_runtime_core::sha256_hex(surface.as_bytes()));
            }
            worker.call(&owned)
        })();
        match result {
            Ok(result) => Response {
                id: &request.id,
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some(safe_error(error)),
            },
        }
    }
}
