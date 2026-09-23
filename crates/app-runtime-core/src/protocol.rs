//! 内置模块 App v1 协议常量与 wire 类型（托管应用契约 §4/§5）。
//!
//! 单一来源：扩展页面、Core Host 与 App Host 的类型/校验资料从这里导出，
//! 禁止各端手写影子枚举。

use serde::{Deserialize, Serialize};

/// 内置模块 Host 运行协议当前版本（App Runtime Protocol v2，唯一生产版本）。
pub const APP_PROTOCOL_VERSION: u32 = 2;
pub const APP_PROTOCOL_VERSION_V2: u32 = 2;
/// Natives 产品配置协议当前版本（Core Apps Protocol v5，唯一生产版本）。
pub const PRODUCT_PROTOCOL_VERSION: u32 = 5;
pub const PRODUCT_PROTOCOL_VERSION_V5: u32 = 5;

/// Native/App 单帧上限：严格小于 Chrome 1 MiB 出站上限。
pub const MAX_FRAME_BYTES: usize = 512 * 1024;
/// 会话 token 字节数（32 字节 CSPRNG，base64url 43 字符）。
pub const SESSION_TOKEN_BYTES: usize = 32;
/// 会话最长时间。
pub const SESSION_TTL_SECS: u64 = 15 * 60;
/// instanceId/generation/challenge 随机字节数（≥128 位）。
pub const INSTANCE_ID_BYTES: usize = 16;
/// 每 OS 用户命名空间最大并发应用实例。
pub const MAX_RUNTIME_SLOTS: usize = 4;

/// App v1 请求帧：{id, method, params}。未知字段拒绝由解析层保证。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// App v1 成功/失败响应帧。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppResponse<R> {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::error::AppErrorBody>,
}

/// app:state 事件：{event, instanceId, sequence, data}，sequence 单调递增。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEvent {
    pub event: &'static str,
    pub instance_id: String,
    pub sequence: u64,
    pub data: AppEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEventData {
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
}

/// 真实执行状态描述；deadlineAt 必须有界。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub kind: String,
    pub cancellable: bool,
    pub started_at: u64,
    pub deadline_at: u64,
}

/// app:handshake v2 请求参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeParamsV2 {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "appId")]
    pub app_id: String,
    #[serde(rename = "productVersion", default)]
    pub product_version: Option<String>,
    #[serde(rename = "activationGeneration", default)]
    pub activation_generation: Option<u64>,
}

/// app:handshake v2 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResultV2 {
    pub protocol_version: u32,
    pub app_id: String,
    pub state: String,
    pub module_api_version: u32,
    pub data_schema_version: u32,
    pub capability_version: u32,
}

/// app:handshake 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResult {
    pub protocol_version: u32,
    pub app_id: String,
    pub app_version: String,
    pub data_schema_range: SchemaRange,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRange {
    pub readable: String,
    pub writable: String,
}

/// app:start 结果：真实 loopback 端口与会话代。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub instance_id: String,
    pub port: u16,
    pub generation: String,
    pub state: String,
}

/// app:status 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResult {
    pub instance_id: String,
    pub state: String,
    pub operation: Option<Operation>,
}

/// app:data_status 结果：数据 schema 与迁移 journal 摘要；不返回业务记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStatusResult {
    pub current_schema: u32,
    pub migration_state: String,
    pub last_data_writer_version: String,
    pub has_committed_new_writes: bool,
    pub previous_version_compatible: bool,
}

/// app:session issue 结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIssueResult {
    pub generation: String,
    pub token: String,
    pub expires_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_and_unknown_shape() {
        let request = AppRequest {
            id: "r1".into(),
            method: "app:handshake".into(),
            params: serde_json::json!({"protocolVersion": 2}),
        };
        let bytes = serde_json::to_vec(&request).unwrap();
        let parsed: AppRequest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.method, "app:handshake");
        assert_eq!(APP_PROTOCOL_VERSION, 2);
        assert_eq!(PRODUCT_PROTOCOL_VERSION, 5);
        assert_eq!(MAX_FRAME_BYTES, 512 * 1024);
    }

    #[test]
    fn response_omits_absent_sides() {
        let ok: AppResponse<u32> = AppResponse {
            id: "1".into(),
            ok: true,
            result: Some(3),
            error: None,
        };
        let text = serde_json::to_string(&ok).unwrap();
        assert!(!text.contains("error"));
        let failed: AppResponse<u32> = AppResponse {
            id: "1".into(),
            ok: false,
            result: None,
            error: Some(crate::error::AppErrorBody::new(
                crate::error::AppErrorCode::SessionInvalid,
                "bad token",
                false,
            )),
        };
        let text = serde_json::to_string(&failed).unwrap();
        assert!(text.contains("APP_SESSION_INVALID"));
    }
}
