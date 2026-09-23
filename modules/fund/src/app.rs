//! FundModule：Built-in App Module（ADR-0031）。
//!
//! 生产运行关系：natives-app-runtime（ModuleRegistry）
//!   → FundModule（本模块）→ ledger / portfolio / nav / import / storage。
//! 业务规则完全复用 lib 内既有实现（R-03：Core 不知道基金业务，本模块不被 Core 复制）。

use crate::api;
use crate::migration;
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;
use app_runtime_core::module::{
    AppHealth, BuiltInAppModule, ModuleContext, ModuleDescriptor, ModuleHttpResponse,
};
use app_runtime_core::protocol::DataStatusResult;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub struct FundModule {
    data_dir: Option<PathBuf>,
    /// 产品版本（Natives Product Release，如 "2.5.0"）。
    /// 计划 §27.2：禁止用 Fund Cargo Version 冒充 last_data_writer_version /
    /// migration appVersion / 用户 DB writer version。
    product_version: Option<String>,
    /// 取消信号：NAV/导入等长任务必须轮询（计划 §15/§30）。
    cancellation: Option<std::sync::Arc<app_runtime_core::CancellationToken>>,
    store: Option<Arc<Store>>,
    python_child: Option<std::process::Child>,
}

fn open_ready_store(data_dir: &std::path::Path, product_version: &str) -> Result<Store, String> {
    migration::recover_pending_journal(data_dir).map_err(|e| format!("migration recover: {e}"))?;
    std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {e}"))?;
    let conn = rusqlite::Connection::open(data_dir.join("fund.db"))
        .map_err(|e| format!("open fund.db: {e}"))?;
    conn.busy_timeout(Duration::from_millis(2000))
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    migration::run_migrations(data_dir, &conn, product_version)
        .map_err(|e| format!("APP_MIGRATION_FAILED: {}", e.message()))?;
    drop(conn);
    let store = Store::open(data_dir).map_err(|e| format!("open store: {e}"))?;
    // 计划 §27.2：用户 DB writer 版本 = Natives 产品版本。
    store.set_writer_version(product_version);
    Ok(store)
}

impl FundModule {
    pub fn new() -> Self {
        Self {
            data_dir: None,
            product_version: None,
            cancellation: None,
            store: None,
            python_child: None,
        }
    }

    /// 真实 Health（计划 §28）：只做本地轻量检查——DB 可打开、
    /// `PRAGMA user_version` 与期望 schema 一致、migration journal 无残留。
    /// 禁止发外部网络请求；失败给出可判别状态。
    fn local_health(&self) -> AppHealth {
        let Some(data_dir) = self.data_dir.as_ref() else {
            return AppHealth {
                ok: false,
                detail: "NOT_INITIALIZED".into(),
            };
        };
        let db_path = data_dir.join("fund.db");
        if !db_path.exists() {
            // 未初始化是合法状态（首用前），不是故障。
            return AppHealth {
                ok: true,
                detail: "NOT_INITIALIZED".into(),
            };
        }
        let conn = match rusqlite::Connection::open(&db_path) {
            Ok(conn) => conn,
            Err(error) => {
                return AppHealth {
                    ok: false,
                    detail: format!("FAILED: open fund.db: {error}"),
                };
            }
        };
        let user_version =
            match conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0)) {
                Ok(version) => version,
                Err(error) => {
                    return AppHealth {
                        ok: false,
                        detail: format!("FAILED: PRAGMA user_version: {error}"),
                    };
                }
            };
        let expected = crate::storage::SCHEMA_VERSION;
        if user_version != expected {
            return AppHealth {
                ok: false,
                detail: format!(
                    "MIGRATION_PENDING: user_version={user_version} expected={expected}"
                ),
            };
        }
        // migration journal 残留 → 上次迁移未完成，需恢复后才能 READY。
        if data_dir.join(".migration.json").exists() {
            return AppHealth {
                ok: false,
                detail: "MIGRATION_PENDING: journal present".into(),
            };
        }
        AppHealth {
            ok: true,
            detail: format!("READY schema={user_version}"),
        }
    }
}

