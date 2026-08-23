//! SystemApplicationDriver contract（APP-030 / APPV2-T06 补全）。
//!
//! 非 macOS 构建 / 未实现平台一律返回 `Error::NotImplemented`，**不 panic**
//! （APP-030 验收）。macOS NSWorkspace 实现（discover/launch_or_activate/
//! observe/hide/unhide/terminate/force_terminate）在 `macos.rs`。
//!
//! APPV2-T06 契约要点：
//! - [`observe`](SystemDriver::observe) 返回 typed [`SystemRunningState`]
//!   （installed / running / pid / hidden / active / unobservable），不再用
//!   `Option` 把「未安装 / 未运行 / 运行中」压成单值；
//! - [`launch_or_activate`](SystemDriver::launch_or_activate) 区分「已尝试启动」
//!   （`launched`）与「已确认运行」（`confirmed`，有上限的真实 observe 重试，
//!   不做固定 300ms 假就绪）；
//! - [`terminate`](SystemDriver::terminate) 终止后必须验证进程消失（~5s 上限），
//!   超时返回 typed 错误，不再无条件 `Ok`；
//! - [`hide`](SystemDriver::hide) / [`unhide`](SystemDriver::unhide) 最小能力，
//!   返回是否真实执行（目标未运行 = `Ok(false)`）。

#[cfg(target_os = "macos")]
pub mod macos;

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::Connection;
use tauri::AppHandle;

use crate::creative_app::model_runtime::BrowserBounds;
use crate::{Error, Result};

/// 系统应用运行身份（APP-030 step 2）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemRunningIdentity {
    /// macOS bundle id（其他平台为空）。
    pub bundle_id: Option<String>,
    /// `.app` 绝对路径（运行实例的当前 bundle 路径；App 移动/升级后以实际为准）。
    pub path: String,
    /// 当前 PID（宿主启动的受管进程；None = 外部 preexisting / 未运行）。
    pub pid: Option<u32>,
    /// 所有权：`managed`（本次 Host 启动）/ `preexisting`（外部已存在实例）。
    pub ownership: String,
}

/// 系统应用 typed 运行状态（APPV2-T06）。
///
/// `stopped / launching / running / hidden / active / unobservable` 可被区分：
/// - `installed=false` → not_installed（路径失效，UI 可恢复重新选择）；
/// - `installed=true, running=false` → 已安装未运行（stopped）；
/// - `running=true` → 运行中，`hidden` / `active` 为真实窗口状态
///   （`unobservable=true` 时二者为默认值，不可作为事实）；
/// - launch 后 `confirmed=false` → launching（启动已尝试未确认，下次 observe
///   收敛）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemRunningState {
    /// `.app` 是否仍存在于注册路径（路径失效 = false，可恢复重新选择）。
    pub installed: bool,
    /// 是否检测到运行实例（bundle ID 优先 + bundle 路径回退）。
    pub running: bool,
    /// 运行实例 PID（未运行 = None）。
    pub pid: Option<u32>,
    /// 目标应用是否隐藏（`unobservable=true` 时为默认值 false）。
    pub hidden: bool,
    /// 目标应用是否在前台激活（`unobservable=true` 时为默认值 false）。
    pub active: bool,
    /// 运行实例的 `.app` 实际路径（App 移动/升级后与注册路径可能不同；
    /// 未运行 = None）。
    pub bundle_path: Option<String>,
    /// 运行实例的 bundle id（未运行 = None）。
    pub bundle_id: Option<String>,
    /// hidden/active 等窗口属性是否不可观测（读不到时置 true，不算失败）。
    pub unobservable: bool,
}

