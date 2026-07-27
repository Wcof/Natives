//! Protocol v2 wire envelopes.
//!
//! These types are the formal v2 request/response/event surface. Call sites
//! should prefer them over the legacy v1 `RpcRequest` / `RpcResponse` shells.
//! During migration, servers may still accept v1-shaped JSON and normalize
//! into these types.

use crate::error::{DaemonError, ErrorCategory};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PROTOCOL_V2;

/// How the daemon treats a named RPC method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodStatus {
    /// Fully implemented and safe to advertise.
    Implemented,
    /// Known catalogue entry but not yet implemented (fail-closed).
    Unsupported,
    /// Malformed request (missing fields, bad types, etc.).
    InvalidRequest,
}

/// Unified v2 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2Request {
    pub protocol_version: String,
    pub request_id: String,
    pub session_id: Option<String>,
    pub client_id: String,
    pub session_token: String,
    /// Optional run correlation for audit / broker binding.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Client-supplied idempotency key to prevent duplicate execution.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Unified v2 success response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2SuccessResponse {
    pub protocol_version: String,
    pub request_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    pub success: bool,
    pub data: Value,
}

/// Unified v2 error response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ErrorResponse {
    pub protocol_version: String,
    pub request_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    pub success: bool,
    pub error: V2ErrorBody,
}

/// Structured error body (stable for UI + logs; never contains secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2ErrorBody {
    pub code: String,
    pub category: ErrorCategory,
    pub retryable: bool,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
    /// Method disposition when relevant.
    #[serde(default)]
    pub method_status: Option<MethodStatus>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

/// Unified event envelope for push/subscribe and replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2EventEnvelope {
    pub protocol_version: String,
    pub session_id: Option<String>,
    pub run_id: String,
    pub sequence: u64,
    pub event_type: String,
    pub payload: Value,
    /// Monotonic server timestamp (RFC3339) when available.
    #[serde(default)]
    pub emitted_at: Option<String>,
}

impl V2Request {
    pub fn new(
        client_id: impl Into<String>,
        session_token: impl Into<String>,
        method: impl Into<String>,
        params: Value,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_V2.to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            session_id: None,
            client_id: client_id.into(),
            session_token: session_token.into(),
            run_id: None,
            idempotency_key: None,
            method: method.into(),
            params,
        }
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

impl V2SuccessResponse {
    pub fn new(request_id: impl Into<String>, data: Value) -> Self {
        Self {
            protocol_version: PROTOCOL_V2.to_string(),
            request_id: request_id.into(),
            session_id: None,
            run_id: None,
            success: true,
            data,
        }
    }
}

impl V2ErrorResponse {
    pub fn new(request_id: impl Into<String>, body: V2ErrorBody) -> Self {
        Self {
            protocol_version: PROTOCOL_V2.to_string(),
            request_id: request_id.into(),
            session_id: None,
            run_id: None,
            success: false,
            error: body,
        }
    }

    pub fn unsupported(request_id: impl Into<String>, method: &str) -> Self {
        Self::new(
            request_id,
            V2ErrorBody {
                code: "unsupported".into(),
                category: ErrorCategory::Unsupported,
                retryable: false,
                message: format!("method not implemented: {method}"),
                details: Some(serde_json::json!({ "method": method })),
                method_status: Some(MethodStatus::Unsupported),
                correlation_id: Some(uuid::Uuid::new_v4().to_string()),
            },
        )
    }

    pub fn invalid_request(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(
            request_id,
            V2ErrorBody {
                code: "invalid_request".into(),
                category: ErrorCategory::Validation,
                retryable: false,
                message: message.into(),
                details: None,
                method_status: Some(MethodStatus::InvalidRequest),
                correlation_id: Some(uuid::Uuid::new_v4().to_string()),
            },
        )
    }

    pub fn from_daemon_error(request_id: impl Into<String>, err: &DaemonError) -> Self {
        Self::new(
            request_id,
            V2ErrorBody {
                code: err.code.clone(),
                category: err.category.clone(),
                retryable: err.retryable,
                message: err.technical_message.clone(),
                details: None,
                method_status: None,
                correlation_id: Some(err.correlation_id.clone()),
            },
        )
    }
}

/// Wire response that is either success or error (for ser/de of a single line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum V2Response {
    Success(V2SuccessResponse),
    Error(V2ErrorResponse),
}

