//! runtime/native/mod.rs — Native Runtime 子模块
//!
//! 原子能力层架构（参考 Claude Code 能力模型）：
//!
//! ┌─────────────────────────────────────────┐
//! │            AgentLoop (状态机)             │
//! │  Thinking → ToolExec → Observing → Done │
//! └──────────┬──────────────────────┬───────-┘
//!            │ 流式通信              │ 能力调度
//!     ┌──────▼──────┐      ┌────────▼────────┐
//!     │StreamProvider│      │CapabilityRegistry│
//!     │ (SSE 消费)   │      │ (原子能力注册表)  │
//!     └──────────────┘      └────────┬────────┘
//!                           ┌────────▼────────┐
//!                           │  HookPipeline   │
//!                           │ Pre/Post hooks  │
//!                           └────────┬────────┘
//!                           ┌────────▼────────┐
//!                           │   RuleEngine    │
//!                           │ 条件匹配策略引擎 │
//!                           └─────────────────┘

pub mod capability;
pub mod command_agent_skill;
pub mod hook_pipeline;
pub mod plugin_system;
pub mod rule_engine;
pub mod subagent;
