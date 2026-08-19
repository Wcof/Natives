//! Frame writing, size limits, and read deadlines for the UDS RPC channel.
//!
//! Split out of the former single-file `rpc.rs` (A2-03): every function here
//! serializes one response/error/event frame to the client write half.

use assistant_protocol::error::{DaemonError, ErrorCategory};
use assistant_protocol::v2::{
    RunEventV2, V2ErrorBody, V2ErrorResponse, V2Request, V2SuccessResponse,
};
use tokio::io::AsyncWriteExt;

/// Maximum size of a single UDS frame (handshake or RPC), including the
/// trailing newline. Frames larger than this are rejected and the connection
/// closed before more bytes are buffered (N04). Public so the frame-boundary
/// contract is testable from integration tests.
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

/// Deadline for a peer to deliver one full frame. Bounds slowloris / stalled
/// connections without affecting normal local request latency (N04).
pub(crate) const FRAME_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) async fn send_success(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request_id: &str,
    _client_id: &str,
    _session_token: &str,
    data: serde_json::Value,
) {
    let resp = V2SuccessResponse::new(request_id, data);
    let json = serde_json::to_string(&resp).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

/// Write one event as a newline-delimited `V2EventEnvelope` on a persistent
/// event stream (`run.watch`). Returns the I/O result so the caller can exit
/// cleanly when the client drops.
///
/// Legacy (V2EventEnvelope, stream-contract-v1): the RunWatchStreamV2 path now
/// writes [`crate::stream_protocol::RunStreamFrameV2`] frames via
/// [`write_stream_frame`]. Retained for legacy tooling/tests that still read
/// the old envelope shape; remove once those are gone.
#[allow(dead_code)]
pub(crate) async fn write_event_envelope(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    event: &RunEventV2,
) -> std::io::Result<()> {
    let envelope = assistant_protocol::v2::V2EventEnvelope {
        protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
        session_id: None,
        run_id: event.run_id.clone(),
        sequence: event.run_sequence,
        event_type: event.payload.type_name().to_string(),
        payload: serde_json::to_value(&event.payload).unwrap_or_default(),
        emitted_at: Some(event.timestamp.to_rfc3339()),
    };
    let json = serde_json::to_string(&envelope).unwrap_or_default();
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await
}

/// Write one `RunWatchStreamV2` frame as a newline-delimited JSON line on a
/// persistent `run.watch` stream. Returns the I/O result so the caller can exit
/// cleanly when the client drops.
pub(crate) async fn write_stream_frame(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    frame: &crate::stream_protocol::RunStreamFrameV2,
) -> std::io::Result<()> {
    let json = serde_json::to_string(frame).unwrap_or_default();
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await
}

/// Write one `Serialize` value as a newline-delimited JSON line. Generic over
/// the payload so non-`run.watch` streams (e.g. `model.gateway.stream`) share
/// the same framing without a bespoke writer (R-B3).
pub(crate) async fn write_json_line(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    value: &impl serde::Serialize,
) -> std::io::Result<()> {
    let json = serde_json::to_string(value).unwrap_or_default();
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await
}

/// Send an error response.
pub(crate) async fn send_error(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request_id: &str,
    error: &DaemonError,
) {
    let mut response = V2ErrorResponse::from_daemon_error(request_id, error);
    response.error.method_status = match error.code.as_str() {
        "unsupported" => Some(assistant_protocol::v2::MethodStatus::Unsupported),
        "invalid_request" => Some(assistant_protocol::v2::MethodStatus::InvalidRequest),
        _ => None,
    };
    let json = serde_json::to_string(&response).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

pub(crate) async fn send_invalid_request(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request_id: &str,
    message: impl Into<String>,
) {
    let response = V2ErrorResponse::invalid_request(request_id, message);
    let json = serde_json::to_string(&response).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

pub(crate) async fn send_rpc_failure(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &V2Request,
    code: &str,
    message: String,
) {
    let response = V2ErrorResponse::new(
        &request.request_id,
        V2ErrorBody {
            code: code.into(),
            category: ErrorCategory::Validation,
            retryable: false,
            message,
            details: None,
            method_status: None,
            correlation_id: Some(uuid::Uuid::new_v4().to_string()),
        },
    );
    let json = serde_json::to_string(&response).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}
