//! A2 — Persistent event stream (`run.watch`) tests.
//!
//! Verifies the frozen stream contract (`.contracts/stream-contract-v1.md`):
//! - `run.watch` replays durable events > after_sequence, then continuously
//!   pushes new events on the same connection (no fixed poll sleep).
//! - `after_sequence` reconnect never re-sends already-seen durable events.
//! - terminal event cleanly closes the stream.
//!
//! Run: `cargo test -p natives-agent-daemon --test stream_watch -- --nocapture`

use assistant_protocol::v1::daemon::{HandshakeRequest, HandshakeResponse, RpcRequest};
use natives_agent_daemon::rpc::RpcServer;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn temp_socket(tag: &str) -> PathBuf {
    PathBuf::from(format!(
        "/tmp/nsw-{tag}-{}.sock",
        &uuid::Uuid::new_v4().to_string()[..8]
    ))
}

async fn start_server(socket: &std::path::Path, bootstrap: &str) {
    let server = RpcServer::new(&socket.to_string_lossy(), bootstrap, "2.0.0", "0.1.0-test");
    let sock = socket.to_path_buf();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn connect_handshake(
    socket: &std::path::Path,
    bootstrap: &str,
    client_id: &str,
) -> (tokio::net::UnixStream, String) {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("connect to daemon socket");
    let req = HandshakeRequest {
        client_version: "2.0.0".to_string(),
        client_id: client_id.to_string(),
        bootstrap_token: bootstrap.to_string(),
    };
    let mut v = serde_json::to_vec(&req).unwrap();
    v.push(b'\n');
    stream.write_all(&v).await.expect("write handshake");
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).await.expect("read handshake");
        if n == 0 || byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    let resp: HandshakeResponse = serde_json::from_slice(&buf).expect("handshake response");
    assert!(resp.accepted, "handshake must be accepted: {resp:?}");
    (stream, resp.session_token.clone())
}

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
    String::from_utf8_lossy(&out).to_string()
}

async fn send_rpc(
    stream: &mut tokio::net::UnixStream,
    session_token: &str,
    method: &str,
    params: Value,
) {
    let req = RpcRequest {
        protocol_version: "2.0.0".to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        client_id: "stream-client".to_string(),
        session_token: session_token.to_string(),
        method: method.to_string(),
        params,
    };
    let mut v = serde_json::to_vec(&req).unwrap();
    v.push(b'\n');
    stream.write_all(&v).await.expect("write rpc");
}