impl Default for FundModule {
    fn default() -> Self {
        Self::new()
    }
}

impl BuiltInAppModule for FundModule {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            app_id: "fund".into(),
            display_name: "投资".into(),
            module_api_version: 1,
            data_schema_version: crate::storage::SCHEMA_VERSION as u32,
            capability_version: 1,
        }
    }

    fn initialize(&mut self, context: &ModuleContext) -> Result<(), String> {
        self.data_dir = Some(context.module_data_root.clone());
        // 计划 §27.2：统一使用 Runtime 注入的产品版本（如 "2.5.0"），
        // 不使用 Fund 自身 Cargo Version。
        self.product_version = Some(context.product_version.clone());
        self.cancellation = Some(context.cancellation.clone());
        Ok(())
    }

    fn start(&mut self) -> Result<(), String> {
        let data_dir = self.data_dir.as_ref().ok_or("module not initialized")?;
        let product_version = self
            .product_version
            .as_deref()
            .ok_or("module not initialized")?;
        let store = open_ready_store(data_dir, product_version)?;
        self.store = Some(Arc::new(store));

        // 启动伴生 Python 趋势分析引擎子进程
        let engine_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("engine");
        let app_py = engine_dir.join("app.py");
        if app_py.exists() {
            match std::process::Command::new("python3")
                .arg(&app_py)
                .arg("--port")
                .arg("8795")
                .current_dir(&engine_dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    self.python_child = Some(child);
                }
                Err(e) => {
                    eprintln!("Warning: failed to spawn trends python engine: {e}");
                }
            }
        }

        Ok(())
    }

    fn handle_ui(&self, path: &str) -> Result<Option<ModuleHttpResponse>, String> {
        if path == "/" || path == "/index.html" {
            Ok(Some(ModuleHttpResponse::ok_html(crate::ui::index_html())))
        } else if let Some(js) = crate::ui::static_js(path) {
            // 按能力域拆分的 ES module 静态资源（见 ui/dist/）。
            Ok(Some(ModuleHttpResponse {
                status_code: 200,
                reason: "OK".into(),
                content_type: "application/javascript; charset=utf-8".into(),
                body: js.into_bytes(),
            }))
        } else {
            Ok(None)
        }
    }

    fn handle_api(
        &self,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        // 计划 §30：长任务入口先检查取消信号——Runtime 正在关闭时
        // 不再发起新的网络/导入工作，尽快返回以便进程退出。
        if let Some(cancel) = self.cancellation.as_ref() {
            if cancel.is_cancelled() {
                return Err((503, "APP_RUNTIME_STOPPING".into()));
            }
        }
        let store = self.store.as_ref().ok_or((503, "store not ready".into()))?;
        match api::handle_api_request(store, req, route) {
            Ok(body) => Ok(ModuleHttpResponse::ok_json(body.into_bytes())),
            Err((code, body)) => Err((code, body)),
        }
    }

    fn data_status(&self) -> Result<DataStatusResult, String> {
        let data_dir = self.data_dir.as_ref().ok_or("module not initialized")?;
        let product_version = self
            .product_version
            .as_deref()
            .ok_or("module not initialized")?;
        let status = migration::read_data_status(data_dir, product_version);
        Ok(DataStatusResult {
            current_schema: status.current_schema.max(0) as u32,
            migration_state: status.migration_state,
            last_data_writer_version: status.last_data_writer_version,
            has_committed_new_writes: status.has_committed_new_writes,
            previous_version_compatible: status.previous_version_compatible,
        })
    }

    fn health(&self) -> Result<AppHealth, String> {
        Ok(self.local_health())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.store = None;
        if let Some(mut child) = self.python_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

impl Drop for FundModule {
    fn drop(&mut self) {
        if let Some(mut child) = self.python_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
