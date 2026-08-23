//! CapabilityResolver（APP-016 / 06 契约）。
//!
//! 按 AppKind + 运行态输出 `AppCapabilities`（动作矩阵 + 风险等级提示）。
//! 冻结动作矩阵（主 Agent 决策 #5）：
//!
//! - **local**：open/start/stop/restart/force_stop/edit/remove/set_sidebar；
//!   risk：stop/restart/remove = 1，force_stop（及含 force 的删除路径）= 2。
//! - **system**：open_or_activate/stop/restart/edit/remove/set_sidebar；
//!   force_stop 为 capability-gated（平台支持强杀才开启，risk 2）。
//!   注意：「risk 2 = preexisting 强杀」由 service 侧能力位
//!   （[`SystemDriver::supports_force_terminate`]）表达，`AppCapabilities`
//!   结构在 Phase B（model.rs 冻结）定形，这里 `can_stop/can_restart` 仅表示
//!   当前运行态下操作可行。
//! - **web**：open/edit/remove/set_sidebar + browser actions；
//!   `can_stop` / `can_restart` **恒 false**（`apps_stop`/`apps_restart` 对 web
//!   返回 typed unsupported，见 `AppsService::stop`）。
//!
//! riskLevel 语义：0 = 无副作用（直接）、1 = 轻确认（stop/restart/remove）、
//! 2 = 强确认（force/preexisting 强杀/clear web data/orphan 强清）。

use super::model::{AppCapabilities, AppRuntimeState};
use crate::creative_app::model_runtime::WindowInstance;

/// 风险等级（冻结矩阵）。
pub const RISK_NONE: u8 = 0;
pub const RISK_CONFIRM: u8 = 1;
pub const RISK_STRONG_CONFIRM: u8 = 2;

/// Local 项目应用的动作矩阵。
pub fn local_capabilities(runtime_state: AppRuntimeState) -> AppCapabilities {
    let active = matches!(
        runtime_state,
        AppRuntimeState::Running
            | AppRuntimeState::Starting
            | AppRuntimeState::Stopping
            | AppRuntimeState::Orphaned
    );
    let mut caps = AppCapabilities {
        // start 永远可用（是否真有活动实例由 service 的 one-active 守卫判定）。
        can_start: true,
        can_stop: active,
        can_restart: active,
        can_open: runtime_state == AppRuntimeState::Running,
        can_edit: true,
        can_remove: true,
        can_sidebar: true,
        risk_level: if active { RISK_CONFIRM } else { RISK_NONE },
    };
    // orphan 需要用户显式 resolve（强确认路径）。
    if runtime_state == AppRuntimeState::Orphaned {
        caps.risk_level = RISK_STRONG_CONFIRM;
    }
    caps
}

/// 系统应用的动作矩阵。`force_stop_supported` = 平台 capability（NSWorkspace
/// 能否强制终止目标 PID）；false 时 force_stop 不可用（typed unsupported）。
pub fn system_capabilities(running: bool, force_stop_supported: bool) -> AppCapabilities {
    let mut caps = AppCapabilities {
        can_start: false,
        can_stop: running,
        can_restart: running,
        can_open: true, // open = launch/activate（06 契约）
        can_edit: true,
        can_remove: true,
        can_sidebar: true,
        risk_level: RISK_NONE,
    };
    if running {
        // stop/restart 轻确认；remove 亦为 1。
        caps.risk_level = RISK_CONFIRM;
        // preexisting 系统进程（宿主之外的已存在实例）强杀 → 强确认。
        if force_stop_supported {
            caps.risk_level = RISK_STRONG_CONFIRM;
        }
    }
    caps
}

