//! TokenUsageModule：Built-in App Module 实现（ADR-0031 / ADR-0033 / 架构 Wave 2）。

use crate::api;
use crate::model_host::ModelUsageQueryClient;
use crate::storage::{Store, SCHEMA_VERSION};
use crate::ui;
use app_runtime_core::http::HttpRequest;
use app_runtime_core::module::{
    AppHealth, BuiltInAppModule, ModuleContext, ModuleDescriptor, ModuleHttpResponse,
};
use app_runtime_core::protocol::DataStatusResult;
use std::path::PathBuf;
use std::sync::Arc;

pub struct TokenUsageModule {
    data_dir: Option<PathBuf>,
    product_version: Option<String>,
    cancellation: Option<Arc<app_runtime_core::CancellationToken>>,
    store: Option<Arc<Store>>,
    client: ModelUsageQueryClient,
}

impl TokenUsageModule {
    pub fn new() -> Self {
        Self {
            data_dir: None,
            product_version: None,
            cancellation: None,
            store: None,
            client: ModelUsageQueryClient::new(),
        }
    }

    pub fn with_client(client: ModelUsageQueryClient) -> Self {
        Self {
            data_dir: None,
            product_version: None,
            cancellation: None,
            store: None,
            client,
        }
    }

    fn local_health(&self) -> AppHealth {
        let Some(data_dir) = self.data_dir.as_ref() else {
            return AppHealth {
                ok: false,
                detail: "NOT_INITIALIZED".into(),
            };
        };
        let db_path = data_dir.join("tokenusage.db");
        if !db_path.exists() {
            return AppHealth {
                ok: true,
                detail: "NOT_INITIALIZED".into(),
            };
        }
        let conn = match rusqlite::Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                return AppHealth {
                    ok: false,
                    detail: format!("FAILED: open tokenusage.db: {e}"),
                };
            }
        };
        let user_version: i64 = match conn.query_row("PRAGMA user_version", [], |row| row.get(0)) {
            Ok(v) => v,
            Err(e) => {
                return AppHealth {
                    ok: false,
                    detail: format!("FAILED: PRAGMA user_version: {e}"),
                };
            }
        };
        if user_version != SCHEMA_VERSION {
            return AppHealth {
                ok: false,
                detail: format!(
                    "MIGRATION_PENDING: user_version={user_version} expected={SCHEMA_VERSION}"
                ),
            };
        }
        AppHealth {
            ok: true,
            detail: format!("READY schema={user_version}"),
        }
    }
}

impl Default for TokenUsageModule {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TokenUsageService {
    store: Arc<Store>,
    client: ModelUsageQueryClient,
    cancellation: Option<Arc<app_runtime_core::CancellationToken>>,
}

impl app_runtime_core::module::ModuleService for TokenUsageService {
    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String> {
        if path == "/" || path == "/index.html" {
            return Ok(Some(ModuleHttpResponse::ok_html(ui::index_html())));
        }
        if let Some(js) = ui::static_js(path) {
            let mut resp = ModuleHttpResponse::ok_json(js.into_bytes());
            resp.content_type = "application/javascript; charset=utf-8".into();
            return Ok(Some(resp));
        }
        if let Some(css) = ui::static_css(path) {
            let mut response = ModuleHttpResponse::ok_json(css.as_bytes().to_vec());
            response.content_type = "text/css; charset=utf-8".into();
            return Ok(Some(response));
        }
        Ok(None)
    }

    fn handle_api(
        &self,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        if let Some(cancel) = self.cancellation.as_ref() {
            if cancel.is_cancelled() {
                return Err((503, "APP_RUNTIME_STOPPING".into()));
            }
        }
        let json = api::handle_api_request(&self.store, &self.client, req, route)?;
        Ok(ModuleHttpResponse::ok_json(json.into_bytes()))
    }

    fn handle_authenticated_api(
        &self,
        context: &app_runtime_core::module::RequestContext,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        if context.app_id != "tokenusage" {
            return Err((403, "APP_REQUEST_CONTEXT_INVALID".into()));
        }
        self.handle_api(req, route)
    }
}

impl BuiltInAppModule for TokenUsageModule {
    fn descriptor(&self) -> ModuleDescriptor {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../module.json"))
            .expect("tokenusage module.json must be valid");
        ModuleDescriptor {
            app_id: manifest["appId"].as_str().unwrap().into(),
            display_name: manifest["displayName"]["zh_CN"].as_str().unwrap().into(),
            module_api_version: manifest["moduleApiVersion"].as_u64().unwrap() as u32,
            data_schema_version: manifest["dataSchemaVersion"].as_u64().unwrap() as u32,
            capability_version: manifest["capabilityVersion"].as_u64().unwrap() as u32,
        }
    }

    fn initialize(&mut self, context: &ModuleContext) -> Result<(), String> {
        self.data_dir = Some(context.module_data_root.clone());
        self.product_version = Some(context.product_version.clone());
        self.cancellation = Some(context.cancellation.clone());
        Ok(())
    }

    fn start(
        &mut self,
    ) -> Result<std::sync::Arc<dyn app_runtime_core::module::ModuleService>, String> {
        let Some(data_dir) = self.data_dir.as_ref() else {
            return Err("not initialized".into());
        };
        let product_version = self.product_version.as_deref().unwrap_or("unknown");
        let store = Arc::new(Store::open(data_dir).map_err(|e| format!("open store: {e}"))?);
        store.set_writer_version(product_version);
        self.store = Some(store.clone());
        let service = Arc::new(TokenUsageService {
            store,
            client: self.client.clone(),
            cancellation: self.cancellation.clone(),
        });
        Ok(service)
    }

    fn health(&self) -> Result<AppHealth, String> {
        Ok(self.local_health())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.store = None;
        Ok(())
    }

    fn data_status(&self) -> Result<DataStatusResult, String> {
        let health = self.local_health();
        let writer_version = self
            .store
            .as_ref()
            .map(|s| s.writer_version())
            .unwrap_or_else(|| "unknown".into());

        Ok(DataStatusResult {
            current_schema: SCHEMA_VERSION as u32,
            migration_state: if health.ok {
                "ready".into()
            } else {
                "pending".into()
            },
            last_data_writer_version: writer_version,
            has_committed_new_writes: false,
            previous_version_compatible: true,
            readiness: Some(if health.ok {
                "ready".into()
            } else {
                "recovery_required".into()
            }),
        })
    }
}
