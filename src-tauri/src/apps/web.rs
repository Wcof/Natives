//! WebSurfaceDriver（Phase C skeleton —— Phase F 的 B 任务落全量 browser 代码）。
//!
//! 本轮只提供 contract + 复用接入：
//! - **open**：复用 `browser::browser_show_non_owned`（trust-domain 受限 child
//!   WebView，approved_origins 来自 `web_application_specs`）+ `window_instances`
//!   行投影；
//! - **close/hide/reload/back/forward**：复用 `browser::browser_*` 成熟 helper
//!   （label = `non_owned_label(app_id)`，与 non_owned 表面同一 WebView 命名空间）；
//! - **clear_data**：独立高风险操作（risk 2）—— 轮换绑定 profile 的 data store
//!   标识（该 profile 的 cookies/localStorage/IndexedDB 随之失效）。**不**删除
//!   profile 行；`apps_remove` 绝不清数据（02 架构 §删除）。
//!
//! Web **没有 RuntimeInstance**（02 架构 §Runtime）：只有 Surface / Window
//! session。`apps_stop` / `apps_restart` 对 web 恒返回 typed unsupported
//! （06 强制规则；`AppsService::stop`/`restart` 在 kind 分派层拦截）。

use crate::creative_app::browser::{self, BrowserStateHandle};
use crate::creative_app::model_runtime::WindowInstance;
use crate::creative_app::{profile_store, surface_store};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use tauri::AppHandle;

/// Web surface 默认边界（与 non_owned open 一致）。
pub const DEFAULT_WEB_BOUNDS: crate::creative_app::model_runtime::BrowserBounds =
    crate::creative_app::model_runtime::BrowserBounds {
        x: 160.0,
        y: 120.0,
        width: 960.0,
        height: 720.0,
    };

fn web_label(app_id: &str) -> String {
    browser::non_owned_label(app_id)
}

/// 校验 app 是 web_application（typed NotFound / InvalidInput）。
pub fn require_web_kind(conn: &Connection, application_id: &str) -> Result<()> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM applications WHERE id = ?1",
            [application_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(Error::Database)?
        .ok_or_else(|| Error::NotFound(application_id.into()))?;
    if kind.as_deref() != Some("web_application") {
        return Err(Error::InvalidInput(format!(
            "app {application_id} is not a web application"
        )));
    }
    Ok(())
}

/// open：在 trust domain 内打开 child WebView。
///
/// `approved_origins` 非空 = remote（导航限制在 approved origins）；空 = 按
/// attached 语义（loopback-only）。落 `application_surfaces` +
/// `window_instances` 行（呈现层投影；web 无 runtime_instance_id），返回
/// WebView label。
pub fn open(
    app: &AppHandle,
    browser_state: &BrowserStateHandle,
    conn: &Connection,
    application_id: &str,
    url: &str,
    approved_origins: &[String],
) -> Result<String> {
    require_web_kind(conn, application_id)?;
    let label = web_label(application_id);
    browser::browser_show_non_owned(
        app,
        browser_state,
        &label,
        application_id,
        url,
        DEFAULT_WEB_BOUNDS,
        if approved_origins.is_empty() {
            None
        } else {
            Some(approved_origins)
        },
    )?;
    // 呈现层投影（WebView 已真实 show，DB 失败不回滚 WebView —— 与 non_owned
    // 路径一致：下次 open 复用既有 label）。
    let surface_id =
        surface_store::create_surface(conn, application_id, "main", "Browser", Some(url))?;
    let _ = surface_store::create_window(conn, application_id, &surface_id, None)?;
    Ok(label)
}

/// close：关闭该 app 的 child WebView（WebView 已不存在时幂等成功）。
pub fn close(
    app: &AppHandle,
    browser_state: &BrowserStateHandle,
    application_id: &str,
) -> Result<()> {
    let label = web_label(application_id);
    browser::browser_close(app, browser_state, &label)
}

/// hide：隐藏（不销毁）child WebView。
pub fn hide(app: &AppHandle, application_id: &str) -> Result<()> {
    browser::browser_hide(app, &web_label(application_id))
}

/// reload：复用 browser 成熟 helper。
pub fn reload(app: &AppHandle, application_id: &str) -> Result<()> {
    browser::browser_reload(app, &web_label(application_id))
}

/// back：history back（复用 helper）。
pub fn back(app: &AppHandle, application_id: &str) -> Result<()> {
    browser::browser_back(app, &web_label(application_id))
}