/// 打开/启动结果（APPV2-T06：「已尝试启动」与「已确认运行」分离）。
///
/// - `launched=true, confirmed=true` → 本次真正发起启动且已确认运行；
/// - `launched=false, confirmed=true` → 命中已有实例，已前台激活；
/// - `launched=true, confirmed=false` → 启动已发出但未在时限内确认
///   （UI 显示 starting，下次 observe 收敛）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLaunchResult {
    pub identity: SystemRunningIdentity,
    /// 是否由本次调用真正发起启动（false = 仅激活已有实例）。
    pub launched: bool,
    /// 是否在时限内确认进程运行（false ≠ 失败，见 launching 语义）。
    pub confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDockRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockStatus {
    Docked,
    PermissionRequired,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockCapability {
    Available,
    PermissionRequired,
    NoStandardWindow,
    NotMovable,
    NotResizable,
    FullscreenUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockResult {
    pub status: DockStatus,
    pub capability: DockCapability,
    pub message: Option<String>,
}

/// 系统应用发现候选（Phase E 由 /Applications 枚举填充）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAppCandidate {
    pub bundle_id: Option<String>,
    pub path: String,
    pub display_name: String,
    pub icon_path: Option<String>,
}

/// 系统应用驱动契约（APPV2-T06 冻结签名）。
#[async_trait]
pub trait SystemDriver: Send + Sync {
    /// 平台能力：能否强制终止目标 PID（NSWorkspace 强杀能力位）。
    /// false 时 `force_terminate` 必须返回 typed unsupported（06 契约）。
    fn force_terminate_supported(&self) -> bool;

    /// 发现已安装应用（/Applications 枚举，spawn_blocking 由调用方负责）。
    fn discover(&self, conn: &Connection) -> Result<Vec<SystemAppCandidate>>;

    /// 打开或激活（已运行 → 前台激活；未运行 → 启动后做有上限的 observe
    /// 重试确认运行，超时不失败，返回 `confirmed=false`）。
    /// `bundle_id`：注册侧已知的 bundle id（None 时驱动从 Info.plist 读取）。
    async fn launch_or_activate(
        &self,
        app: &AppHandle,
        application_id: &str,
        path: &str,
        bundle_id: Option<&str>,
    ) -> Result<SystemLaunchResult>;

    /// 终止（graceful NSWorkspace.terminate；终止后验证进程消失，~5s 上限；
    /// 超时返回 typed Err，不再无条件 Ok）。
    async fn terminate(
        &self,
        app: &AppHandle,
        application_id: &str,
        path: &str,
        bundle_id: Option<&str>,
    ) -> Result<()>;

    /// 强杀（capability-gated：不支持的平台/宿主外实例返回 typed unsupported）。
    /// 底层能力保留（不扩展 kill -9 之外的行为）；新 UI 不暴露。
    async fn force_terminate(
        &self,
        app: &AppHandle,
        application_id: &str,
        identity: &SystemRunningIdentity,
    ) -> Result<()>;

    /// 观察当前 typed 运行状态（reconcile / capability 位输入）。
    /// 未安装 / 未运行 / 运行中 / hidden / active 均如实填充，不压成 Option。
    async fn observe(&self, path: &str, bundle_id: Option<&str>) -> Result<SystemRunningState>;

    /// 当前运行实例的身份（`observe` 的便捷投影；未运行 = None）。
    async fn active_identity(
        &self,
        path: &str,
        bundle_id: Option<&str>,
    ) -> Result<Option<SystemRunningIdentity>> {
        let state = self.observe(path, bundle_id).await?;
        Ok(if state.running {
            Some(SystemRunningIdentity {
                bundle_id: state.bundle_id,
                path: state
                    .bundle_path
                    .clone()
                    .unwrap_or_else(|| path.to_string()),
                pid: state.pid,
                ownership: "preexisting".into(),
            })
        } else {
            None
        })
    }

    /// 隐藏目标应用。返回是否真实执行（目标未运行 / 调用失败 = false，
    /// 不算错误 —— hide 对未运行目标无副作用）。
    async fn hide(&self, path: &str, bundle_id: Option<&str>) -> Result<bool>;

    /// 恢复（取消隐藏）目标应用。返回是否真实执行（同上）。
    async fn unhide(&self, path: &str, bundle_id: Option<&str>) -> Result<bool>;

    /// Move and resize the current standard main window once. No listener or
    /// continuous correction is installed.
    async fn dock(
        &self,
        path: &str,
        bundle_id: Option<&str>,
        rect: SystemDockRect,
    ) -> Result<DockResult>;
}

/// 当前构建是否提供系统应用驱动实现。
pub fn driver_available() -> bool {
    cfg!(target_os = "macos")
}

/// 平台 force-stop capability 位（system_capabilities 的输入）。
pub fn force_stop_supported() -> bool {
    driver_available()
}

/// 取驱动。
pub fn driver() -> Option<std::sync::Arc<dyn SystemDriver>> {
    #[cfg(target_os = "macos")]
    {
        Some(std::sync::Arc::new(macos::MacosSystemDriver::new()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// 未实现路径的统一 typed 错误（06：不 panic）。
pub fn unsupported(msg: &str) -> Error {
    Error::NotImplemented(msg.to_string())
}

/// Web/system 共享的 surface 边界默认值（占位，Phase E/F 复用）。
#[allow(dead_code)]
pub const DEFAULT_SYSTEM_BOUNDS: BrowserBounds = BrowserBounds {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
};

// ── APPV2-T06 纯逻辑（平台无关，单测覆盖；真机 NSWorkspace 行为由 T11 手工矩阵）──

/// launch 确认的 observe 重试上限（20 次，替代固定 300ms 假就绪）。
pub const LAUNCH_CONFIRM_ATTEMPTS: u32 = 20;
/// launch 确认的轮询间隔（20 × 150ms ≈ 3s 上限）。
pub const LAUNCH_CONFIRM_POLL: Duration = Duration::from_millis(150);
/// terminate 后验证进程消失的轮询次数（20 × 250ms = 5s 上限）。
pub const TERMINATE_VERIFY_POLLS: u32 = 20;
/// terminate 验证的轮询间隔。
pub const TERMINATE_VERIFY_POLL: Duration = Duration::from_millis(250);
/// 轮询默认间隔（测试可注入更小值）。
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 一次观测到的运行进程（平台层填充；hidden/active 读不到时 unobservable=true）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProcessObs {
    pub pid: u32,
    pub bundle_id: Option<String>,
    pub bundle_path: Option<String>,
    pub hidden: bool,
    pub active: bool,
    pub unobservable: bool,
}

/// 从「注册路径 + 观测到的运行进程 + 安装判定」派生 typed 状态（纯函数）。
///
/// `live = None` 表示未匹配到运行实例；`installed` 由注册路径是否存在判定
/// （路径失效 = not_installed，UI 可恢复重新选择）。
pub fn state_from_observation(
    path: &str,
    live: Option<&LiveProcessObs>,
    installed: bool,
) -> SystemRunningState {
    match live {
        None => SystemRunningState {
            installed,
            running: false,
            pid: None,
            hidden: false,
            active: false,
            bundle_path: None,
            bundle_id: None,
            unobservable: false,
        },
        Some(p) => SystemRunningState {
            installed: true,
            running: true,
            pid: Some(p.pid),
            hidden: p.hidden,
            active: p.active,
            bundle_path: Some(p.bundle_path.clone().unwrap_or_else(|| path.to_string())),
            bundle_id: p.bundle_id.clone(),
            unobservable: p.unobservable,
        },
    }
}

pub fn validate_dock_rect(rect: SystemDockRect) -> Result<SystemDockRect> {
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width < 80.0
        || rect.height < 60.0
    {
        return Err(Error::InvalidInput("invalid system dock bounds".into()));
    }
    Ok(rect)
}

/// 读 `.app/Contents/Info.plist` 的 `CFBundleIdentifier`（bundle ID 优先匹配
/// 的注册侧输入；plist 缺失/不可读 = None，回退路径匹配）。
#[cfg(target_os = "macos")]
pub fn read_bundle_identifier(app_path: &str) -> Option<String> {
    let plist = Path::new(app_path).join("Contents/Info.plist");
    if !plist.is_file() {
        return None;
    }
    unsafe {
        use objc::runtime::{Class, Object};
        use objc::{msg_send, sel, sel_impl};

        let path_ns = macos::new_nsstring(&plist.to_string_lossy());
        let cls = Class::get("NSDictionary")?;
        let dict: *mut Object = msg_send![cls, dictionaryWithContentsOfFile: path_ns.as_ptr()];
        if dict.is_null() {
            return None;
        }
        let key = macos::new_nsstring("CFBundleIdentifier");
        let bid: *mut Object = msg_send![dict, objectForKey: key.as_ptr()];
        macos::nsstring_to_rust(bid)
    }
}

/// 有上限的 observe 重试，直到 `probe` 返回 true（launch 确认用）。
///
/// 共 `attempts` 次探针（首次立即，之后每次间隔 `delay`，默认
/// [`POLL_INTERVAL`]）；返回是否在时限内确认。**超时返回 `false` 而非错误**
/// —— 「启动已尝试」不等于失败，UI 侧显示 starting，下次 observe 收敛。
pub async fn wait_until_true<F, Fut>(mut probe: F, attempts: u32, delay: Option<Duration>) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let delay = delay.unwrap_or(POLL_INTERVAL);
    for i in 0..attempts {
        if i > 0 {
            tokio::time::sleep(delay).await;
        }
        if probe().await {
            return true;
        }
    }
    false
}

/// 有上限地验证条件成立（terminate 验证进程消失用；probe = 「已消失」）。
///
/// 共 `polls` 次探针（每次先间隔 `delay`，默认 [`POLL_INTERVAL`]）；返回
/// `true` = 上限内确认，`false` = 超时未确认（调用方映射为 typed 超时错误，
/// 不再无条件 Ok）。
pub async fn wait_until_true_before_timeout<F, Fut>(
    mut probe: F,
    polls: u32,
    delay: Option<Duration>,
) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let delay = delay.unwrap_or(POLL_INTERVAL);
    for _ in 0..polls {
        tokio::time::sleep(delay).await;
        if probe().await {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    //! APPV2-T06 trait 层逻辑单测（fake probe / 纯函数）。
    //! 依赖真机 NSWorkspace 的 observe/hide/terminate 行为不在此覆盖 ——
    //! 由 T11 macOS packaged smoke 手工矩阵验证。

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 前 `running_from` 次探针返回 false，之后返回 true（模拟进程延迟出现/消失）。
    fn counting_probe(
        running_from: u32,
    ) -> (
        impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>,
        Arc<AtomicUsize>,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let c2 = calls.clone();
        (
            move || {
                let n = c2.fetch_add(1, Ordering::SeqCst) as u32;
                let r = n >= running_from;
                Box::pin(async move { r })
            },
            calls,
        )
    }

    #[tokio::test]
    async fn launch_retry_confirms_when_process_appears() {
        let (probe, calls) = counting_probe(2); // 第 3 次探针才 running
        assert!(wait_until_true(probe, 10, Some(Duration::from_millis(1))).await);
        assert_eq!(calls.load(Ordering::SeqCst), 3, "stops as soon as running");
    }

    #[tokio::test]
    async fn launch_retry_times_out_as_unconfirmed_not_error() {
        let (probe, _) = counting_probe(u32::MAX); // 永远不 running
        assert!(
            !wait_until_true(probe, 5, Some(Duration::from_millis(1))).await,
            "timeout must surface as confirmed=false, not Err"
        );
    }

    #[tokio::test]
    async fn launch_retry_confirms_on_first_probe() {
        let (probe, calls) = counting_probe(0);
        assert!(wait_until_true(probe, 10, Some(Duration::from_millis(1))).await);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// probe = 「进程已消失」：前 `gone_from` 次返回 false（进程还在），之后 true。
    #[tokio::test]
    async fn terminate_verify_detects_gone_process() {
        let (probe, _) = counting_probe(1); // 第一次还在，第二次起 gone
        assert!(wait_until_true_before_timeout(probe, 10, Some(Duration::from_millis(1))).await);
    }

    /// probe = 「进程已消失」永远 false（进程一直 running）→ 超时返回 false。
    #[tokio::test]
    async fn terminate_verify_timeout_returns_false() {
        let (probe, _) = counting_probe(u32::MAX); // 永远不 gone
        assert!(
            !wait_until_true_before_timeout(probe, 3, Some(Duration::from_millis(1))).await,
            "caller maps this to a typed timeout error"
        );
    }

    #[test]
    fn state_projection_running_with_window_flags() {
        let s = state_from_observation(
            "/Applications/Fake.app",
            Some(&LiveProcessObs {
                pid: 7,
                bundle_id: Some("com.fake".into()),
                bundle_path: Some("/Moved/Fake.app".into()),
                hidden: true,
                active: false,
                unobservable: false,
            }),
            true,
        );
        assert!(s.running && s.installed);
        assert_eq!(s.pid, Some(7));
        assert!(s.hidden && !s.active);
        // App 移动后以运行实例实际路径为准
        assert_eq!(s.bundle_path.as_deref(), Some("/Moved/Fake.app"));
        assert!(!s.unobservable);
    }

    #[test]
    fn state_projection_installed_not_running() {
        let s = state_from_observation("/Applications/Fake.app", None, true);
        assert!(!s.running && s.installed);
        assert_eq!(s.pid, None);
        assert!(!s.unobservable, "stopped state has no observability gap");
    }

    #[test]
    fn state_projection_not_installed_is_typed() {
        let s = state_from_observation("/Applications/Gone.app", None, false);
        assert!(!s.running && !s.installed);
        assert!(s.bundle_path.is_none());
    }

    #[test]
    fn state_projection_unobservable_defaults_are_marked() {
        let s = state_from_observation(
            "/Applications/Fake.app",
            Some(&LiveProcessObs {
                pid: 9,
                bundle_id: None,
                bundle_path: None,
                hidden: false,
                active: false,
                unobservable: true,
            }),
            true,
        );
        assert!(s.running);
        assert!(
            s.unobservable,
            "read failure must be visible, not silent false"
        );
        assert_eq!(
            s.bundle_path.as_deref(),
            Some("/Applications/Fake.app"),
            "bundle path falls back to registered path"
        );
    }

    #[test]
    fn launch_result_separates_attempt_from_confirmation() {
        let confirmed = SystemLaunchResult {
            identity: SystemRunningIdentity {
                bundle_id: Some("com.fake".into()),
                path: "/Applications/Fake.app".into(),
                pid: Some(11),
                ownership: "managed".into(),
            },
            launched: true,
            confirmed: true,
        };
        let json = serde_json::to_string(&confirmed).unwrap();
        assert!(json.contains("\"launched\":true") && json.contains("\"confirmed\":true"));
        let starting = SystemLaunchResult {
            confirmed: false,
            ..confirmed
        };
        let json = serde_json::to_string(&starting).unwrap();
        assert!(
            json.contains("\"confirmed\":false"),
            "launching state must be expressible over IPC"
        );
    }

    #[test]
    fn dock_rect_rejects_non_finite_and_tiny_values() {
        let valid = SystemDockRect {
            x: 10.0,
            y: 20.0,
            width: 800.0,
            height: 600.0,
        };
        assert_eq!(validate_dock_rect(valid).unwrap(), valid);
        assert!(validate_dock_rect(SystemDockRect {
            width: 79.0,
            ..valid
        })
        .is_err());
        assert!(validate_dock_rect(SystemDockRect {
            x: f64::NAN,
            ..valid
        })
        .is_err());
    }
}
