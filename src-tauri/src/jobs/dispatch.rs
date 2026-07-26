//! jobs/dispatch.rs — 派发接缝（接缝 A，契约第 3 节）
//!
//! P0 冻结类型，唯一适配器 NotWiredDispatcher（is_wired()=false，
//! dispatch()=Err(NotWired)）。P1 适配器将把 JobDefinition 映射为
//! CreateRunRequest/StartRunRequest（assistant-protocol v2 run.*），经 Host 的
//! daemon_authority 走 run.* 主链；禁止绕过单执行接口。

use super::store::JobDefinition;
use serde::Serialize;

/// 派发回执：引擎侧会话 / run 标识 + 幂等键。
#[derive(Debug, Clone, Serialize)]
pub struct DispatchReceipt {
    pub conversation_id: String,
    pub run_id: String,
    pub idempotency_key: String,
}

/// 触发来源（契约第 2 节 trigger 列：schedule | manual）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Schedule,
    Manual,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Schedule => "schedule",
            Trigger::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// 执行链路未接线（P0 唯一现实）。
    NotWired,
    /// Job 定义无法映射为引擎请求。
    InvalidJob(String),
    /// 引擎侧失败。
    Engine(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::NotWired => write!(f, "dispatcher not wired"),
            DispatchError::InvalidJob(m) => write!(f, "invalid job: {m}"),
            DispatchError::Engine(m) => write!(f, "engine error: {m}"),
        }
    }
}

/// 幂等键约定（契约第 3 节）：`job-{job_id}-{scheduled_at_unix}`。
/// P1 对接时随派发传给引擎。
pub fn idempotency_key(job_id: &str, scheduled_at_unix: i64) -> String {
    format!("job-{job_id}-{scheduled_at_unix}")
}

#[async_trait::async_trait]
pub trait JobDispatcher: Send + Sync {
    fn is_wired(&self) -> bool;
    async fn dispatch(
        &self,
        job: &JobDefinition,
        trigger: Trigger,
    ) -> Result<DispatchReceipt, DispatchError>;
}

/// P0 唯一适配器：执行链路未接线，诚实报告 NotWired（无假绿，R-F2）。
pub struct NotWiredDispatcher;

#[async_trait::async_trait]
impl JobDispatcher for NotWiredDispatcher {
    fn is_wired(&self) -> bool {
        false
    }

    async fn dispatch(
        &self,
        _job: &JobDefinition,
        _trigger: Trigger,
    ) -> Result<DispatchReceipt, DispatchError> {
        Err(DispatchError::NotWired)
    }
}
