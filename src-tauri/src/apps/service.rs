//! AppsService（APP-014 / APP-017 / APP-018 / APP-019）
//!
//! 应用中心统一业务编排与分派入口。
//!
//! 职责：
//! 1. 按 AppKind 分派到 Local / System / Web 三类驱动；
//! 2. 统一并发控制（基于 application_id 的 MutationLock）；
//! 3. 统一变更事件广播（db-state-changed channel = apps）；
//! 4. 强制执行动作矩阵与能力校验（Web 禁 start/stop/restart，System force stop 过平台能力门）。

use rusqlite::Connection;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use super::capabilities::CapabilityResolver;
use super::local;
use super::model::{
    AppKind, AppRuntimeState, AppView, RegisterLocalProjectInput, RegisterSystemApplicationInput,
    RegisterWebApplicationInput, RegistrationOrigin, RuntimeInstance, RuntimeSpec, Surface,
    UpdateAppMetadataInput, UpdateSystemApplicationSpecInput, UpdateWebApplicationSpecInput,
};
use super::mutation_lock::MutationLockRegistry;
use super::repository::AppRepository;
use super::system;
use super::web;
use crate::creative_app::browser::BrowserStateHandle;
use crate::creative_app::local::LocalRuntimeHandle;
use crate::db::DbPool;
use crate::{Error, Result};

/// AppsService 运行依赖包。
///
/// APP-03：`browser` 是 **共享** managed 实例的借用（`lib.rs`
/// `app.manage(Mutex::new(BrowserState::new()))`，Tauri state 键为
/// `Mutex<BrowserState>`）。所有命令在同一 Host 进程内借用到同一个
/// `Mutex<BrowserState>`，open / close / hide / budget / LRU 休眠判定因此读写
/// **同一运行时实例**——严禁在 `service()` 里每次 `BrowserState::default()`
/// 新建（状态分裂：一个命令 open、另一个命令的空 state 无法感知 → budget 判定
/// 失真 / `browser_close` 清不到条目）。
#[derive(Clone)]
pub struct AppsServiceDeps<'a> {
    pub db: DbPool,
    pub locks: Arc<MutationLockRegistry>,
    pub local_runtime: LocalRuntimeHandle,
    pub browser: &'a BrowserStateHandle,
    pub app_handle: AppHandle,
    pub host_http_port: u16,
}

/// 应用中心统一业务服务。
#[derive(Clone)]
pub struct AppsService<'a> {
    deps: AppsServiceDeps<'a>,
}

impl<'a> AppsService<'a> {
    pub fn new(deps: AppsServiceDeps<'a>) -> Self {
        Self { deps }
    }

