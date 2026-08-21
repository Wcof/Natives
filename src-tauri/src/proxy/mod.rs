//! Local Proxy 目标域（ADR-0020 §4 / 05-MODULE-REMEDIATION-PLAN §5）
//!
//! 个人轻量本地代理：listener policy + route definitions + credential pool +
//! failover/cooldown + 三协议转换（可替换 `ProxyEngine`）+ usage 归一化。
//! 不建设企业 AI Gateway。配置 SoT 在 AiNative；Engine 运行态 health 可 ephemeral。

pub mod engine;
pub mod model;
pub mod native;

pub use engine::{EngineCall, EngineError, EngineOutcome, ProxyEngine};
pub use model::{
    CredentialSelector, ListenerConfig, PoolPolicy, ProxyStatus, RouteDefinition, RouteTarget,
};
pub use native::NativeProxyEngine;
