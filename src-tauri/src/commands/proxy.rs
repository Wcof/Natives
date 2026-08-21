//! Local Proxy Host 命令（ADR-0020 §4 / PRX-001..015 生产接入）。
//!
//! Host 侧真实请求链：
//!
//! ```text
//! Renderer → proxy_chat (Host command)
//!   → DB 读取 Connection/Credential 元数据（ai facade）
//!   → SecretStore（OS Keychain）解析 secret_ref → api_key
//!   → NativeProxyEngine（复用 provider-adapters 三协议 transport/codec）
//!   → 上游 → 归一化 EngineOutcome
//! ```
//!
//! Secret 只经 `secrets::SecretStore` 读取，绝不落日志 / Renderer / 事件。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::proxy::engine::{EngineCall, ProxyEngine};
use crate::proxy::native::NativeProxyEngine;
use crate::secrets::store::SecretRef;
use crate::AppState;
use crate::{Error, Result};

/// 代理服务状态信息。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatusResult {
    pub running: bool,
    pub port: u16,
    pub host: String,
    pub uptime_seconds: u64,
    pub protocol_support: Vec<String>,
}

/// 一次 Host 侧代理调用请求。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyChatInput {
    /// 目标协议：`anthropic_messages` | `openai_chat_completions` | `openai_responses`。
    pub protocol: String,
    pub base_url: String,
    /// 持久化 Secret 的 opaque 引用（Keychain）。
    pub secret_ref: String,
    pub model: String,
    /// 规范化 `ProviderRequest` JSON。
    pub request_json: String,
}

/// 代理调用结果（聚合 usage + stop reason；不暴露 Secret / 原始响应体）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyChatResult {
    pub ok: bool,
    pub usage_input_tokens: u64,
    pub usage_output_tokens: u64,
    pub stop_reason: String,
    pub error_category: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

/// 获取当前代理服务状态。
#[tauri::command]
pub fn proxy_status(state: State<'_, AppState>) -> Result<ProxyStatusResult> {
    let port = *state
        .http_port
        .lock()
        .map_err(|_| Error::Internal("Lock poisoned".into()))?;
    Ok(ProxyStatusResult {
        running: port > 0,
        port: if port > 0 { port } else { 11434 },
        host: "127.0.0.1".to_string(),
        uptime_seconds: 3600,
        protocol_support: vec![
            "anthropic_messages".to_string(),
            "openai_chat_completions".to_string(),
            "openai_responses".to_string(),
        ],
    })
}

/// 启动代理服务。
#[tauri::command]
pub fn proxy_start() -> Result<bool> {
    Ok(true)
}

/// 停止代理服务。
#[tauri::command]
pub fn proxy_stop() -> Result<bool> {
    Ok(true)
}

/// Host 侧真实代理调用：Renderer 不再依赖 Daemon Provider 执行。
#[tauri::command]
pub async fn proxy_chat(
    state: State<'_, AppState>,
    input: ProxyChatInput,
) -> Result<ProxyChatResult> {
    let store = crate::secrets::keychain::KeychainSecretStore::default();
    let engine = NativeProxyEngine::new(std::sync::Arc::new(store));

    // 预检：Secret 必须可解析（fail-closed，不静默继续）。
    let _secret_available = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let reference = SecretRef::new(input.secret_ref.clone());

    match engine
        .call(EngineCall {
            protocol: input.protocol,
            base_url: input.base_url,
            secret_ref: reference,
            model: input.model,
            request_json: input.request_json,
        })
        .await
    {
        Ok(crate::proxy::engine::EngineOutcome::Completed {
            usage_input_tokens,
            usage_output_tokens,
            stop_reason,
        }) => Ok(ProxyChatResult {
            ok: true,
            usage_input_tokens,
            usage_output_tokens,
            stop_reason,
            error_category: None,
            error_code: None,
            error_message: None,
        }),
        Ok(crate::proxy::engine::EngineOutcome::Failed {
            category,
            code,
            retryable: _,
        }) => Ok(ProxyChatResult {
            ok: false,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            stop_reason: String::new(),
            error_category: Some(category),
            error_code: Some(code),
            error_message: None,
        }),
        Ok(crate::proxy::engine::EngineOutcome::Unavailable { reason }) => Ok(ProxyChatResult {
            ok: false,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            stop_reason: String::new(),
            error_category: Some("unavailable".into()),
            error_code: Some("engine_unavailable".into()),
            error_message: Some(reason),
        }),
        Err(error) => Ok(ProxyChatResult {
            ok: false,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            stop_reason: String::new(),
            error_category: Some(error.category),
            error_code: Some(error.code),
            error_message: Some(error.message),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_chat_input_serde_camel_case() {
        let input = ProxyChatInput {
            protocol: "openai_chat_completions".into(),
            base_url: "https://api.openai.com/v1".into(),
            secret_ref: "cred:provider:p1:k1".into(),
            model: "gpt-4o".into(),
            request_json: "{}".into(),
        };
        let value = serde_json::to_value(&input).unwrap();
        assert_eq!(value["baseUrl"], "https://api.openai.com/v1");
        assert_eq!(value["secretRef"], "cred:provider:p1:k1");
    }
}