    fn conn(&self) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
        self.deps
            .db
            .get()
            .map_err(|e| Error::Internal(e.to_string()))
    }

    fn emit(&self, action: &str, app_id: &str, kind: AppKind) {
        super::emit_apps_event(&self.deps.app_handle, action, app_id, kind.as_str());
    }

    /// 构建单个应用的 AppView 投影。
    pub fn build_view(conn: &Connection, app_id: &str) -> Result<AppView> {
        let app =
            AppRepository::get(conn, app_id)?.ok_or_else(|| Error::NotFound(app_id.into()))?;
        let (kind_opt, origin_opt, show, order) = AppRepository::identity(conn, app_id)?;
        let kind = kind_opt.unwrap_or(AppKind::LocalProject);
        let kind_str = kind.as_str();

        let runtime_state = match kind {
            AppKind::WebApplication => {
                let windows = web::list_windows(conn, app_id)?;
                let any_hib = web::any_hibernated(conn, app_id)?;
                super::capabilities::web_runtime_state_from_windows(&windows, any_hib)
            }
            AppKind::LocalProject | AppKind::SystemApplication => {
                let active = AppRepository::active_instance(conn, app_id)?;
                AppRuntimeState::from_instance_status(active.as_ref().map(|i| i.status.as_str()))
            }
        };

        let capabilities =
            CapabilityResolver::resolve(kind_str, runtime_state, system::force_stop_supported());

        Ok(AppView {
            app_id: app.id,
            title: app.title,
            kind: kind_str.to_string(),
            registration_origin: origin_opt
                .map(|o| o.as_str().to_string())
                .unwrap_or_default(),
            description: app.description,
            show_in_sidebar: show,
            sidebar_order: order,
            capabilities,
            runtime_state,
            updated_at: app.updated_at,
        })
    }

    // ── 查询接口 ─────────────────────────────────────────────────────────

    /// 获取全部注册应用视图（包含能力与运行态）。
    pub fn list_views(&self) -> Result<Vec<AppView>> {
        let conn = self.conn()?;
        let apps = AppRepository::list(&conn)?;
        let mut views = Vec::with_capacity(apps.len());
        for app in apps {
            if let Ok(view) = Self::build_view(&conn, &app.id) {
                views.push(view);
            }
        }
        Ok(views)
    }

    /// 获取单个注册应用视图。
    pub fn get_view(&self, id: &str) -> Result<Option<AppView>> {
        let conn = self.conn()?;
        match Self::build_view(&conn, id) {
            Ok(v) => Ok(Some(v)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 获取指定应用的运行实例列表。
    pub fn list_instances(&self, application_id: &str) -> Result<Vec<RuntimeInstance>> {
        let conn = self.conn()?;
        AppRepository::list_instances(&conn, application_id)
    }

    /// 获取指定应用的呈现表面列表。
    pub fn list_surfaces(&self, application_id: &str) -> Result<Vec<Surface>> {
        let conn = self.conn()?;
        AppRepository::list_surfaces(&conn, application_id)
    }

    /// 获取指定应用的当前生效 RuntimeSpec。
    pub fn active_spec(&self, application_id: &str) -> Result<Option<RuntimeSpec>> {
        let conn = self.conn()?;
        AppRepository::active_spec(&conn, application_id)
    }

    pub async fn observe_system(&self, id: &str) -> Result<system::SystemRunningState> {
        let spec = {
            let conn = self.conn()?;
            let (kind, _, _, _) = AppRepository::identity(&conn, id)?;
            if kind != Some(AppKind::SystemApplication) {
                return Err(Error::InvalidInput(format!(
                    "observe is only available for system applications (app {id})"
                )));
            }
            AppRepository::load_system_spec(&conn, id)?
        };
        system::driver()
            .ok_or_else(|| system::unsupported("SystemDriver not available on this platform"))?
            .observe(&spec.application_path, spec.bundle_identifier.as_deref())
            .await
    }

    pub async fn dock_system(
        &self,
        id: &str,
        bounds: crate::creative_app::model_runtime::BrowserBounds,
    ) -> Result<system::DockResult> {
        const SAFE_INSET: f64 = 8.0;
        let spec = {
            let conn = self.conn()?;
            let (kind, _, _, _) = AppRepository::identity(&conn, id)?;
            if kind != Some(AppKind::SystemApplication) {
                return Err(Error::InvalidInput(format!(
                    "dock is only available for system applications (app {id})"
                )));
            }
            AppRepository::load_system_spec(&conn, id)?
        };
        let window = self
            .deps
            .app_handle
            .get_webview_window("main")
            .ok_or_else(|| Error::NotFound("main window".into()))?;
        let scale = window
            .scale_factor()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let origin = window
            .inner_position()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let rect = system::SystemDockRect {
            x: origin.x as f64 / scale + bounds.x + SAFE_INSET,
            y: origin.y as f64 / scale + bounds.y + SAFE_INSET,
            width: bounds.width - SAFE_INSET * 2.0,
            height: bounds.height - SAFE_INSET * 2.0,
        };
        system::driver()
            .ok_or_else(|| system::unsupported("SystemDriver not available on this platform"))?
            .dock(
                &spec.application_path,
                spec.bundle_identifier.as_deref(),
                rect,
            )
            .await
    }

    // ── 注册与元数据更新 ──────────────────────────────────────────────────

    /// 注册本地项目应用。
    pub fn register_local(&self, input: RegisterLocalProjectInput) -> Result<AppView> {
        let mut conn = self.conn()?;
        let source = local::register(
            &mut conn,
            &input.title,
            &input.project_root,
            input.description.as_deref(),
            input.icon.as_deref(),
            crate::creative_app::model::LaunchMode::Smart,
            None,
            &[],
        )?;

        let app_id = AppRepository::register_local(
            &conn,
            &input.title,
            &source.id,
            input.description.as_deref(),
            input.icon.as_deref(),
            RegistrationOrigin::Manual,
        )?;

        self.emit("registered", &app_id, AppKind::LocalProject);
        Self::build_view(&conn, &app_id)
    }

    /// 注册系统应用。
    pub fn register_system(&self, input: RegisterSystemApplicationInput) -> Result<AppView> {
        let conn = self.conn()?;
        let app_id = AppRepository::register_system(
            &conn,
            &input.title,
            &input.application_path,
            input.bundle_identifier.as_deref(),
            &input.platform,
            input.launch_policy.as_deref(),
            RegistrationOrigin::Manual,
        )?;

        self.emit("registered", &app_id, AppKind::SystemApplication);
        Self::build_view(&conn, &app_id)
    }

    /// 注册 Web 应用。
    pub fn register_web(&self, input: RegisterWebApplicationInput) -> Result<AppView> {
        let conn = self.conn()?;
        let app_id = AppRepository::register_web(
            &conn,
            &input.title,
            &input.url,
            &input.approved_origins,
            input.open_behavior.as_deref(),
            input.keep_alive,
        )?;

        self.emit("registered", &app_id, AppKind::WebApplication);
        Self::build_view(&conn, &app_id)
    }

    /// 更新应用公共元数据。
    pub fn update_metadata(&self, input: UpdateAppMetadataInput) -> Result<AppView> {
        let mut conn = self.conn()?;
        let (kind_opt, _, _, _) = AppRepository::identity(&conn, &input.app_id)?;
        let kind = kind_opt.unwrap_or(AppKind::LocalProject);

        AppRepository::update_metadata(&mut conn, &input)?;
        self.emit("updated", &input.app_id, kind);
        Self::build_view(&conn, &input.app_id)
    }

    /// 更新系统应用 Spec。
    pub fn update_system_spec(&self, input: UpdateSystemApplicationSpecInput) -> Result<AppView> {
        let mut conn = self.conn()?;
        let (kind_opt, _, _, _) = AppRepository::identity(&conn, &input.app_id)?;
        if kind_opt != Some(AppKind::SystemApplication) {
            return Err(Error::InvalidInput("not a system application".into()));
        }

        AppRepository::update_system_spec(
            &mut conn,
            &input.app_id,
            input.application_path.as_deref(),
            input.bundle_identifier.as_deref().map(Some),
            input.platform.as_deref(),
            input.launch_policy.as_deref(),
        )?;

        self.emit("updated", &input.app_id, AppKind::SystemApplication);
        Self::build_view(&conn, &input.app_id)
    }

    /// 更新 Web 应用 Spec。
    pub fn update_web_spec(&self, input: UpdateWebApplicationSpecInput) -> Result<AppView> {
        let mut conn = self.conn()?;
        let (kind_opt, _, _, _) = AppRepository::identity(&conn, &input.app_id)?;
        if kind_opt != Some(AppKind::WebApplication) {
            return Err(Error::InvalidInput("not a web application".into()));
        }

        AppRepository::update_web_spec(
            &mut conn,
            &input.app_id,
            input.url.as_deref(),
            input.approved_origins.as_deref(),
            input.open_behavior.as_deref(),
            input.keep_alive,
        )?;

        self.emit("updated", &input.app_id, AppKind::WebApplication);
        Self::build_view(&conn, &input.app_id)
    }

    /// 设置侧栏显示与排序。
    pub fn set_sidebar(&self, id: &str, show: Option<bool>, order: Option<i64>) -> Result<AppView> {
        let mut conn = self.conn()?;
        let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
        let kind = kind_opt.unwrap_or(AppKind::LocalProject);

        AppRepository::set_sidebar(&mut conn, id, show, order)?;
        self.emit("sidebar", id, kind);
        Self::build_view(&conn, id)
    }

    // ── 生命周期动作 ──────────────────────────────────────────────────────

    /// 统一打开入口：
    /// - Local: 若正在运行则打开 open_url，若停止则启动后打开；
    /// - System: 激活或启动原生应用；
    /// - Web: 打开受控 Child WebView 窗口（`bounds` = Renderer 实测内容区矩形，
    ///   APPV2-T03「先 bounds 后 show」）。
    pub async fn open(
        &self,
        id: &str,
        bounds: Option<crate::creative_app::model_runtime::BrowserBounds>,
    ) -> Result<bool> {
        let kind = {
            let conn = self.conn()?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            let _app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            kind_opt.unwrap_or(AppKind::LocalProject)
        };

        match kind {
            AppKind::LocalProject => {
                let running_url = {
                    let conn = self.conn()?;
                    let active = AppRepository::active_instance(&conn, id)?;
                    if let Some(inst) = active {
                        if inst.status == "running" {
                            AppRepository::list_surfaces(&conn, id)?
                                .into_iter()
                                .find_map(|s| s.url)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(url) = running_url {
                    let _ = open::that(url);
                    return Ok(true);
                }

                self.start(id).await
            }
            AppKind::SystemApplication => {
                let spec = {
                    let conn = self.conn()?;
                    AppRepository::load_system_spec(&conn, id)?
                };
                let driver = system::driver().ok_or_else(|| {
                    system::unsupported("SystemDriver not available on this platform")
                })?;
                let result = driver
                    .launch_or_activate(
                        &self.deps.app_handle,
                        id,
                        &spec.application_path,
                        spec.bundle_identifier.as_deref(),
                    )
                    .await?;

                {
                    let conn = self.conn()?;
                    if result.confirmed {
                        crate::creative_app::runtime_store::upsert_system_instance(
                            &conn,
                            id,
                            result.identity.pid.map(|p| p as i32),
                            Some(&result.identity.ownership),
                            result.identity.bundle_id.as_deref(),
                        )?;
                    }
                }

                self.emit(
                    if result.confirmed {
                        "started"
                    } else {
                        "starting"
                    },
                    id,
                    AppKind::SystemApplication,
                );
                Ok(true)
            }
            AppKind::WebApplication => {
                let spec = {
                    let conn = self.conn()?;
                    AppRepository::load_web_spec(&conn, id)?
                };
                {
                    let conn = self.conn()?;
                    web::open(
                        &self.deps.app_handle,
                        &self.deps.browser,
                        &conn,
                        id,
                        &spec.url,
                        &spec.approved_origins,
                        bounds,
                    )?;
                }
                self.emit("web_opened", id, AppKind::WebApplication);
                Ok(true)
            }
        }
    }

    pub async fn web_set_bounds(
        &self,
        id: &str,
        bounds: crate::creative_app::model_runtime::BrowserBounds,
    ) -> Result<crate::creative_app::model_runtime::BrowserBounds> {
        let window_size = self
            .deps
            .app_handle
            .get_webview_window("main")
            .and_then(|w| {
                let size = w.inner_size().ok()?;
                let scale = w.scale_factor().ok()?;
                Some((size.width as f64 / scale, size.height as f64 / scale))
            });
        web::set_bounds(&self.deps.app_handle, id, bounds, window_size)
    }

    /// 启动应用（仅限 Local 与 System；Web 返回 typed unsupported）。
    pub async fn start(&self, id: &str) -> Result<bool> {
        let _guard = self.deps.locks.acquire_app(id).await;

        let (kind, source_id) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            (kind_opt.unwrap_or(AppKind::LocalProject), app.source_id)
        };

        match kind {
            AppKind::LocalProject => {
                self.emit("starting", id, AppKind::LocalProject);

                let pool = self.deps.db.clone();
                let app_handle = self.deps.app_handle.clone();
                let local_runtime = self.deps.local_runtime.clone();
                let host_port = self.deps.host_http_port;
                let s_id = source_id.clone();

                let spawned = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
                    rt.block_on(local::begin_start(
                        &c,
                        &app_handle,
                        &local_runtime,
                        host_port,
                        &s_id,
                    ))
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))??;

                let pool = self.deps.db.clone();
                let app_handle = self.deps.app_handle.clone();
                let local_runtime = self.deps.local_runtime.clone();
                let host_port = self.deps.host_http_port;
                let s_id = source_id.clone();

                let res = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
                    rt.block_on(local::await_start_ready(
                        &c,
                        &app_handle,
                        &local_runtime,
                        host_port,
                        &s_id,
                        &spawned,
                    ))
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;

                match res {
                    Ok(summary) => {
                        self.emit("started", id, AppKind::LocalProject);
                        if let Some(ref u) = summary.open_url {
                            let _ = open::that(u);
                        }
                        Ok(true)
                    }
                    Err(e) => {
                        self.emit("start_failed", id, AppKind::LocalProject);
                        Err(e)
                    }
                }
            }
            AppKind::SystemApplication => {
                let spec = {
                    let conn = self.conn()?;
                    AppRepository::load_system_spec(&conn, id)?
                };
                let driver = system::driver().ok_or_else(|| {
                    system::unsupported("SystemDriver not available on this platform")
                })?;
                let result = driver
                    .launch_or_activate(
                        &self.deps.app_handle,
                        id,
                        &spec.application_path,
                        spec.bundle_identifier.as_deref(),
                    )
                    .await?;

                {
                    let conn = self.conn()?;
                    if result.confirmed {
                        crate::creative_app::runtime_store::upsert_system_instance(
                            &conn,
                            id,
                            result.identity.pid.map(|p| p as i32),
                            Some(&result.identity.ownership),
                            result.identity.bundle_id.as_deref(),
                        )?;
                    }
                }

                self.emit(
                    if result.confirmed {
                        "started"
                    } else {
                        "starting"
                    },
                    id,
                    AppKind::SystemApplication,
                );
                Ok(true)
            }
            AppKind::WebApplication => Err(web::unsupported("start")),
        }
    }

    /// 停止应用（优雅终止）。
    pub async fn stop(&self, id: &str, _risk_level: u8) -> Result<bool> {
        let _guard = self.deps.locks.acquire_app(id).await;

        let (kind, source_id) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            (kind_opt.unwrap_or(AppKind::LocalProject), app.source_id)
        };

        match kind {
            AppKind::LocalProject => {
                self.emit("stopping", id, AppKind::LocalProject);

                let pool = self.deps.db.clone();
                let app_handle = self.deps.app_handle.clone();
                let local_runtime = self.deps.local_runtime.clone();
                let host_port = self.deps.host_http_port;
                let s_id = source_id.clone();

                let res = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
                    rt.block_on(local::stop(
                        &c,
                        &app_handle,
                        &local_runtime,
                        host_port,
                        &s_id,
                    ))
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;

                match res {
                    Ok(_) => {
                        self.emit("stopped", id, AppKind::LocalProject);
                        Ok(true)
                    }
                    Err(e) => {
                        self.emit("stop_failed", id, AppKind::LocalProject);
                        Err(e)
                    }
                }
            }
            AppKind::SystemApplication => {
                let spec = {
                    let conn = self.conn()?;
                    AppRepository::load_system_spec(&conn, id)?
                };
                let driver = system::driver().ok_or_else(|| {
                    system::unsupported("SystemDriver not available on this platform")
                })?;
                self.emit("stopping", id, AppKind::SystemApplication);
                match driver
                    .terminate(
                        &self.deps.app_handle,
                        id,
                        &spec.application_path,
                        spec.bundle_identifier.as_deref(),
                    )
                    .await
                {
                    Ok(()) => {
                        let conn = self.conn()?;
                        crate::creative_app::runtime_store::settle_system_instance_stopped(
                            &conn, id,
                        )?;
                        self.emit("stopped", id, AppKind::SystemApplication);
                        Ok(true)
                    }
                    Err(e) => {
                        self.emit("stop_failed", id, AppKind::SystemApplication);
                        Err(e)
                    }
                }
            }
            AppKind::WebApplication => Err(web::unsupported("stop")),
        }
    }

    pub async fn hide_system(&self, id: &str) -> Result<bool> {
        let _guard = self.deps.locks.acquire_app(id).await;
        self.system_hide_action(id, true).await
    }

    pub async fn unhide_system(&self, id: &str) -> Result<bool> {
        let _guard = self.deps.locks.acquire_app(id).await;
        self.system_hide_action(id, false).await
    }

    async fn system_hide_action(&self, id: &str, hide: bool) -> Result<bool> {
        let (kind, spec) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            let kind = kind_opt.unwrap_or(AppKind::LocalProject);
            if kind != AppKind::SystemApplication {
                return Err(Error::InvalidInput(format!(
                    "hide/unhide is only available for system applications (app {id} is {})",
                    app.source
                )));
            }
            (kind, AppRepository::load_system_spec(&conn, id)?)
        };

        let driver = system::driver()
            .ok_or_else(|| system::unsupported("SystemDriver not available on this platform"))?;
        let action = if hide { "hidden" } else { "unhidden" };
        let executed = if hide {
            driver
                .hide(&spec.application_path, spec.bundle_identifier.as_deref())
                .await?
        } else {
            driver
                .unhide(&spec.application_path, spec.bundle_identifier.as_deref())
                .await?
        };
        // 未真实执行（目标未运行）时不 emit 事件，避免 UI 误刷新。
        if executed {
            self.emit(action, id, kind);
        }
        Ok(executed)
    }

    /// 强制停止应用（capability 门控 + 严格 identity 匹配）。
    pub async fn force_stop(&self, id: &str) -> Result<bool> {
        let _guard = self.deps.locks.acquire_app(id).await;

        let (kind, source_id) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            (kind_opt.unwrap_or(AppKind::LocalProject), app.source_id)
        };

        match kind {
            AppKind::LocalProject => {
                let pool = self.deps.db.clone();
                let app_handle = self.deps.app_handle.clone();
                let local_runtime = self.deps.local_runtime.clone();
                let host_port = self.deps.host_http_port;
                let s_id = source_id.clone();

                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
                    rt.block_on(local::force_stop(
                        &c,
                        &app_handle,
                        &local_runtime,
                        host_port,
                        &s_id,
                    ))
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))??;

                self.emit("force_stopped", id, AppKind::LocalProject);
                Ok(true)
            }
            AppKind::SystemApplication => {
                if !system::force_stop_supported() {
                    return Err(system::unsupported(
                        "force terminate not supported on this platform",
                    ));
                }
                let spec = {
                    let conn = self.conn()?;
                    AppRepository::load_system_spec(&conn, id)?
                };
                let driver = system::driver().ok_or_else(|| {
                    system::unsupported("SystemDriver not available on this platform")
                })?;
                let identity = driver
                    .active_identity(&spec.application_path, spec.bundle_identifier.as_deref())
                    .await?
                    .ok_or_else(|| {
                        Error::InvalidInput("no live process identity found to force stop".into())
                    })?;
                driver
                    .force_terminate(&self.deps.app_handle, id, &identity)
                    .await?;

                {
                    let conn = self.conn()?;
                    crate::creative_app::runtime_store::settle_system_instance_stopped(&conn, id)?;
                }

                self.emit("force_stopped", id, AppKind::SystemApplication);
                Ok(true)
            }
            AppKind::WebApplication => Err(web::unsupported("force_stop")),
        }
    }

    /// 重启应用。
    pub async fn restart(&self, id: &str) -> Result<bool> {
        let _guard = self.deps.locks.acquire_app(id).await;

        let (kind, source_id) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            (kind_opt.unwrap_or(AppKind::LocalProject), app.source_id)
        };

        match kind {
            AppKind::LocalProject => {
                self.emit("starting", id, AppKind::LocalProject);

                let pool = self.deps.db.clone();
                let app_handle = self.deps.app_handle.clone();
                let local_runtime = self.deps.local_runtime.clone();
                let host_port = self.deps.host_http_port;
                let s_id = source_id.clone();

                let res = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
                    rt.block_on(local::restart(
                        &c,
                        &app_handle,
                        &local_runtime,
                        host_port,
                        &s_id,
                    ))
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;

                match res {
                    Ok(summary) => {
                        self.emit("restarted", id, AppKind::LocalProject);
                        if let Some(ref u) = summary.open_url {
                            let _ = open::that(u);
                        }
                        Ok(true)
                    }
                    Err(e) => {
                        self.emit("start_failed", id, AppKind::LocalProject);
                        Err(e)
                    }
                }
            }
            AppKind::SystemApplication => {
                let spec = {
                    let conn = self.conn()?;
                    AppRepository::load_system_spec(&conn, id)?
                };
                let driver = system::driver().ok_or_else(|| {
                    system::unsupported("SystemDriver not available on this platform")
                })?;
                // APPV2-T06：terminate 验证后 launch（有上限 observe 重试确认）。
                driver
                    .terminate(
                        &self.deps.app_handle,
                        id,
                        &spec.application_path,
                        spec.bundle_identifier.as_deref(),
                    )
                    .await?;
                let result = driver
                    .launch_or_activate(
                        &self.deps.app_handle,
                        id,
                        &spec.application_path,
                        spec.bundle_identifier.as_deref(),
                    )
                    .await?;

                {
                    let conn = self.conn()?;
                    if result.confirmed {
                        crate::creative_app::runtime_store::upsert_system_instance(
                            &conn,
                            id,
                            result.identity.pid.map(|p| p as i32),
                            Some(&result.identity.ownership),
                            result.identity.bundle_id.as_deref(),
                        )?;
                    }
                }

                self.emit(
                    if result.confirmed {
                        "restarted"
                    } else {
                        "starting"
                    },
                    id,
                    AppKind::SystemApplication,
                );
                Ok(true)
            }
            AppKind::WebApplication => Err(web::unsupported("restart")),
        }
    }

    /// 从应用中心移除注册（只删注册元数据，绝不删除用户项目目录、不卸载 .app、不清 Web profile 数据）。
    pub async fn remove(&self, id: &str, _risk_level: u8) -> Result<()> {
        let _guard = self.deps.locks.acquire_app(id).await;

        let (kind, source_id) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let (kind_opt, _, _, _) = AppRepository::identity(&conn, id)?;
            (kind_opt.unwrap_or(AppKind::LocalProject), app.source_id)
        };

        match kind {
            AppKind::LocalProject => {
                let pool = self.deps.db.clone();
                let app_handle = self.deps.app_handle.clone();
                let local_runtime = self.deps.local_runtime.clone();
                let host_port = self.deps.host_http_port;
                let s_id = source_id.clone();

                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
                    rt.block_on(local::delete(
                        &c,
                        &app_handle,
                        &local_runtime,
                        host_port,
                        &s_id,
                    ))
                })
                .await
                .map_err(|e| Error::Internal(e.to_string()))??;
            }
            AppKind::SystemApplication => {
                let spec_opt = {
                    let conn = self.conn()?;
                    AppRepository::load_system_spec(&conn, id).ok()
                };
                if let Some(spec) = spec_opt {
                    if let Some(driver) = system::driver() {
                        // best-effort：remove 只清注册元数据，terminate 失败不阻塞删除。
                        let _ = driver
                            .terminate(
                                &self.deps.app_handle,
                                id,
                                &spec.application_path,
                                spec.bundle_identifier.as_deref(),
                            )
                            .await;
                    }
                }
                let conn = self.conn()?;
                AppRepository::remove_application(&conn, id)?;
            }
            AppKind::WebApplication => {
                let _ = web::close(&self.deps.app_handle, &self.deps.browser, id);
                let conn = self.conn()?;
                AppRepository::remove_application(&conn, id)?;
            }
        }

        self.emit("removed", id, kind);
        Ok(())
    }

    // ── 专用领域操作 ──────────────────────────────────────────────────────

    /// 获取 Local 应用运行日志。
    pub fn local_logs(&self, id: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
        let active = AppRepository::active_instance(&conn, id)?;
        Ok(local::logs(
            &self.deps.local_runtime,
            &app.source_id,
            active.as_ref().map(|i| i.id.as_str()),
            limit,
        ))
    }

    /// 解决 Local 应用孤儿进程。
    pub async fn local_resolve_orphan(&self, id: &str, action: &str) -> Result<()> {
        let (source_id, _active_id) = {
            let conn = self.conn()?;
            let app = AppRepository::get(&conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
            let active = AppRepository::active_instance(&conn, id)?.ok_or_else(|| {
                Error::NotFound("no active runtime instance to resolve orphan".into())
            })?;
            (app.source_id, active.id)
        };

        let restart = action == "restart";
        let pool = self.deps.db.clone();
        let app_handle = self.deps.app_handle.clone();
        let local_runtime = self.deps.local_runtime.clone();
        let host_port = self.deps.host_http_port;

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            let c = pool.get().map_err(|e| Error::Internal(e.to_string()))?;
            rt.block_on(local::resolve_orphan(
                &c,
                &app_handle,
                &local_runtime,
                host_port,
                &source_id,
                restart,
            ))
        })
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;

        Ok(())
    }

    /// 清理 Web 应用数据（cookies / storage 隔离轮换，独立 Risk Level 2 操作）。
    pub fn web_clear_data(&self, id: &str) -> Result<()> {
        let mut conn = self.conn()?;
        web::clear_data(&mut conn, id)?;
        self.emit("web_data_cleared", id, AppKind::WebApplication);
        Ok(())
    }
}
