//! SystemApplicationDriver contract（APP-030 占位 —— Phase E 落实现）。
//!
//! 本轮（Phase C/D）只定义 trait + 平台 capability 查询 + typed unsupported
//! 返回：非 macOS 构建 / 未实现平台一律返回 `Error::NotImplemented`，**不 panic**
//! （APP-030 验收）。macOS NSWorkspace 实现（discover/launch_or_activate/
//! force_terminate）是 Phase E 的 B 任务。

#[cfg(target_os = "macos")]
pub mod macos;

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
    /// `.app` 绝对路径。
    pub path: String,
    /// 当前 PID（宿主启动的受管进程；None = 外部 preexisting）。
    pub pid: Option<u32>,
    /// 所有权：`managed`（本次 Host 启动）/ `preexisting`（外部已存在实例）。
    pub ownership: String,
}

/// 系统应用发现候选（Phase E 由 NSWorkspace / /Applications 枚举填充）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAppCandidate {
    pub bundle_id: Option<String>,
    pub path: String,
    pub display_name: String,
    pub icon_path: Option<String>,
}

/// 系统应用驱动契约（冻结签名 —— Phase E 实现体必须满足）。
#[async_trait]
pub trait SystemDriver: Send + Sync {
    /// 平台能力：能否强制终止目标 PID（NSWorkspace 强杀能力位）。
    /// false 时 `force_terminate` 必须返回 typed unsupported（06 契约）。
    fn force_terminate_supported(&self) -> bool;

    /// 发现已安装应用（Phase E：/Applications + NSWorkspace，spawn_blocking）。
    fn discover(&self, conn: &Connection) -> Result<Vec<SystemAppCandidate>>;

    /// 打开或激活（已运行 → 前台激活；未运行 → 启动）。
    async fn launch_or_activate(
        &self,
        app: &AppHandle,
        application_id: &str,
        path: &str,
    ) -> Result<SystemRunningIdentity>;

    /// 终止（graceful 优先）。
    async fn terminate(&self, app: &AppHandle, application_id: &str, path: &str) -> Result<()>;

    /// 强杀（capability-gated：不支持的平台/宿主外实例返回 typed unsupported）。
    async fn force_terminate(
        &self,
        app: &AppHandle,
        application_id: &str,
        identity: &SystemRunningIdentity,
    ) -> Result<()>;

    /// 观察当前运行身份（reconcile / capability 位输入）。
    async fn observe(&self, path: &str) -> Result<Option<SystemRunningIdentity>>;
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
