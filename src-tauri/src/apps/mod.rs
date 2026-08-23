//! Apps 目标域（ADR-0020 / 05-MODULE-REMEDIATION-PLAN §3）。
//!
//! 收敛为 App / RuntimeSpec / RuntimeInstance / Surface 四元边界，运行与呈现分离。
//! 资产复用 `creative_app` 成熟实现；本模块提供目标类型边界、Repository（唯一 SQL
//! 层）、AppsService（统一业务入口）与三类 Driver（local / system / web）。
//!
//! 分层（Phase C，APP-014~021）：
//! - [`repository`] —— `applications` / 三类 spec / `runtime_instances` 的强类型 SQL
//!   （唯一写库层；所有写操作事务化、幂等）。
//! - [`capabilities`] —— `CapabilityResolver`：按 AppKind + 运行态输出动作矩阵与风险级。
//! - [`service`] —— `AppsService`：统一入口（list/get/register/update/remove/open/
//!   start/stop/force_stop/restart/set_sidebar/local_logs/local_resolve_orphan），
//!   内部走 MutationLock（按 app_id 串行）+ repository + driver + 操作日志。
//! - [`local`] / [`system`] / [`web`] —— 三类 Driver；local 适配
//!   `creative_app::local` 成熟 helper（不重写进程/PGID 逻辑），system/web 本轮为
//!   skeleton（Phase E/F 落平台实现）。
//! - [`facade`] —— 过渡只读 facade（Phase H 删除）；新读一律走 repository。

pub mod capabilities;
pub mod local;
pub mod model;
pub mod mutation_lock;
pub mod repository;
pub mod service;
pub mod system;
pub mod web;
pub mod web_url;

#[cfg(test)]
mod tests;

pub use model::{App, AppView, RegistrationOrigin, RuntimeInstance, RuntimeSpec, Surface};
pub use mutation_lock::{new_mutation_lock, MutationLock, MutationLockRegistry};
pub use service::{AppsService, AppsServiceDeps};

/// 应用中心统一事件 channel（APP-019 / 06 契约）。
///
/// 所有 Apps 域 DB 变更 / lifecycle 变更都 emit 到 `db-state-changed` 的
/// `apps` channel，payload = `{ action, id, kind }`（`id` 为统一 application id）。
/// Renderer 只需监听 `apps` 一个 channel 即可刷新应用中心（R-S9）。
///
/// 迁移说明：`creative_app::local` lifecycle 广播已从旧 `creative-app` channel
/// 直接切换到 `apps`（旧监听方 Phase H 删除；不保留双 emit，避免双刷新）。
pub const EVENT_CHANNEL_APPS: &str = "apps";

/// Emit 一条 Apps 域事件（统一 `db-state-changed` / channel=apps 信封）。
///
/// `action`：`registered` / `updated` / `removed` / `sidebar` / `starting` /
/// `started` / `stopping` / `stopped` / `force_stopped` / `restarted` /
/// `start_failed` / `stop_failed` / `orphaned` / `orphan_resolved` / `reconciled` /
/// `web_opened` / `web_closed` / `web_data_cleared` …
pub fn emit_apps_event<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    action: &str,
    app_id: &str,
    kind: &str,
) {
    crate::emit_db_state_changed(
        app_handle,
        EVENT_CHANNEL_APPS,
        serde_json::json!({ "action": action, "id": app_id, "kind": kind }),
    );
}