/// Web 应用的动作矩阵：`can_stop` / `can_restart` 恒 false（APP-016 验收）。
/// clear web data 是独立高风险操作（risk 2，由 service 侧能力位表达，不在
/// remove 动作里）。
pub fn web_capabilities(_hib: bool, _has_open_window: bool) -> AppCapabilities {
    AppCapabilities {
        can_start: false,
        can_stop: false,
        can_restart: false,
        can_open: true,
        can_edit: true,
        can_remove: true,
        can_sidebar: true,
        risk_level: RISK_NONE,
    }
}

/// 由 window_instances 派生 Web 运行态：任一窗口 open/minimized → Running；
/// 无开放窗口但存在 hibernated 标记 → Hibernated；否则 Stopped。
/// （Web 无 RuntimeInstance —— 02 架构 §Runtime。）
pub fn web_runtime_state_from_windows(
    windows: &[WindowInstance],
    any_hibernated: bool,
) -> AppRuntimeState {
    if windows
        .iter()
        .any(|w| w.state != WindowInstance::STATE_CLOSED)
    {
        return AppRuntimeState::Running;
    }
    if any_hibernated {
        return AppRuntimeState::Hibernated;
    }
    AppRuntimeState::Stopped
}

/// 组合 resolver：AppKind × 运行态 → AppCapabilities。
pub struct CapabilityResolver;

impl CapabilityResolver {
    /// `force_stop_supported` 仅对 system 有意义（平台 capability 位）。
    pub fn resolve(
        kind: &str,
        runtime_state: AppRuntimeState,
        force_stop_supported: bool,
    ) -> AppCapabilities {
        match kind {
            "local_project" => local_capabilities(runtime_state),
            "system_application" => system_capabilities(
                runtime_state == AppRuntimeState::Running,
                force_stop_supported,
            ),
            _ => web_capabilities(
                runtime_state == AppRuntimeState::Hibernated,
                runtime_state == AppRuntimeState::Running,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::model::AppRuntimeState;

    #[test]
    fn web_never_can_stop_or_restart() {
        for state in [
            AppRuntimeState::Stopped,
            AppRuntimeState::Starting,
            AppRuntimeState::Running,
            AppRuntimeState::Stopping,
            AppRuntimeState::Hibernated,
        ] {
            let c = CapabilityResolver::resolve("web_application", state, false);
            assert!(!c.can_stop, "web can_stop must be false ({state:?})");
            assert!(!c.can_restart, "web can_restart must be false ({state:?})");
            assert!(c.can_open && c.can_edit && c.can_remove && c.can_sidebar);
        }
    }

    #[test]
    fn local_matrix_tracks_runtime_state() {
        let running = local_capabilities(AppRuntimeState::Running);
        assert!(running.can_start && running.can_stop && running.can_restart);
        assert!(running.can_open);
        assert_eq!(running.risk_level, RISK_CONFIRM);

        let stopped = local_capabilities(AppRuntimeState::Stopped);
        assert!(stopped.can_start);
        assert!(!stopped.can_stop && !stopped.can_restart && !stopped.can_open);
        assert_eq!(stopped.risk_level, RISK_NONE);

        let orphan = local_capabilities(AppRuntimeState::Orphaned);
        assert_eq!(
            orphan.risk_level, RISK_STRONG_CONFIRM,
            "orphan resolve is a strong-confirm path"
        );
    }

    #[test]
    fn system_force_stop_is_capability_gated() {
        // 平台支持强杀：running 时 risk 2（强确认）。
        let c = system_capabilities(true, true);
        assert!(c.can_stop && c.can_restart && c.can_open);
        assert_eq!(c.risk_level, RISK_STRONG_CONFIRM);
        // 平台不支持强杀：risk 降到 1，force 路径由 service 返回 unsupported。
        let c2 = system_capabilities(true, false);
        assert_eq!(c2.risk_level, RISK_CONFIRM);
        // 未运行：不能 stop/restart，open(activate) 永远可用。
        let c3 = system_capabilities(false, true);
        assert!(!c3.can_stop && !c3.can_restart);
        assert!(c3.can_open);
        assert_eq!(c3.risk_level, RISK_NONE);
    }
}
