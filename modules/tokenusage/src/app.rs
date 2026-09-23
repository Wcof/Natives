//! TokenUsageModule：Built-in App Module 实现（ADR-0031 / 技术 06）。

use crate::api;
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
}

impl TokenUsageModule {
    pub fn new() -> Self {
        Self {
            data_dir: None,
            product_version: None,
            cancellation: None,
            store: None,
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

impl BuiltInAppModule for TokenUsageModule {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            app_id: "tokenusage".into(),
            display_name: "Token Usage".into(),
            module_api_version: 1,
            data_schema_version: 1,
            capability_version: 1,
        }
    }

    fn initialize(&mut self, context: &ModuleContext) -> Result<(), String> {
        self.data_dir = Some(context.module_data_root.clone());
        self.product_version = Some(context.product_version.clone());
        self.cancellation = Some(context.cancellation.clone());
        Ok(())
    }

    fn start(&mut self) -> Result<(), String> {
        let Some(data_dir) = self.data_dir.as_ref() else {
            return Err("not initialized".into());
        };
        let product_version = self.product_version.as_deref().unwrap_or("unknown");
        let store = Store::open(data_dir).map_err(|e| format!("open store: {e}"))?;
        store.set_writer_version(product_version);
        let _ = crate::collector::scanner::seed_sources(&store);
        // 启动时自动同步 Model Host usage.db 权威数据及本地扫描，保证开箱即有数据
        let _ = crate::collector::collect_all(&store, None, None);
        self.store = Some(Arc::new(store));
        Ok(())
    }

    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String> {
        if path == "/" || path == "/index.html" {
            return Ok(Some(ModuleHttpResponse::ok_html(ui::index_html())));
        }
        if let Some(js) = ui::static_js(path) {
            let mut resp = ModuleHttpResponse::ok_json(js.into_bytes());
            resp.content_type = "application/javascript; charset=utf-8".into();
            return Ok(Some(resp));
        }
        Ok(None)
    }

    fn handle_api(
        &self,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        let Some(store) = self.store.as_ref() else {
            return Err(api::api_error(503, "NOT_STARTED", "module not started"));
        };
        let json = api::handle_api_request(store, req, route)?;
        Ok(ModuleHttpResponse::ok_json(json.into_bytes()))
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
        })
    }

    fn health(&self) -> Result<AppHealth, String> {
        Ok(self.local_health())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.store = None;
        self.data_dir = None;
        self.cancellation = None;
        Ok(())
    }
}