impl V2Response {
    pub fn is_success(&self) -> bool {
        matches!(self, V2Response::Success(_))
    }

    pub fn request_id(&self) -> &str {
        match self {
            V2Response::Success(s) => &s.request_id,
            V2Response::Error(e) => &e.request_id,
        }
    }
}

/// Normalize a legacy v1-style JSON object into a [`V2Request`] when possible.
pub fn parse_request_compat(value: &Value) -> Result<V2Request, String> {
    if let Ok(req) = serde_json::from_value::<V2Request>(value.clone()) {
        if !req.method.is_empty() {
            return Ok(req);
        }
    }
    // Legacy RpcRequest shape
    let protocol_version = value
        .get("protocol_version")
        .and_then(|v| v.as_str())
        .unwrap_or(PROTOCOL_V2)
        .to_string();
    let request_id = value
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let client_id = value
        .get("client_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_token = value
        .get("session_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let method = value
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing method".to_string())?
        .to_string();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let session_id = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let run_id = value
        .get("run_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let idempotency_key = value
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if request_id.is_empty() {
        return Err("missing request_id".into());
    }
    Ok(V2Request {
        protocol_version,
        request_id,
        session_id,
        client_id,
        session_token,
        run_id,
        idempotency_key,
        method,
        params,
    })
}

/// Convert a v2 response into the legacy RpcResponse-shaped JSON for old clients.
pub fn response_to_legacy_json(resp: &V2Response) -> Value {
    match resp {
        V2Response::Success(s) => serde_json::json!({
            "protocol_version": s.protocol_version,
            "request_id": s.request_id,
            "success": true,
            "data": s.data,
            "error": null,
            "session_id": s.session_id,
            "run_id": s.run_id,
        }),
        V2Response::Error(e) => serde_json::json!({
            "protocol_version": e.protocol_version,
            "request_id": e.request_id,
            "success": false,
            "data": null,
            "error": {
                "code": e.error.code,
                "category": e.error.category,
                "retryable": e.error.retryable,
                "technical_message": e.error.message,
                "message": e.error.message,
                "details": e.error.details,
                "method_status": e.error.method_status,
                "correlation_id": e.error.correlation_id,
            },
            "session_id": e.session_id,
            "run_id": e.run_id,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = V2Request::new("c1", "tok", "run.start", serde_json::json!({"x": 1}))
            .with_run_id("r1")
            .with_idempotency_key("idem-1");
        let json = serde_json::to_string(&req).unwrap();
        let back: V2Request = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "run.start");
        assert_eq!(back.run_id.as_deref(), Some("r1"));
        assert_eq!(back.idempotency_key.as_deref(), Some("idem-1"));
        assert_eq!(back.protocol_version, PROTOCOL_V2);
    }

    #[test]
    fn unsupported_error_has_status() {
        let err = V2ErrorResponse::unsupported("req-1", "scheduler.create");
        assert!(!err.success);
        assert_eq!(err.error.method_status, Some(MethodStatus::Unsupported));
        assert_eq!(err.error.code, "unsupported");
    }

    #[test]
    fn parse_legacy_rpc_request() {
        let v = serde_json::json!({
            "protocol_version": "2.0.0",
            "request_id": "r1",
            "client_id": "c",
            "session_token": "t",
            "method": "daemon.ping",
            "params": {}
        });
        let req = parse_request_compat(&v).unwrap();
        assert_eq!(req.method, "daemon.ping");
        assert_eq!(req.request_id, "r1");
    }

    #[test]
    fn legacy_response_shape() {
        let ok = V2Response::Success(V2SuccessResponse::new(
            "r",
            serde_json::json!({"pong": true}),
        ));
        let j = response_to_legacy_json(&ok);
        assert_eq!(j["success"], true);
        assert_eq!(j["data"]["pong"], true);
    }
}
