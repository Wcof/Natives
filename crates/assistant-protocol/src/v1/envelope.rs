use serde::{Deserialize, Serialize};
use crate::v1::daemon::RpcRequest;
use crate::v1::daemon::RpcResponse;

/// ProtocolEnvelope wraps every RPC message with version and routing metadata.
///
/// This is the outermost layer of every message sent between the client and daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEnvelope {
    /// Protocol version string (e.g. "0.1.0").
    pub protocol_version: String,
    /// Unique request identifier for correlation.
    pub request_id: String,
    /// Client identifier (set during handshake).
    pub client_id: String,
    /// Session token (set during handshake).
    pub session_token: String,
    /// The inner message.
    #[serde(flatten)]
    pub payload: EnvelopePayload,
}

/// The payload of a protocol envelope — either a request or a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvelopePayload {
    /// An RPC request.
    Request(RpcRequest),
    /// An RPC response.
    Response(RpcResponse),
}

impl ProtocolEnvelope {
    /// Create a new request envelope.
    pub fn new_request(
        protocol_version: String,
        client_id: String,
        session_token: String,
        method: String,
        params: serde_json::Value,
    ) -> Self {
        let request_id = uuid::Uuid::new_v4().to_string();
        let pv = protocol_version.clone();
        let cid = client_id.clone();
        let st = session_token.clone();
        ProtocolEnvelope {
            protocol_version,
            request_id: request_id.clone(),
            client_id,
            session_token,
            payload: EnvelopePayload::Request(RpcRequest {
                protocol_version: pv,
                request_id,
                client_id: cid,
                session_token: st,
                method,
                params,
            }),
        }
    }

    /// Create a new response envelope.
    pub fn new_response(
        protocol_version: String,
        request_id: String,
        client_id: String,
        session_token: String,
        success: bool,
        data: Option<serde_json::Value>,
        error: Option<serde_json::Value>,
    ) -> Self {
        let pv = protocol_version.clone();
        let rid = request_id.clone();
        ProtocolEnvelope {
            protocol_version,
            request_id,
            client_id,
            session_token,
            payload: EnvelopePayload::Response(RpcResponse {
                protocol_version: pv,
                request_id: rid,
                success,
                data,
                error,
            }),
        }
    }
}