/// forward：history forward（复用 helper）。
pub fn forward(app: &AppHandle, application_id: &str) -> Result<()> {
    browser::browser_forward(app, &web_label(application_id))
}

/// clear_data（独立高风险操作，risk 2，APP-013 动作矩阵）：轮换绑定 profile 的
/// WebKit data store identifier → 旧 store 的 cookies/localStorage/IndexedDB /
/// service workers 随之失效。
///
/// - 不删除 profile 行（binding 保持，新 identifier 重新生效）；
/// - 幂等（每次调用都是新鲜轮换）；
/// - 只影响绑定到该 profile 的 WebView 数据（remove 绝不清数据，02 架构 §删除）。
pub fn clear_data(conn: &mut Connection, application_id: &str) -> Result<()> {
    require_web_kind(conn, application_id)?;
    let profile = profile_store::profile_for_app(conn, application_id)?;
    let t = chrono::Utc::now().to_rfc3339();
    let new_key = profile_store::store_key_for_id(&format!(
        "{}:cleared-{}",
        profile.id,
        uuid::Uuid::new_v4()
    ));
    conn.execute(
        "UPDATE browser_profiles SET platform_store_key = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![profile.id, new_key, t],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// 该 app 是否还有 open/minimized 的 window 行（capability 位输入）。
pub fn has_open_window(conn: &Connection, application_id: &str) -> Result<bool> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM window_instances
             WHERE application_id = ?1 AND state != 'closed'",
            [application_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    Ok(n > 0)
}

/// 是否存在 hibernated 标记（v28 window_instances.hibernated_at；LRU 休眠治理
/// 本身是 Phase F 的 B 任务，这里只读投影）。
pub fn any_hibernated(conn: &Connection, application_id: &str) -> Result<bool> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM window_instances
             WHERE application_id = ?1 AND hibernated_at IS NOT NULL",
            [application_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    Ok(n > 0)
}

/// web 不支持的 runtime 操作统一 typed 错误（06 强制规则：web stop/restart 必须
/// typed unsupported，不 panic）。
pub fn unsupported(op: &str) -> Error {
    Error::InvalidInput(format!(
        "web applications do not support '{op}' (no runtime instance; use open/close surface actions)"
    ))
}

/// 窗口行投影（apps_list_surfaces 之外的 window 视角，Phase F 扩 surface 状态机）。
pub fn list_windows(conn: &Connection, application_id: &str) -> Result<Vec<WindowInstance>> {
    surface_store::list_windows(conn, application_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v28() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn web_kind_gate_rejects_non_web() {
        let conn = v28();
        let app_id = crate::apps::repository::AppRepository::register_local(
            &conn,
            "L",
            "/tmp",
            None,
            None,
            crate::apps::model::RegistrationOrigin::Manual,
        )
        .unwrap();
        assert!(matches!(
            require_web_kind(&conn, &app_id),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            require_web_kind(&conn, "app-nope"),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn unsupported_is_typed_message() {
        let e = unsupported("stop");
        assert!(e.to_string().contains("do not support 'stop'"));
    }

    #[test]
    fn clear_data_rotates_profile_key() {
        let mut conn = v28();
        let w = crate::apps::repository::AppRepository::register_web(
            &conn,
            "W",
            "https://w.example.com",
            &["w.example.com".into()],
            None,
            false,
        )
        .unwrap();
        let before = profile_store::profile_for_app(&conn, &w)
            .unwrap()
            .platform_store_key;
        clear_data(&mut conn, &w).unwrap();
        let after = profile_store::profile_for_app(&conn, &w)
            .unwrap()
            .platform_store_key;
        assert_ne!(
            before, after,
            "clear data must rotate the data store identifier"
        );
        // 幂等：再清一次仍然成功。
        clear_data(&mut conn, &w).unwrap();
    }

    #[test]
    fn web_window_state_projections() {
        let conn = v28();
        let w = crate::apps::repository::AppRepository::register_web(
            &conn,
            "W",
            "https://w.example.com",
            &[],
            None,
            false,
        )
        .unwrap();
        assert!(!has_open_window(&conn, &w).unwrap());
        assert!(!any_hibernated(&conn, &w).unwrap());
        let surface = surface_store::create_surface(&conn, &w, "main", "Browser", None).unwrap();
        let window = surface_store::create_window(&conn, &w, &surface, None).unwrap();
        surface_store::update_window_state(&conn, &window, WindowInstance::STATE_OPEN).unwrap();
        assert!(has_open_window(&conn, &w).unwrap());
    }
}
