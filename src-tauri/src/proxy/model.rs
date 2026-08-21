//! Local Proxy 目标边界（ADR-0020 §4 / 05-MODULE-REMEDIATION-PLAN §5）
//!
//! AiNative 拥有 Proxy 产品能力与配置 SoT：
//!
//! ```text
//! ProxyService
//! ├─ Listener policy          （localhost 入站）
//! ├─ Route definitions        （RouteTarget = Connection + Model + CredentialSelector）
//! ├─ Credential pool policy   （priority / RR / failover / cooldown）
//! ├─ Usage normalization      （复用现有 usage 资产）
//! └─ Engine supervision       （可替换 ProxyEngine）
//! ```
//!
//! V1 只允许：localhost、model/connection routing、credential pool、
//! priority/RR/failover/cooldown、三协议转换、usage/error。
//! 明确不做：多租户、虚拟 Key、WAF、Guardrail、组织预算、策略 DSL、语义路由。

use serde::{Deserialize, Serialize};

/// 本地入站 listener 策略（默认 127.0.0.1）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListenerConfig {
    pub enabled: bool,
    /// 仅 localhost（个人轻量本地代理，不暴露 LAN 默认）。
    pub host: String,
    pub port: u16,
}

impl Default for ListenerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".to_string(),
            port: 15721,
        }
    }
}

/// 运行态路由目标：Connection + Model + CredentialSelector（05 §6 不变量）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteTarget {
    pub connection_id: String,
    pub model_id: String,
    /// 凭证选择：具体 credential id（显式）或池策略（优先级 / 轮转）。
    pub credential_selector: CredentialSelector,
}

/// Credential 选择策略（API Key Pool / OAuth 池）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CredentialSelector {
    /// 显式单 Key（确定性）。
    Credential { id: String },
    /// Key Pool：公平轮转 / 失败切换 / 冷却（复用 key_pool.rs 语义）。
    Pool { policy: PoolPolicy },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PoolPolicy {
    /// 优先级优先，同优先级 round-robin（默认）。
    PriorityRoundRobin,
    /// 严格轮转。
    RoundRobin,
}

/// 持久化 Route 定义（配置 SoT 在 AiNative，Engine 运行态 health 可 ephemeral）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteDefinition {
    pub id: String,
    pub name: String,
    pub target: RouteTarget,
    pub enabled: bool,
    pub priority: u32,
}

/// Proxy 运行状态摘要（可观测，不暴露 Secret）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub listener_enabled: bool,
    pub host: String,
    pub port: u16,
    pub route_count: usize,
    pub engine: String,
    pub last_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_defaults_to_localhost_off() {
        let config = ListenerConfig::default();
        assert!(!config.enabled, "默认不暴露入站");
        assert_eq!(config.host, "127.0.0.1", "个人轻量本地代理仅 localhost");
    }

    #[test]
    fn route_target_serializes_camel_case() {
        let route = RouteDefinition {
            id: "r1".into(),
            name: "Anthropic Claude".into(),
            target: RouteTarget {
                connection_id: "conn-p1".into(),
                model_id: "claude-sonnet-4".into(),
                credential_selector: CredentialSelector::Pool {
                    policy: PoolPolicy::PriorityRoundRobin,
                },
            },
            enabled: true,
            priority: 10,
        };
        let json = serde_json::to_value(&route).unwrap();
        assert_eq!(json["target"]["connectionId"], "conn-p1");
        assert_eq!(
            json["target"]["credentialSelector"]["pool"]["policy"],
            "priorityRoundRobin"
        );
    }

    #[test]
    fn explicit_credential_selector_roundtrips() {
        let selector = CredentialSelector::Credential { id: "k1".into() };
        let json = serde_json::to_string(&selector).unwrap();
        assert!(json.contains("\"credential\""));
        let back: CredentialSelector = serde_json::from_str(&json).unwrap();
        assert_eq!(back, selector);
    }
}
