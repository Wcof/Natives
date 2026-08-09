//! Protocol v2 wire envelopes.
//!
//! These types are the formal v2 request/response/event surface and the only
//! wire shape for protocol v2 (MIG-004: v1 `RpcRequest` / `RpcResponse` shell
//! normalization is retired).

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
}
