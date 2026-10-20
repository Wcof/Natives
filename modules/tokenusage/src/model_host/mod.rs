//! model_host: 针对 Model Host 8766 进程外只读查询端口的客户端与 DTO 契约。
//!
//! 依据：ADR-0033 / 架构整改 Wave 2 / 审计 A-01。
//! TokenUsage 模块不再直接探测或读取 Model Host 的 `usage.db` 与 `state.json`，
//! 所有 AI 用量与额度数据统一由 8766 窄接口提供。

mod cache;
pub mod client;
pub mod dto;
pub mod error;

pub use client::ModelUsageQueryClient;
pub use dto::*;
pub use error::ModelQueryError;
