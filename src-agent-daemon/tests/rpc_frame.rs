//! TASK-003 (N04): bounded UDS frames.
//!
//! Oversized / slow / malformed connections must be terminated with bounded
//! memory and structured errors, and the session registry must stay quiet
//! (no leaked sessions after teardown).
//!
//! Run: `cargo test -p natives-agent-daemon rpc_frame -- --test-threads=2`

use assistant_protocol::v1::daemon::{HandshakeRequest, HandshakeResponse, RpcRequest};
use assistant_protocol::v2::{MethodStatus, V2Request, V2Response};
use natives_agent_daemon::rpc::{RpcServer, MAX_FRAME_BYTES};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn temp_socket(tag: &str) -> PathBuf {
    // Keep path short — macOS unix socket path limit is ~104 bytes.
    PathBuf::from(format!(
        "/tmp/nrf-{tag}-{}.sock",
        &uuid::Uuid::new_v4().to_string()[..8]
    ))
}

async fn start_server(socket: &std::path::Path, bootstrap: &str) {
    let server = RpcServer::new(&socket.to_string_lossy(), bootstrap, "2.0.0", "0.1.0-test");
    let sock = socket.to_path_buf();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    // Wait for the listener to bind before connecting.
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn handshake_bytes(bootstrap: &str) -> Vec<u8> {
    let req = HandshakeRequest {
        client_version: "2.0.0".to_string(),
        client_id: "frame-client".to_string(),
        bootstrap_token: bootstrap.to_string(),
    };
    let mut v = serde_json::to_vec(&req).unwrap();
    v.push(b'\n');
    v
}

/// Read one newline-terminated line from a raw socket (defensive cap only).
async fn read_line_raw(stream: &mut tokio::net::UnixStream) -> String {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).await.unwrap_or(0);
        if n == 0 || byte[0] == b'\n' {
            break;
        }
        out.push(byte[0]);
        if out.len() > 64 * 1024 {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn do_handshake(stream: &mut tokio::net::UnixStream, bootstrap: &str) -> HandshakeResponse {
    stream.write_all(&handshake_bytes(bootstrap)).await.unwrap();
    let resp = read_line_raw(stream).await;
    serde_json::from_str(&resp).expect("valid handshake response")
}

#[tokio::test]
async fn rpc_frame_oversize_handshake_is_rejected_and_closed() {
    let socket = temp_socket("hs");
    let bootstrap = "bt-hs";
    start_server(&socket, bootstrap).await;
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();

    // Oversized frame with no newline must be rejected before buffering.
    let oversized = vec![b'a'; MAX_FRAME_BYTES + 1];
    stream.write_all(&oversized).await.unwrap();

    let resp = read_line_raw(&mut stream).await;
    assert!(
        resp.contains("accepted") && resp.contains("false"),
        "server must reject the oversized handshake, got: {resp}"
    );
    // The connection must be closed after the reject (bounded teardown).
    let mut byte = [0u8; 1];
    let n = stream.read(&mut byte).await.unwrap_or(0);
    assert_eq!(n, 0, "connection must close after oversized handshake");
    let _ = std::fs::remove_file(&socket);
}

#[tokio::test]
async fn rpc_frame_oversize_request_gets_error_and_closes() {
    let socket = temp_socket("rpc");
    let bootstrap = "bt-rpc";
    start_server(&socket, bootstrap).await;
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let _hs = do_handshake(&mut stream, bootstrap).await;

    // Oversized RPC frame (no newline) after a valid handshake.
    let oversized = vec![b'{'; MAX_FRAME_BYTES + 1];
    stream.write_all(&oversized).await.unwrap();

    let resp = read_line_raw(&mut stream).await;
    assert!(
        resp.contains("frame") || resp.contains("INVALID_INPUT"),
        "server must return a structured frame error, got: {resp}"
    );
    let mut byte = [0u8; 1];
    let n = stream.read(&mut byte).await.unwrap_or(0);
    assert_eq!(n, 0, "connection must close after oversized request");
    let _ = std::fs::remove_file(&socket);
}

#[tokio::test]
async fn rpc_frame_invalid_json_gets_error_and_session_survives() {
    let socket = temp_socket("json");
    let bootstrap = "bt-json";
    start_server(&socket, bootstrap).await;
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let hs = do_handshake(&mut stream, bootstrap).await;
    assert!(hs.accepted);

    // Malformed JSON frame must yield a structured error, not a crash/hang.
    stream.write_all(b"{ not json }\n").await.unwrap();
    let err_line = read_line_raw(&mut stream).await;
    assert!(
        err_line.contains("invalid_request"),
        "malformed JSON must get a structured error, got: {err_line}"
    );

    // A valid request on the same session must still be served.
    let req = V2Request {
        protocol_version: "2.0.0".to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: None,
        client_id: "frame-client".to_string(),
        session_token: hs.session_token.clone(),
        run_id: None,
        idempotency_key: None,
        method: "daemon.getStatus".to_string(),
        params: serde_json::json!({}),
    };
    let mut line = serde_json::to_vec(&req).unwrap();
    line.push(b'\n');
    stream.write_all(&line).await.unwrap();
    let resp = read_line_raw(&mut stream).await;
    assert!(
        resp.contains("success") && !resp.trim().is_empty(),
        "valid request after malformed JSON must be served, got: {resp}"
    );
    let _ = std::fs::remove_file(&socket);
}

#[tokio::test]
async fn rpc_frame_uses_v2_envelopes_and_rejects_v1_requests() {
    let socket = temp_socket("v2");
    let bootstrap = "bt-v2";
    start_server(&socket, bootstrap).await;
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let hs = do_handshake(&mut stream, bootstrap).await;

    let mut request = V2Request::new(
        "frame-client",
        hs.session_token.clone(),
        "daemon.getStatus",
        serde_json::json!({}),
    )
    .with_session_id("session-correlation")
    .with_run_id("run-correlation");
    request.request_id = "v2-success".into();
    let mut line = serde_json::to_vec(&request).unwrap();
    line.push(b'\n');
    stream.write_all(&line).await.unwrap();

    let success: V2Response = serde_json::from_str(&read_line_raw(&mut stream).await).unwrap();
    match success {
        V2Response::Success(response) => {
            assert_eq!(response.request_id, "v2-success");
        }
        V2Response::Error(error) => panic!("expected success, got {error:?}"),
    }

    let mut unknown = V2Request::new(
        "frame-client",
        hs.session_token.clone(),
        "not.a.real.method",
        serde_json::json!({}),
    );
    unknown.request_id = "v2-unknown".into();
    let mut line = serde_json::to_vec(&unknown).unwrap();
    line.push(b'\n');
    stream.write_all(&line).await.unwrap();
    let unknown: V2Response = serde_json::from_str(&read_line_raw(&mut stream).await).unwrap();
    match unknown {
        V2Response::Error(response) => {
            assert_eq!(response.request_id, "v2-unknown");
            assert_eq!(response.error.code, "unsupported");
            assert_eq!(
                response.error.method_status,
                Some(MethodStatus::Unsupported)
            );
            assert!(response.error.correlation_id.is_some());
        }
        V2Response::Success(response) => panic!("unknown method was accepted: {response:?}"),
    }

    stream
        .write_all(
            b"{\"protocol_version\":\"2.0.0\",\"request_id\":\"v2-invalid\",\"session_id\":null}\n",
        )
        .await
        .unwrap();
    let invalid: V2Response = serde_json::from_str(&read_line_raw(&mut stream).await).unwrap();
    match invalid {
        V2Response::Error(response) => {
            assert_eq!(response.request_id, "v2-invalid");
            assert_eq!(response.error.code, "invalid_request");
            assert_eq!(
                response.error.method_status,
                Some(MethodStatus::InvalidRequest)
            );
            assert!(response.error.correlation_id.is_some());
        }
        V2Response::Success(response) => panic!("invalid V2 request was accepted: {response:?}"),
    }

    let legacy = RpcRequest {
        protocol_version: "2.0.0".to_string(),
        request_id: "legacy-request".to_string(),
        client_id: "frame-client".to_string(),
        session_token: hs.session_token,
        method: "daemon.getStatus".to_string(),
        params: serde_json::json!({}),
    };
    let mut line = serde_json::to_vec(&legacy).unwrap();
    line.push(b'\n');
    stream.write_all(&line).await.unwrap();

    let legacy_response: V2Response =
        serde_json::from_str(&read_line_raw(&mut stream).await).unwrap();
    match legacy_response {
        V2Response::Error(response) => {
            assert_eq!(response.request_id, "legacy-request");
            assert_eq!(response.error.code, "invalid_request");
            assert_eq!(
                response.error.method_status,
                Some(MethodStatus::InvalidRequest)
            );
            assert!(response.error.correlation_id.is_some());
        }
        V2Response::Success(response) => panic!("v1 request was accepted: {response:?}"),
    }
    let _ = std::fs::remove_file(&socket);
}

#[tokio::test]
async fn rpc_frame_client_drop_keeps_registry_quiet() {
    let socket = temp_socket("drop");
    let bootstrap = "bt-drop";
    start_server(&socket, bootstrap).await;

    // First client: handshake then drop the socket abruptly.
    {
        let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
        let _hs = do_handshake(&mut stream, bootstrap).await;
        // Drop closes the connection; the daemon must reap the session.
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    // A second client with the same id must handshake and call successfully,
    // proving the dropped session was cleaned and nothing is blocked.
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let hs = do_handshake(&mut stream, bootstrap).await;
    assert!(hs.accepted);
    let req = V2Request {
        protocol_version: "2.0.0".to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: None,
        client_id: "frame-client".to_string(),
        session_token: hs.session_token.clone(),
        run_id: None,
        idempotency_key: None,
        method: "daemon.getStatus".to_string(),
        params: serde_json::json!({}),
    };
    let mut line = serde_json::to_vec(&req).unwrap();
    line.push(b'\n');
    stream.write_all(&line).await.unwrap();
    let resp = read_line_raw(&mut stream).await;
    assert!(
        resp.contains("success"),
        "reconnect after client drop must be served, got: {resp}"
    );
    let _ = std::fs::remove_file(&socket);
}