/// `run.watch` streams new events on a persistent connection and closes on
/// terminal. Uses the fixture engine (NATIVES_DAEMON_FIXTURE=1) so no network
/// provider is touched.
#[tokio::test]
async fn stream_watch_pushes_new_events_until_terminal() {
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    std::env::set_var("NATIVES_ALLOW_FIXTURE_FALLBACK", "1");
    let scratch = std::env::temp_dir().join(format!("natives-sw-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    std::env::set_var("NATIVES_DB_PATH", scratch.join("natives.db"));
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", scratch.join("assistant.db"));
    std::env::set_var("NATIVES_RUNTIME_DIR", &scratch);

    let sock = temp_socket("watch");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;
    assert!(sock.exists(), "server socket must appear");

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "watch-client").await;

    // create + start a fixture run (mirrors uds_run_lifecycle).
    send_rpc(
        &mut stream,
        &session_token,
        "run.create",
        json!({
            "conversation_id": "watch-conv",
            "provider_id": "openai",
            "model_id": "gpt-4o",
            "key_id": "k-test",
            "permission_profile": "full_access",
            "content": "hello watch",
            "max_steps": 5,
            "project_path": "/tmp/natives-watch-project",
            "idempotency_key": format!("watch-idem-{}", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let create_resp = read_line_raw(&mut stream).await;
    eprintln!("[stream_watch] run.create raw response: {create_resp}");
    let created: Value = serde_json::from_str(&create_resp).expect("run.create response");
    let run_id = created
        .get("data")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .expect("run id")
        .to_string();

    // Watch with after_sequence=0 BEFORE starting so we observe live push.
    send_rpc(
        &mut stream,
        &session_token,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": 0 }),
    )
    .await;

    // Start on a second connection so run.watch keeps streaming.
    let (mut starter, starter_token) = connect_handshake(&sock, &bootstrap, "starter-client").await;
    send_rpc(
        &mut starter,
        &starter_token,
        "run.start",
        json!({
            "run_id": run_id,
            "provider_id": "openai",
            "model_id": "gpt-4o",
            "key_id": "k-test",
            "content": "hello watch",
            "permission_profile": "full_access",
            "max_steps": 5,
            "project_path": "/tmp/natives-watch-project",
        }),
    )
    .await;

    // Drain the watch stream until terminal (Completed/TurnCompleted/Cancelled).
    let mut saw_durable_event = false;
    let mut saw_terminal = false;
    let mut saw_text = false;
    for _ in 0..200 {
        let line = read_line_raw(&mut stream).await;
        if line.is_empty() {
            break; // connection closed
        }
        let env: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // e.g. a subscribe-style success wrapper
        };
        // Newline-delimited V2EventEnvelope: has event_type + payload.
        let event_type = env.get("event_type").and_then(|v| v.as_str());
        if let Some(ty) = event_type {
            saw_durable_event = true;
            if ty == "text_delta" {
                saw_text = true;
            }
            if matches!(ty, "completed" | "turn_completed" | "cancelled" | "failed") {
                saw_terminal = true;
                break;
            }
        }
        if saw_terminal {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert!(
        saw_durable_event,
        "run.watch must stream at least one durable event envelope"
    );
    assert!(saw_terminal, "run.watch must close after a terminal event");
    // STREAM-CONTRACT-V2 (frozen): under the V2 run.watch stream the live lane
    // carries text_delta emitted by the Engine via the shared LiveEventBus.
    // The fixture provider (TextOnly) emits exactly one TextDelta before
    // Completed, so a correct V2 daemon MUST deliver it on the live lane.
    // Gate 0 red test: this assertion is the first-order check that the
    // daemon's run.watch live lane genuinely reaches the client.
    assert!(
        saw_text,
        "V2 run.watch live lane must deliver the fixture's text_delta \
         (STREAM-CONTRACT-V2 live lane); saw durable events but no text_delta frame"
    );
    drop(starter);

    let _ = std::fs::remove_file(&sock);
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");
}

/// `run.watch` with after_sequence>0 must not re-send already-seen durable
/// events (gap-free reconnect semantics).
#[tokio::test]
async fn stream_watch_after_sequence_skips_old_events() {
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    std::env::set_var("NATIVES_ALLOW_FIXTURE_FALLBACK", "1");
    let scratch = std::env::temp_dir().join(format!("natives-swa-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    std::env::set_var("NATIVES_DB_PATH", scratch.join("natives.db"));
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", scratch.join("assistant.db"));
    std::env::set_var("NATIVES_RUNTIME_DIR", &scratch);

    let sock = temp_socket("after");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;
    assert!(sock.exists(), "server socket must appear");

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "after-client").await;

    // First: replay everything to learn the terminal sequence, then reconnect
    // with that sequence and assert NO replayed old events are returned.
    send_rpc(
        &mut stream,
        &session_token,
        "run.create",
        json!({
            "conversation_id": "after-conv",
            "provider_id": "openai",
            "model_id": "gpt-4o",
            "key_id": "k-test",
            "permission_profile": "full_access",
            "content": "after watch",
            "max_steps": 5,
            "project_path": "/tmp/natives-after-project",
            "idempotency_key": format!("after-idem-{}", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let create_resp = read_line_raw(&mut stream).await;
    eprintln!("[stream_watch] run.create raw response: {create_resp}");
    let created: Value = serde_json::from_str(&create_resp).expect("run.create response");
    let run_id = created
        .get("data")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .expect("run id")
        .to_string();

    let (mut starter, starter_token) =
        connect_handshake(&sock, &bootstrap, "starter2-client").await;
    send_rpc(
        &mut starter,
        &starter_token,
        "run.start",
        json!({
            "run_id": run_id,
            "provider_id": "openai",
            "model_id": "gpt-4o",
            "key_id": "k-test",
            "content": "after watch",
            "permission_profile": "full_access",
            "max_steps": 5,
            "project_path": "/tmp/natives-after-project",
        }),
    )
    .await;

    // Drain the first watch to capture the max sequence seen.
    send_rpc(
        &mut stream,
        &session_token,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": 0 }),
    )
    .await;
    let mut max_seq = 0u64;
    let mut terminal = false;
    for _ in 0..200 {
        let line = read_line_raw(&mut stream).await;
        if line.is_empty() {
            break;
        }
        let env: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(seq) = env.get("durable_sequence").and_then(|v| v.as_u64()) {
            max_seq = max_seq.max(seq);
        }
        if let Some(ty) = env.get("event_type").and_then(|v| v.as_str()) {
            if matches!(ty, "completed" | "turn_completed" | "cancelled" | "failed") {
                terminal = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(terminal, "first watch must reach terminal");
    assert!(max_seq > 0, "first watch must observe sequences");

    // Reconnect with after_sequence = max_seq: no durable event with
    // sequence <= max_seq may be replayed.
    let (mut stream2, token2) = connect_handshake(&sock, &bootstrap, "after-client-2").await;
    send_rpc(
        &mut stream2,
        &token2,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": max_seq }),
    )
    .await;
    let mut replay_below = 0u64;
    let mut terminal2 = false;
    for _ in 0..100 {
        let line = read_line_raw(&mut stream2).await;
        if line.is_empty() {
            break;
        }
        let env: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(seq) = env.get("durable_sequence").and_then(|v| v.as_u64()) {
            if seq <= max_seq {
                replay_below += 1;
            }
        }
        if let Some(ty) = env.get("event_type").and_then(|v| v.as_str()) {
            if matches!(ty, "completed" | "turn_completed" | "cancelled" | "failed") {
                terminal2 = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        replay_below == 0,
        "after_sequence={max_seq} must not replay {replay_below} already-seen durable events"
    );
    let _ = terminal2;
    drop(starter);

    let _ = std::fs::remove_file(&sock);
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");
}
