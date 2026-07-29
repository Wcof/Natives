//! jobs — 任务模块（Job Module）Host 侧实现
//!
//! 契约：任务模块接口契约冻结稿 v1。
//! - store：scheduled_tasks / task_runs 复用扩展与 CRUD
//! - schedule：once/interval/cron 解析与 next_run 纯函数
//! - runner：30s tick 循环
//! - dispatch：经 Host daemon_authority 接入 run.* 单一执行主链

pub mod dispatch;
pub mod migration;
pub mod runner;
pub mod schedule;
pub mod store;

use dispatch::{JobDispatcher, NativeJobDispatcher};
use std::sync::{Arc, OnceLock};

/// 进程级派发器：Job 只经 Host daemon_authority 进入 Daemon run.* 主链。
pub fn global_dispatcher() -> Arc<dyn JobDispatcher> {
    static DISPATCHER: OnceLock<Arc<dyn JobDispatcher>> = OnceLock::new();
    DISPATCHER
        .get_or_init(|| Arc::new(NativeJobDispatcher::new()))
        .clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn global_dispatcher_is_wired_to_the_native_run_authority() {
        assert!(super::global_dispatcher().is_wired());
    }
}
