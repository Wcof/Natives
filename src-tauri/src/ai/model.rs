//! AI Resources 目标边界（ADR-0020 §4 / 05-MODULE-REMEDIATION-PLAN §4）
//!
//! 收敛后的 AI 资源模型：
//!
//! - `Provider`    —— 厂商 / 预设（`user_providers` 表 read-through）
//! - `Connection`  —— 真实 upstream endpoint / protocol / 网络配置
//! - `Credential`  —— 独立于 Connection 的凭证（支持多 Key），只保存
//!                    非敏感元数据 + opaque `secret_ref`（OS Keychain）
//! - `Model`       —— 模型目录条目（归属 Connection）
//!
//! 不变量（05 §4）：
//! - Connection 永远不保存 Secret；
//! - Credential 不被一个 Connection 独占（共享 Key 可服务多个 Connection）；
//! - 持久 Secret 由 `secrets::SecretStore`（OS Keychain）持有，DB 只存 `secret_ref`。

use serde::{Deserialize, Serialize};

/// 厂商 / 预设 —— `user_providers` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub preset_name: String,
    pub api_protocol: String,
    pub name: String,
    pub website_url: String,
    pub base_url: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 真实 upstream —— 只含访问属性，**绝不保存 Secret**。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub api_protocol: String,
    /// 网络层代理（可选，非 Secret）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// 凭证 —— 独立于 Connection，支持多 Key。
///
/// 只保存非敏感元数据与 opaque `secret_ref`；明文 Secret 在 OS Keychain
/// （`secrets::SecretStore`，R-S12）。Renderer 只可见掩码。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub provider_id: String,
    pub label: String,
    /// Keychain 引用（opaque，绝不携带 Secret 内容）。
    pub secret_ref: String,
    /// 展示用掩码（如 `sk-a…1b2c`），从 Keychain 读取后派生，不持久化明文。
    pub masked_key: String,
    pub is_primary: bool,
    pub is_active: bool,
    pub status: String,
}

/// 模型目录条目 —— 归属 Connection。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub connection_id: String,
    pub model_id: String,
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}
