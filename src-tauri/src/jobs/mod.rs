//! jobs — 任务模块（Job Module）Host 侧实现
//!
//! 契约：任务模块接口契约冻结稿 v1。
//! - store：scheduled_tasks / task_runs 复用扩展与 CRUD
//! - schedule：once/interval/cron 解析与 next_run 纯函数
//! - runner：30s tick 循环（NotWired 语义见契约第 4 节）
//! - dispatch：派发接缝（接缝 A，P0 唯一适配器 NotWiredDispatcher）

pub mod dispatch;
pub mod runner;
pub mod schedule;
pub mod store;

use dispatch::{JobDispatcher, NotWiredDispatcher};
use std::sync::{Arc, OnceLock};

/// 进程级派发器。P0 固定 NotWired；P1 在此接入 run.* 主链适配器。
pub fn global_dispatcher() -> Arc<dyn JobDispatcher> {
    static DISPATCHER: OnceLock<Arc<dyn JobDispatcher>> = OnceLock::new();
    DISPATCHER
        .get_or_init(|| Arc::new(NotWiredDispatcher))
        .clone()
}
