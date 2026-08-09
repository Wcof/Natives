//! Frame writing, size limits, and read deadlines for the UDS RPC channel.
//!
//! Split out of the former single-file `rpc.rs` (A2-03): every function here
//! serializes one response/error/event frame to the client write half.

use assistant_protocol::error::DaemonError;
use assistant_protocol::v1::daemon::{RpcRequest, RpcResponse};
use assistant_protocol::v2::RunEventV2;
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
    let resp = RpcResponse {
        // Must match DaemonCapabilities / handshake (PROTOCOL_V2), never a stale 0.1.0.
        protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
        request_id: request_id.to_string(),
        success: true,
        data: Some(data),
        error: None,
    };
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
        sequence: event.effective_run_sequence(),
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

/// Send an error response.
pub(crate) async fn send_error(writer: &mut tokio::net::unix::OwnedWriteHalf, error: &DaemonError) {
    let json = serde_json::to_string(error).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}

pub(crate) async fn send_rpc_failure(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &RpcRequest,
    code: &str,
    message: String,
) {
    let response = RpcResponse {
        protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
        request_id: request.request_id.clone(),
        success: false,
        data: None,
        error: Some(serde_json::json!({"code": code, "message": message})),
    };
    let json = serde_json::to_string(&response).unwrap_or_default();
    let _ = writer.write_all(json.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}
