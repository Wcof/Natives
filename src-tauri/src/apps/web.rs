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

/// T04 预算决策（纯函数，可单测）：`reuses_open`（open 的是已存在的
/// child WebView，不新增 renderer）→ 放行；否则 `live >= max` 时返回与旧
/// WindowController 路径（`creative_app::window` R-P9/T11）文案一致的 typed
/// 错误。`soft` 仅文档用途（触发 LRU 休眠的软阈值，实际在
/// [`enforce_live_budget`] 里使用，不参与硬上限判定）。
pub fn budget_decision(
    live: usize,
    reuses_open: bool,
    #[allow(unused)] soft: usize,
    max: usize,
) -> Result<()> {
    if reuses_open {
        return Ok(());
    }
    if live >= max {
        return Err(Error::Internal(format!(
            "window limit reached ({live}/{max} open): close a window before opening another"
        )));
    }
    Ok(())
}

/// T04：把既有 6/10 WebView 预算门禁接入 Apps 域 Web 打开路径（仅新增
/// WebView 时判定，与旧路径语义一致）。live 超过软阈值时先尝试 LRU 休眠
/// 后台候选（keep_alive=0 的 open 窗口）再复检。
///
/// 注意：候选行里存的 label 是 uuid 派生（`creative-window-{id}`）不可信
/// （Apps 域 web 窗口的真实 child label 由 app_id 派生），这里统一用
/// `web_label(&app_id)` 重推真实 child label 后再 exists/close 判定。
fn enforce_live_budget(
    app: &AppHandle,
    browser_state: &BrowserStateHandle,
    conn: &Connection,
    self_label: &str,
) -> Result<()> {
    let mut live = surface_store::count_open_windows(conn)?;
    if live >= crate::creative_app::window::SOFT_LIVE_WINDOWS {
        let candidates = surface_store::list_lru_hibernation_candidates(conn)?;
        for (cand_id, cand_app_id, _stored) in candidates {
            let cand_label = web_label(&cand_app_id);
            if cand_label == self_label {
                continue;
            }
            if browser::browser_exists(app, &cand_label) {
                let _ = browser::browser_close(app, browser_state, &cand_label);
            }
            let _ = surface_store::mark_window_hibernated(conn, &cand_id);
            live = surface_store::count_open_windows(conn)?;
            if live < crate::creative_app::window::SOFT_LIVE_WINDOWS {
                break;
            }
        }
    }
    budget_decision(
        live,
        false,
        crate::creative_app::window::SOFT_LIVE_WINDOWS,
        crate::creative_app::window::MAX_LIVE_WINDOWS,
    )
}

