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
use app_runtime_core::protocol::{CardSnapshotResult, DataStatusResult};
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
    information_tasks: Option<Arc<crate::information::tasks::Tasks>>,
    opportunity_tasks: Option<Arc<crate::opportunity_refresh::Tasks>>,
    notification_tasks: Option<Arc<crate::notification_tasks::Tasks>>,
    secrets: Option<std::sync::Arc<app_runtime_core::secrets::SecretStore>>,
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
            information_tasks: None,
            opportunity_tasks: None,
            notification_tasks: None,
            secrets: None,
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
        // committed 是迁移成功后的正常残留（read_data_status 同样放行），
        // 不能让它把健康状态永久判为 not-ok。
        if let Some(journal) = crate::migration::read_journal(&data_dir.join(".migration.json")) {
            if journal.state != "committed" {
                return AppHealth {
                    ok: false,
                    detail: format!("MIGRATION_PENDING: journal state={}", journal.state),
                };
            }
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

pub struct FundService {
    surface_tasks: crate::surface_tasks::TaskOwners,
    store: Arc<Store>,
    network: Arc<crate::network::Context>,
    information_tasks: Arc<crate::information::tasks::Tasks>,
    opportunity_tasks: Arc<crate::opportunity_refresh::Tasks>,
    notification_tasks: Arc<crate::notification_tasks::Tasks>,
    cancellation: Option<std::sync::Arc<app_runtime_core::CancellationToken>>,
    secrets: Option<std::sync::Arc<app_runtime_core::secrets::SecretStore>>,
}

impl app_runtime_core::module::ModuleService for FundService {
    fn handle_authenticated_api(
        &self,
        context: &app_runtime_core::module::RequestContext,
        req: &HttpRequest,
        route: &str,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        if context.app_id != "fund" {
            return Err((403, "APP_REQUEST_CONTEXT_INVALID".into()));
        }
        let Some((kind, start)) = crate::surface_tasks::Kind::route(&req.method, route) else {
            return self.handle_api(req, route);
        };
        let finished = match kind {
            crate::surface_tasks::Kind::Information => self.information_tasks.is_finished(),
            crate::surface_tasks::Kind::Opportunity => self.opportunity_tasks.is_finished(),
            crate::surface_tasks::Kind::Notification => self.notification_tasks.is_finished(),
        };
        self.surface_tasks.run(
            (&context.surface_id, &context.generation),
            kind,
            start,
            finished,
            req,
            || self.handle_api(req, route),
        )
    }

    fn release_surface(&self, surface_id: &str) {
        for (kind, task_id) in self.surface_tasks.release(surface_id) {
            match kind {
                crate::surface_tasks::Kind::Information => {
                    self.information_tasks.cancel_task(task_id)
                }
                crate::surface_tasks::Kind::Opportunity => {
                    self.opportunity_tasks.cancel_task(task_id)
                }
                crate::surface_tasks::Kind::Notification => {
                    self.notification_tasks.cancel_task(task_id)
                }
            }
        }
    }

    fn protected_task_count(&self) -> usize {
        [
            self.information_tasks.is_finished(),
            self.opportunity_tasks.is_finished(),
            self.notification_tasks.is_finished(),
        ]
        .iter()
        .filter(|finished| !**finished)
        .count()
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
        let _network_scope = crate::network::bind(Some(self.network.clone()));
        let _network_budget = crate::network::limit_duration(std::time::Duration::from_secs(20));
        // 计划 §30：长任务入口先检查取消信号——Runtime 正在关闭时
        // 不再发起新的网络/导入工作，尽快返回以便进程退出。
        if let Some(cancel) = self.cancellation.as_ref() {
            if cancel.is_cancelled() {
                return Err((503, "APP_RUNTIME_STOPPING".into()));
            }
        }
        if route == "/api/settings/network" || route == "/api/settings/network/probe" {
            let input = serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
            let result = match (req.method.as_str(), route) {
                ("GET", "/api/settings/network") => self.network.config(),
                ("POST", "/api/settings/network/probe") => self.network.probe(&input),
                ("POST", "/api/settings/network") => self.network.replay(&input).and_then(|old| {
                    if let Some(old) = old {
                        return Ok(old);
                    }
                    self.network.validate_save(&input).and_then(|()| {
                        self.information_tasks.cancel();
                        self.opportunity_tasks.cancel();
                        self.notification_tasks.cancel();
                        self.network.save(&input)
                    })
                }),
                _ => Err(crate::finance::FinanceError::Invalid(
                    "网络设置方法无效".into(),
                )),
            };
            return result
                .map(|v| ModuleHttpResponse::ok_json(v.to_string().into_bytes()))
                .map_err(|e| {
                    api::home_api::err(if e.code() == "FIN_CONFLICT" { 409 } else { 400 }, &e)
                });
        }
        if let Some(result) = self
            .information_tasks
            .handle(self.store.clone(), req, route)
        {
            return result.map(|body| ModuleHttpResponse::ok_json(body.into_bytes()));
        }
        if let Some(result) = self
            .opportunity_tasks
            .handle(self.store.clone(), req, route)
        {
            return result.map(|body| ModuleHttpResponse::ok_json(body.into_bytes()));
        }
        if let Some(result) =
            self.notification_tasks
                .handle(self.store.clone(), self.secrets.clone(), req, route)
        {
            return result.map(|body| ModuleHttpResponse::ok_json(body.into_bytes()));
        }
        match api::handle_api_request(&self.store, self.secrets.as_ref(), req, route) {
            Ok(body) => {
                let mut json: serde_json::Value = serde_json::from_str(&body)
                    .map_err(|_| (500, "APP_RESPONSE_INVALID".into()))?;
                crate::home::wire_numbers(&mut json);
                Ok(ModuleHttpResponse::ok_json(json.to_string().into_bytes()))
            }
            Err((code, body)) => Err((code, body)),
        }
    }

    /// 白名单只读卡片投影（ADR-0036）：与详情页复用同一查询与定点计算；
    /// 单一读快照，不新开 writer，不复制业务计算到 Core。
    fn card_snapshot(&self) -> Result<CardSnapshotResult, (u16, String)> {
        if let Some(cancel) = self.cancellation.as_ref() {
            if cancel.is_cancelled() {
                return Err((503, "APP_RUNTIME_STOPPING".into()));
            }
        }
        self.store
            .with_snapshot(crate::cards::build_snapshot)
            .map_err(|e| {
                (
                    500,
                    format!("APP_CARD_SNAPSHOT_FAILED: {} {}", e.code(), e.message()),
                )
            })
    }
}

impl BuiltInAppModule for FundModule {
    fn descriptor(&self) -> ModuleDescriptor {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../module.json"))
            .expect("fund module.json must be valid");
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
        self.secrets = Some(context.secrets.clone());
        // 计划 §27.2：统一使用 Runtime 注入的产品版本（如 "2.5.0"），
        // 不使用 Fund 自身 Cargo Version。
        self.product_version = Some(context.product_version.clone());
        self.cancellation = Some(context.cancellation.clone());
        Ok(())
    }

    fn start(
        &mut self,
    ) -> Result<std::sync::Arc<dyn app_runtime_core::module::ModuleService>, String> {
        let data_dir = self.data_dir.as_ref().ok_or("module not initialized")?;
        let product_version = self
            .product_version
            .as_deref()
            .ok_or("module not initialized")?;
        let store = Arc::new(open_ready_store(data_dir, product_version)?);
        self.store = Some(store.clone());
        let information_tasks = Arc::new(crate::information::tasks::Tasks::new(
            self.cancellation.clone(),
        ));
        self.information_tasks = Some(information_tasks.clone());
        let opportunity_tasks = Arc::new(crate::opportunity_refresh::Tasks::with_discovery(
            self.cancellation.clone(),
            true,
        ));
        self.opportunity_tasks = Some(opportunity_tasks.clone());
        let notification_tasks = Arc::new(crate::notification_tasks::Tasks::new(
            self.cancellation.clone(),
        ));
        self.notification_tasks = Some(notification_tasks.clone());
        let service = Arc::new(FundService {
            surface_tasks: crate::surface_tasks::TaskOwners::default(),
            network: crate::network::Context::new(store.clone(), self.secrets.clone()),
            notification_tasks,
            store,
            information_tasks,
            opportunity_tasks,
            cancellation: self.cancellation.clone(),
            secrets: self.secrets.clone(),
        });
        Ok(service)
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
            readiness: Some(status.readiness),
        })
    }

    fn health(&self) -> Result<AppHealth, String> {
        Ok(self.local_health())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        if let Some(tasks) = self.information_tasks.take() {
            tasks.cancel();
        }
        if let Some(tasks) = self.opportunity_tasks.take() {
            tasks.cancel();
        }
        // mode/T03：通知任务 owner 有界等待执行句柄（共同 deadline；
        // stopping 后新任务被拒、迟到结果不写），不用进程退出冒充收尾。
        // 超时任务仍持有句柄；如实记录，最终受 Runtime EOF 退出预算约束。
        if let Some(tasks) = self.notification_tasks.take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            let report = tasks.close(deadline);
            if report.pending > 0 {
                eprintln!(
                    "[fund] notification task close: joined={} pending={} (bounded wait expired)",
                    report.joined, report.pending
                );
            }
        }
        self.store = None;
        Ok(())
    }
}