/// open：在 trust domain 内打开 child WebView。
///
/// `approved_origins` 非空 = remote（导航限制在 approved origins）；空 = 按
/// attached 语义（loopback-only）。落 `application_surfaces` +
/// `window_instances` 行（呈现层投影；web 无 runtime_instance_id），返回
/// WebView label。
///
/// APPV2-T03：`bounds` = Renderer 实测内容区矩形（已 Host 验证）；Some 时
/// 优先于 `DEFAULT_WEB_BOUNDS` —— 新建与复用（已存在 label）两条路径都生效，
/// 保证「先 bounds 后 show」，消除固定 160,120,960,720 的坐标闪现。
pub fn open(
    app: &AppHandle,
    browser_state: &BrowserStateHandle,
    conn: &Connection,
    application_id: &str,
    url: &str,
    approved_origins: &[String],
    bounds: Option<crate::creative_app::model_runtime::BrowserBounds>,
) -> Result<String> {
    require_web_kind(conn, application_id)?;
    let label = web_label(application_id);
    // T04：预算门禁只在会新增 live WebView 时判定（既有 label 复用不占新
    // renderer，与旧路径「reuses open」豁免一致）。
    let reuses_open = browser::browser_exists(app, &label);
    if !reuses_open {
        enforce_live_budget(app, browser_state, conn, &label)?;
    }
    browser::browser_show_non_owned(
        app,
        browser_state,
        &label,
        application_id,
        url,
        bounds.unwrap_or(DEFAULT_WEB_BOUNDS),
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
    // T04：窗口行复用既有 surface 行（不每次 open 堆新行），并把它推进
    // open 态 + 真实 child label（stored label 是 uuid 派生投影，不可信）。
    // DB 失败保持既有吞错风格（WebView 已是权威呈现状态）。
    let window_id =
        match surface_store::find_latest_surface_window(conn, application_id, &surface_id) {
            Ok(Some(w)) => w.id,
            _ => surface_store::create_window(conn, application_id, &surface_id, None)?,
        };
    let _ = surface_store::set_window_live(conn, &window_id, &label);
    Ok(label)
}

/// close：关闭该 app 的 child WebView（WebView 已不存在时幂等成功）。
///
/// T04：WebView 销毁后把该 app 的窗口行落 closed（释放 live 预算槽）。行定位与
/// open 路径镜像：main surface → 最近 window 行（state-agnostic，正是
/// `set_window_live` 写下的那行）；找不到 main surface / 无窗口行 / DB 失败时
/// 沿用既有吞错风格（WebView 是权威呈现状态，不阻断关闭）。
pub fn close(
    app: &AppHandle,
    browser_state: &BrowserStateHandle,
    application_id: &str,
) -> Result<()> {
    let label = web_label(application_id);
    browser::browser_close(app, browser_state, &label)?;
    let _ = crate::db::get_main_conn().and_then(|conn| {
        let surface_id = surface_store::find_main_surface(&conn, application_id)?
            .ok_or_else(|| Error::NotFound(application_id.into()))?;
        let window_id =
            surface_store::find_latest_surface_window(&conn, application_id, &surface_id)?
                .map(|w| w.id)
                .ok_or_else(|| Error::NotFound(application_id.into()))?;
        surface_store::set_window_closed(&conn, &window_id)
    });
    Ok(())
}

/// hide：隐藏（不销毁）child WebView。
pub fn hide(app: &AppHandle, application_id: &str) -> Result<()> {
    browser::browser_hide(app, &web_label(application_id))
}

/// APPV2-T03：Web Surface 动态内容区 bounds。
///
/// `window_size` = 主窗口逻辑尺寸（Host 验证用的窗口边界）；返回 clamp 后的
/// 实际生效 bounds。`window_size` 不可用时退化为仅 finite/正宽高/最小尺寸校验
/// （bounds 仍可能暂时超出窗口，下一次 set 会收敛——不阻断呈现）。
pub fn set_bounds(
    app: &AppHandle,
    application_id: &str,
    bounds: crate::creative_app::model_runtime::BrowserBounds,
    window_size: Option<(f64, f64)>,
) -> Result<crate::creative_app::model_runtime::BrowserBounds> {
    let validated = validate_bounds(bounds, window_size)?;
    browser::browser_set_bounds(app, &web_label(application_id), validated.clone())?;
    Ok(validated)
}

/// bounds 策略校验（纯函数，可单测）：
/// - 全部 finite；
/// - 宽高为正且不小于 80×60（再小没有呈现意义）；
/// - 有窗口尺寸时 clamp 进主窗口逻辑范围（x/y ≥ 0，右下不超出窗口，
///   保留最小尺寸优先——窗口本身小于最小尺寸时按最小尺寸放行并截断原点）。
pub fn validate_bounds(
    b: crate::creative_app::model_runtime::BrowserBounds,
    window_size: Option<(f64, f64)>,
) -> Result<crate::creative_app::model_runtime::BrowserBounds> {
    use crate::creative_app::model_runtime::BrowserBounds;
    for (name, v) in [
        ("x", b.x),
        ("y", b.y),
        ("width", b.width),
        ("height", b.height),
    ] {
        if !v.is_finite() {
            return Err(Error::InvalidInput(format!(
                "web bounds {name} not finite: {v}"
            )));
        }
    }
    const MIN_W: f64 = 80.0;
    const MIN_H: f64 = 60.0;
    if b.width < MIN_W || b.height < MIN_H {
        return Err(Error::InvalidInput(format!(
            "web bounds too small: {}x{} (min {MIN_W}x{MIN_H})",
            b.width, b.height
        )));
    }
    let Some((ww, wh)) = window_size else {
        return Ok(BrowserBounds {
            x: b.x.max(0.0),
            y: b.y.max(0.0),
            width: b.width,
            height: b.height,
        });
    };
    // clamp 回窗口内，但保证最小尺寸（窗口本身小于最小尺寸时允许溢出窗口）。
    let width = b.width.min(ww).max(MIN_W);
    let height = b.height.min(wh).max(MIN_H);
    let x = b.x.clamp(0.0, ww.max(width) - width);
    let y = b.y.clamp(0.0, wh.max(height) - height);
    Ok(BrowserBounds {
        x,
        y,
        width,
        height,
    })
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
    fn budget_decision_is_typed() {
        assert!(budget_decision(3, false, 6, 10).is_ok());
        assert!(budget_decision(9, false, 6, 10).is_ok());
        assert!(budget_decision(10, false, 6, 10).is_err());
        assert!(budget_decision(12, true, 6, 10).is_ok());
    }

    #[test]
    fn unsupported_is_typed_message() {
        let e = unsupported("stop");
        assert!(e.to_string().contains("do not support 'stop'"));
    }

    /// APPV2-T03：bounds Host 端验证策略（finite / 最小尺寸 / 窗口范围 clamp）。
    #[test]
    fn validate_bounds_policy() {
        use crate::creative_app::model_runtime::BrowserBounds;
        fn b(x: f64, y: f64, w: f64, h: f64) -> BrowserBounds {
            BrowserBounds {
                x,
                y,
                width: w,
                height: h,
            }
        }

        // 合法 bounds + 窗口尺寸：原样通过（在窗口内）。
        let ok = validate_bounds(b(100.0, 120.0, 960.0, 720.0), Some((1440.0, 900.0))).unwrap();
        assert_eq!(
            (ok.x, ok.y, ok.width, ok.height),
            (100.0, 120.0, 960.0, 720.0)
        );

        // 超出窗口右下：clamp 回窗口内。
        let clamped =
            validate_bounds(b(1400.0, 850.0, 400.0, 300.0), Some((1440.0, 900.0))).unwrap();
        assert_eq!((clamped.x, clamped.y), (1040.0, 600.0));

        // 负原点：截断到 0。
        let neg = validate_bounds(b(-50.0, -20.0, 300.0, 200.0), Some((1440.0, 900.0))).unwrap();
        assert_eq!((neg.x, neg.y), (0.0, 0.0));

        // 非 finite / 过小 / 零宽高：拒绝。
        assert!(validate_bounds(b(f64::NAN, 0.0, 300.0, 200.0), None).is_err());
        assert!(validate_bounds(b(0.0, 0.0, f64::INFINITY, 200.0), None).is_err());
        assert!(validate_bounds(b(0.0, 0.0, 79.0, 200.0), None).is_err());
        assert!(validate_bounds(b(0.0, 0.0, 300.0, 0.0), None).is_err());

        // 无窗口尺寸：只保证 x/y ≥ 0，不 clamp 宽高。
        let no_win = validate_bounds(b(-10.0, 5.0, 2000.0, 1500.0), None).unwrap();
        assert_eq!(
            (no_win.x, no_win.y, no_win.width, no_win.height),
            (0.0, 5.0, 2000.0, 1500.0)
        );

        // 窗口本身小于最小尺寸：按最小尺寸放行（原点截断到 0）。
        let tiny = validate_bounds(b(0.0, 0.0, 80.0, 60.0), Some((40.0, 30.0))).unwrap();
        assert_eq!(
            (tiny.x, tiny.y, tiny.width, tiny.height),
            (0.0, 0.0, 80.0, 60.0)
        );
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

    /// T04：close 落 closed 释放预算槽（store 路径，不依赖真实 WebView）。
    #[test]
    fn close_marks_window_closed() {
        let conn = v28();
        let app_id = crate::apps::repository::AppRepository::register_web(
            &conn,
            "W",
            "https://w.example.com",
            &[],
            None,
            false,
        )
        .unwrap();
        let surface_id =
            surface_store::create_surface(&conn, &app_id, "main", "Browser", None).unwrap();
        let window_id = surface_store::create_window(&conn, &app_id, &surface_id, None).unwrap();
        surface_store::set_window_live(&conn, &window_id, &web_label(&app_id)).unwrap();
        let open_after_live = surface_store::count_open_windows(&conn).unwrap();
        assert_eq!(open_after_live, 1, "set_window_live 后 open 计数应包含该行");
        surface_store::set_window_closed(&conn, &window_id).unwrap();
        assert_eq!(
            surface_store::count_open_windows(&conn).unwrap(),
            open_after_live - 1,
            "close 落 closed 后 open 计数必须下降（预算槽释放）"
        );
        // 幂等：重复 close（WebView 已不存在）仍是 closed，计数不再变化。
        surface_store::set_window_closed(&conn, &window_id).unwrap();
        assert_eq!(
            surface_store::count_open_windows(&conn).unwrap(),
            open_after_live - 1
        );
    }
}
