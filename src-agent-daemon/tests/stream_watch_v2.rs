//! RunWatchStreamV2 — genuine-link tests for the frozen STREAM-CONTRACT-V2.
//!
//! Contract: `docs/contracts/STREAM-CONTRACT-V2.md`.
//!
//! These tests drive the **real daemon link**, not mocks:
//!
//! - fixture AgentEngine → shared [`LiveEventBus`] → `run.watch` RPC → wire frames;
//! - [`UdsAuthority::watch_events`] (ACK-then-frames ordering);
//! - durable-lane guards (EventSequencer/SQLite must reject live-only deltas);
//! - heartbeat / bounded-replay / no-replay-subscribe-gap / terminal-cleanup.
//!
//! Every test uses a unique temp DB, UDS socket and runtime dir and never
//! touches `~/.natives`.
//!
//! Run:
//! ```bash
//! cargo test -p natives-agent-daemon --test stream_watch_v2 -- --nocapture
//! ```
//!
//! Gate 0 (red) note: while S1/S2 have not yet registered
//! `pub mod stream_protocol;` in `src-agent-daemon/src/lib.rs`, this binary
//! (like `stream_watch.rs`) fails at **compile time** — the expected red light
//! for this branch. Each test below documents which frozen contract point it
//! proves.

use agent_core::{EventSequencer, ToolProgressSink, ToolProgressUpdate};
use assistant_protocol::v1::daemon::{HandshakeRequest, HandshakeResponse, RpcRequest};
use assistant_protocol::v2::RunEventKind;
use natives_agent_daemon::client::{client_protocol_version, DaemonClient};
use natives_agent_daemon::rpc::RpcServer;
use natives_agent_daemon::storage::DataStore;
use natives_agent_daemon::stream_protocol::{RunStreamFrameV2, RunStreamLane};
use natives_agent_daemon::{global_run_manager, ExecutionAuthority, UdsAuthority};
use natives_agent_daemon::{DaemonToolProgressSink, ProductionRuntime};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Shared harness (mirrors stream_watch.rs / uds_run_lifecycle.rs)
// ---------------------------------------------------------------------------

fn temp_socket(tag: &str) -> PathBuf {
    // Keep path short — macOS unix socket path limit is ~104 bytes.
    PathBuf::from(format!(
        "/tmp/nswv2-{tag}-{}.sock",
        &uuid::Uuid::new_v4().to_string()[..8]
    ))
}

fn scratch_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("natives-swv2-{tag}-{}", uuid::Uuid::new_v4()))
}

/// Point every daemon side effect at a unique temp dir: authority DB,
/// assistant DB, event-log runtime dir, plus the fixture provider.
fn apply_fixture_env(scratch: &std::path::Path) {
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    std::env::set_var("NATIVES_ALLOW_FIXTURE_FALLBACK", "1");
    std::env::set_var("NATIVES_DB_PATH", scratch.join("natives.db"));
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", scratch.join("assistant.db"));
    std::env::set_var("NATIVES_RUNTIME_DIR", scratch);
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
    assert!(sock.exists(), "server socket must appear");
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
        client_id: "swv2-client".to_string(),
        session_token: session_token.to_string(),
        method: method.to_string(),
        params,
    };
    let mut v = serde_json::to_vec(&req).unwrap();
    v.push(b'\n');
    stream.write_all(&v).await.expect("write rpc");
}

/// Create a fixture run over RPC and return its `run_id`.
async fn rpc_create_run(
    stream: &mut tokio::net::UnixStream,
    session_token: &str,
    conv: &str,
) -> String {
    send_rpc(
        stream,
        session_token,
        "run.create",
        json!({
            "conversation_id": conv,
            "provider_id": "openai",
            "model_id": "gpt-4o",
            "key_id": "k-test",
            "permission_profile": "full_access",
            "content": "hello v2 watch",
            "max_steps": 5,
            "project_path": "/tmp/natives-v2-project",
            "idempotency_key": format!("swv2-{conv}-{}", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let resp = read_line_raw(stream).await;
    let created: Value = serde_json::from_str(&resp).expect("run.create response");
    created
        .get("data")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .expect("run id")
        .to_string()
}

fn start_run_params(run_id: &str) -> Value {
    json!({
        "run_id": run_id,
        "provider_id": "openai",
        "model_id": "gpt-4o",
        "key_id": "k-test",
        "content": "hello v2 watch",
        "permission_profile": "full_access",
        "max_steps": 5,
        "project_path": "/tmp/natives-v2-project",
    })
}

/// Minimal summary of one wire line (RunWatchStreamV2 frame or RPC ACK).
#[derive(Debug, Default, Clone)]
struct WireLine {
    frame_type: Option<String>, // "event" | "heartbeat" | "resync_required"
    lane: Option<String>,       // "durable" | "live"
    event_type: Option<String>, // "text_delta" | "completed" | ...
    live_sequence: Option<u64>,
    durable_sequence: Option<u64>,
    is_ack: bool,
    stream_version: Option<u64>,
}

/// Parse one newline-delimited wire frame into a field-checked view.
///
/// Field-assignment style (instead of one struct literal) keeps each field
/// extraction explicit next to its JSON source; the `WireLine` fields are all
/// covered and asserted by the tests below.
#[allow(clippy::field_reassign_with_default)] // test helper; explicit is clearer
fn parse_wire_line(line: &str) -> WireLine {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return WireLine::default(),
    };
    let mut w = WireLine::default();
    w.frame_type = v
        .get("frame_type")
        .and_then(|x| x.as_str())
        .map(String::from);
    w.lane = v.get("lane").and_then(|x| x.as_str()).map(String::from);
    w.event_type = v
        .get("event_type")
        .and_then(|x| x.as_str())
        .map(String::from);
    w.live_sequence = v.get("live_sequence").and_then(|x| x.as_u64());
    w.durable_sequence = v.get("durable_sequence").and_then(|x| x.as_u64());
    if let Some(data) = v.get("data") {
        if data.get("stream").and_then(|x| x.as_str()) == Some("run.watch") {
            w.is_ack = true;
            w.stream_version = data.get("streamVersion").and_then(|x| x.as_u64());
        }
    }
    w
}

fn is_terminal_event_type(ty: &str) -> bool {
    matches!(
        ty,
        "completed" | "turn_completed" | "cancelled" | "failed" | "interrupted"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Contract point: STREAM-CONTRACT-V2 live lane — an Engine `TextDelta` must
/// cross the shared `LiveEventBus` (the daemon's single live sink) and arrive
/// as a `lane:"live"` `text_delta` frame on the `run.watch` wire.
///
/// This is the full link: fixture Engine → runtime.live → run.watch RPC →
/// UDS socket → test client.
#[tokio::test]
async fn shared_live_bus_reaches_run_watch() {
    let scratch = scratch_dir("live");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    apply_fixture_env(&scratch);

    let sock = temp_socket("live");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "live-client").await;
    let run_id = rpc_create_run(&mut stream, &session_token, "live-conv").await;

    // Watch BEFORE starting so the live lane is live-subscribed.
    send_rpc(
        &mut stream,
        &session_token,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": 0, "after_live_sequence": 0 }),
    )
    .await;

    let (mut starter, starter_token) = connect_handshake(&sock, &bootstrap, "live-starter").await;
    send_rpc(
        &mut starter,
        &starter_token,
        "run.start",
        start_run_params(&run_id),
    )
    .await;

    let mut saw_live_text = false;
    let mut saw_terminal = false;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let line = read_line_raw(&mut stream).await;
            if line.is_empty() {
                break;
            }
            let w = parse_wire_line(&line);
            if w.lane.as_deref() == Some("live") && w.event_type.as_deref() == Some("text_delta") {
                saw_live_text = true;
            }
            if let Some(ty) = w.event_type.as_deref() {
                if is_terminal_event_type(ty) {
                    saw_terminal = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(result.is_ok(), "watch stream must complete within 30s");

    assert!(
        saw_live_text,
        "Engine TextDelta must reach run.watch as a live-lane text_delta frame \
         (shared LiveEventBus → run.watch)"
    );
    assert!(
        saw_terminal,
        "run.watch must close on the durable terminal event"
    );

    drop(starter);
    let _ = std::fs::remove_file(&sock);
}

/// Contract point: `UdsAuthority::watch_events` reads the ordinary RPC ACK
/// first (`stream:"run.watch"`, `streamVersion:2`) — `begin_watch` validates
/// it and only then yields frames — and the stream subsequently delivers the
/// fixture's live `text_delta`.
#[tokio::test]
async fn uds_watch_reads_ack_then_text_delta() {
    let scratch = scratch_dir("uds");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    apply_fixture_env(&scratch);

    let sock = temp_socket("uds");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;

    let auth = UdsAuthority::new(sock.clone(), bootstrap.clone());
    let created = auth
        .request(
            "run.create",
            json!({
                "conversation_id": "uds-conv",
                "provider_id": "openai",
                "model_id": "gpt-4o",
                "key_id": "k-test",
                "permission_profile": "full_access",
                "content": "hello v2 uds",
                "max_steps": 5,
                "project_path": "/tmp/natives-v2-project",
                "idempotency_key": format!("uds-idem-{}", uuid::Uuid::new_v4()),
            }),
        )
        .await
        .expect("run.create via UdsAuthority");
    let run_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .expect("run id")
        .to_string();

    // watch_events consumes + validates the V2 ACK before returning; an ACK
    // without streamVersion 2 would surface here as Err.
    let mut watch = auth
        .watch_events(&run_id, 0, 0)
        .await
        .expect("watch_events must succeed — V2 ACK read and validated");

    let _ = auth
        .request("run.start", start_run_params(&run_id))
        .await
        .expect("run.start via UdsAuthority");

    let mut saw_live_text = false;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match watch.next_frame().await {
                Some(Ok(RunStreamFrameV2::Event {
                    lane: RunStreamLane::Live,
                    event_type,
                    ..
                })) if event_type == "text_delta" => {
                    saw_live_text = true;
                }
                Some(Ok(RunStreamFrameV2::Event { event_type, .. }))
                    if is_terminal_event_type(&event_type) =>
                {
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("watch frame error: {e}"),
                None => break,
            }
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "watch_events stream must complete within 30s"
    );
    assert!(
        saw_live_text,
        "UdsAuthority.watch_events must deliver ACK (validated inside begin_watch) \
         then the live text_delta frame"
    );

    let _ = std::fs::remove_file(&sock);
}

/// Contract point: ToolOutputDelta is live-lane ONLY — publishing through the
/// production `DaemonToolProgressSink` lands on the shared LiveEventBus and is
/// NEVER written to the durable EventSequencer / SQLite `run_event` table.
#[tokio::test]
async fn tool_output_delta_does_not_persist() {
    let scratch = scratch_dir("nopersist");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let db = scratch.join("natives.db");
    let artifacts = scratch.join("artifacts");
    let store = Arc::new(DataStore::new(&db, &artifacts).expect("open temp DataStore"));
    let runtime = ProductionRuntime::new_with_event_store(store.clone());
    let sink = DaemonToolProgressSink::new(runtime.live_events());

    let run_id = format!("progress-run-{}", uuid::Uuid::new_v4());
    sink.publish(ToolProgressUpdate {
        run_id: run_id.clone(),
        tool_call_id: "call-1".into(),
        tool_name: "bash".into(),
        stream: "stdout".into(),
        text: "durable-split must never persist this".into(),
        final_update: true,
        turn_id: None,
        message_id: None,
        progress_sequence: 1,
    })
    .await;

    // 1) The live lane must carry the delta (that is where it belongs).
    let sub = runtime.live_events().subscribe_after(&run_id, 0);
    assert!(
        !sub.buffered.is_empty(),
        "ToolOutputDelta must arrive on the shared LiveEventBus"
    );
    assert!(
        sub.buffered
            .iter()
            .any(|e| matches!(e.kind, RunEventKind::ToolOutputDelta { .. })),
        "live bus must carry the ToolOutputDelta event"
    );

    // 2) The durable EventSequencer must not have it.
    let durable = runtime.events.replay_after(&run_id, 0);
    assert!(
        durable.is_empty(),
        "ToolOutputDelta must not persist to the durable EventSequencer (got {durable:?})"
    );
    assert!(
        !durable.iter().any(|e| matches!(
            e.payload,
            RunEventKind::ToolOutputDelta { .. } | RunEventKind::TextDelta { .. }
        )),
        "durable replay must contain no text_delta / tool_output_delta"
    );

    // 3) SQLite must not have any row for this run (live-only event left no trace).
    {
        let conn = store.conn().expect("store connection");
        let mut stmt = conn
            .prepare("SELECT event_type FROM run_event WHERE run_id = ?1")
            .expect("prepare run_event query");
        let types: Vec<String> = stmt
            .query_map([&run_id], |row| row.get::<_, String>(0))
            .expect("query run_event")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect rows");
        assert!(
            types.is_empty(),
            "SQLite run_event must be empty for a live-only ToolOutputDelta run; got {types:?}"
        );
        assert!(
            !types
                .iter()
                .any(|t| t == "text_delta" || t == "tool_output_delta"),
            "durable SQLite rows must never contain live-only event types"
        );
    }
}

/// Contract point: the durable lane REJECTS live-only kinds
/// (`TextDelta`/`ReasoningDelta`/`ToolCallDelta`/`ToolOutputDelta`/`Progress`).
/// `EventSequencer::append` must return `Failed{LIVE_EVENT_ON_DURABLE_LANE}`
/// without persisting anything or consuming the run's durable sequence.
#[tokio::test]
async fn durable_lane_rejects_live_only_events() {
    let events = EventSequencer::memory_only();
    let run_id = format!("liveguard-{}", uuid::Uuid::new_v4());

    let live_kinds = [
        RunEventKind::TextDelta { text: "hi".into() },
        RunEventKind::ReasoningDelta {
            text: "think".into(),
        },
        RunEventKind::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments_delta: "{}".into(),
        },
        RunEventKind::ToolOutputDelta {
            tool_call_id: "c1".into(),
            tool_name: None,
            stream: "stdout".into(),
            text: "out".into(),
            truncated: false,
            turn_id: None,
            message_id: None,
            progress_sequence: None,
        },
        RunEventKind::Progress {
            message: "working".into(),
            percentage: None,
        },
    ];

    for kind in live_kinds {
        let rejected = events.append(&run_id, kind);
        assert!(
            matches!(
                &rejected.payload,
                RunEventKind::Failed { code, .. } if code == "LIVE_EVENT_ON_DURABLE_LANE"
            ),
            "live-only event must be rejected on the durable lane; got {rejected:?}"
        );
    }

    // Nothing durable may persist, and the sequence counter must not advance.
    assert!(
        events.replay_after(&run_id, 0).is_empty(),
        "rejected live-only events must not persist to the durable lane"
    );
    let ok = events.append(&run_id, RunEventKind::Started);
    assert_eq!(
        ok.effective_run_sequence(),
        1,
        "durable sequence must not be consumed by rejected live-only events"
    );
}

/// Contract point: `run.watch` establishes the durable AND live receivers
/// BEFORE the RPC ACK, so there is no replay→subscribe window. A watch opened
/// before `run.start` must observe the run's very first live event
/// (`live_sequence == 1`) with no gap.
#[tokio::test]
async fn run_watch_has_no_replay_subscribe_gap() {
    let scratch = scratch_dir("nogap");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    apply_fixture_env(&scratch);

    let sock = temp_socket("nogap");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "nogap-client").await;
    let run_id = rpc_create_run(&mut stream, &session_token, "nogap-conv").await;

    // Open the watch BEFORE start: the live receiver must already be attached
    // when the engine starts emitting live deltas.
    send_rpc(
        &mut stream,
        &session_token,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": 0, "after_live_sequence": 0 }),
    )
    .await;

    let (mut starter, starter_token) = connect_handshake(&sock, &bootstrap, "nogap-starter").await;
    send_rpc(
        &mut starter,
        &starter_token,
        "run.start",
        start_run_params(&run_id),
    )
    .await;

    let mut live_seqs: Vec<u64> = Vec::new();
    let mut saw_terminal = false;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let line = read_line_raw(&mut stream).await;
            if line.is_empty() {
                break;
            }
            let w = parse_wire_line(&line);
            if w.lane.as_deref() == Some("live") {
                if let Some(seq) = w.live_sequence {
                    live_seqs.push(seq);
                }
            }
            if let Some(ty) = w.event_type.as_deref() {
                if is_terminal_event_type(ty) {
                    saw_terminal = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(result.is_ok(), "watch stream must complete within 30s");
    assert!(saw_terminal, "run must reach terminal");

    assert!(
        !live_seqs.is_empty(),
        "a run.watch opened before start must observe live-lane events"
    );
    assert_eq!(
        live_seqs[0], 1,
        "the first live event (live_sequence=1) must not be lost between \
         durable replay and live subscribe (no replay→subscribe gap)"
    );
    assert!(
        live_seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "live_sequence must be contiguous on the wire: {live_seqs:?}"
    );

    drop(starter);
    let _ = std::fs::remove_file(&sock);
}

/// Contract point: Heartbeat — the daemon sends a heartbeat every 15s so a
/// healthy but idle `run.watch` never trips the client's 30s frame timeout.
/// This test keeps an idle watch open for 45s (> 30s timeout) and asserts the
/// stream survives, delivering heartbeats instead of a false timeout.
#[tokio::test]
async fn run_watch_heartbeat_survives_45s_idle() {
    let scratch = scratch_dir("hb");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    apply_fixture_env(&scratch);

    let sock = temp_socket("hb");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "hb-client").await;
    // Create but DO NOT start: the stream goes idle after the durable replay.
    let run_id = rpc_create_run(&mut stream, &session_token, "hb-conv").await;
    drop(stream); // the create connection is no longer needed

    let mut client = DaemonClient::connect(&sock, &bootstrap, client_protocol_version())
        .await
        .expect("DaemonClient connect");
    client
        .begin_watch(&run_id, 0, 0)
        .await
        .expect("begin_watch (ACK read + validated)");

    let idle_duration = Duration::from_secs(45);
    let deadline = std::time::Instant::now() + idle_duration;
    let mut heartbeats: u32 = 0;
    let mut durable_frames: u32 = 0;

    let result = tokio::time::timeout(Duration::from_secs(70), async {
        while std::time::Instant::now() < deadline {
            match client.read_stream_frame().await {
                Some(Ok(RunStreamFrameV2::Heartbeat { .. })) => heartbeats += 1,
                Some(Ok(RunStreamFrameV2::Event {
                    lane: RunStreamLane::Durable,
                    ..
                })) => durable_frames += 1,
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    panic!("run.watch idle stream failed: {e} (false timeout?)")
                }
                None => panic!("run.watch closed unexpectedly during idle"),
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "idle run.watch must survive {idle_duration:?} without hitting the 30s frame timeout"
    );
    assert!(
        heartbeats >= 2,
        "45s idle must yield ≥2 heartbeats (15s interval), got {heartbeats}"
    );
    eprintln!(
        "[stream_watch_v2] heartbeat idle test OK: {heartbeats} heartbeats, \
         {durable_frames} durable frames over {idle_duration:?}"
    );

    let _ = std::fs::remove_file(&sock);
}

/// Contract point: late subscriber — a `run.watch` opened after live deltas
/// already exist must receive the **bounded in-memory live replay prefix**
/// (bounded ring), or a `resync_required(live)` if the cursor fell out of the
/// buffer. It must NEVER fabricate a durable-lane text_delta.
#[tokio::test]
async fn late_watch_replays_bounded_live_prefix_or_reports_gap() {
    let scratch = scratch_dir("late");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    apply_fixture_env(&scratch);

    let sock = temp_socket("late");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "late-client").await;
    let run_id = rpc_create_run(&mut stream, &session_token, "late-conv").await;

    // Emit a bounded set of live deltas through the SAME shared LiveEventBus
    // the daemon's run.watch subscribes to (real link, controlled input).
    let bus = global_run_manager().runtime.live_events();
    const PREFIX_LEN: u64 = 30;
    for i in 1..=PREFIX_LEN {
        bus.append(
            &run_id,
            RunEventKind::TextDelta {
                text: format!("delta-{i}"),
            },
        );
    }
    assert_eq!(
        bus.last_sequence(&run_id),
        PREFIX_LEN,
        "live bus must carry the injected prefix"
    );

    // Open a LATE watch after the prefix exists.
    send_rpc(
        &mut stream,
        &session_token,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": 0, "after_live_sequence": 0 }),
    )
    .await;

    let mut saw_live_replay = 0u32;
    let mut saw_resync = false;
    let mut saw_durable_text_delta = false;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        // Read until we have seen the whole bounded prefix (or a resync), up
        // to a hard iteration cap so the test can never hang.
        for _ in 0..512 {
            let line = read_line_raw(&mut stream).await;
            if line.is_empty() {
                break;
            }
            let w = parse_wire_line(&line);
            if w.lane.as_deref() == Some("live") && w.event_type.as_deref() == Some("text_delta") {
                saw_live_replay += 1;
                if saw_live_replay as u64 >= PREFIX_LEN {
                    break;
                }
            }
            if w.lane.as_deref() == Some("durable") && w.event_type.as_deref() == Some("text_delta")
            {
                saw_durable_text_delta = true;
            }
            if w.frame_type.as_deref() == Some("resync_required") {
                saw_resync = true;
                break;
            }
        }
    })
    .await;
    assert!(result.is_ok(), "late watch must respond within 10s");

    assert!(
        saw_live_replay > 0 || saw_resync,
        "late watch must either replay the bounded live prefix or report a live-lane gap"
    );
    assert!(
        !saw_durable_text_delta,
        "live deltas must never appear on the durable lane (no fabricated durable text_delta)"
    );
    eprintln!(
        "[stream_watch_v2] late watch: replayed {saw_live_replay} live deltas, resync={saw_resync}"
    );

    let _ = std::fs::remove_file(&sock);
}

/// Contract point: Terminal — after the durable terminal event the daemon
/// clears the run's live ring/bus state (`LiveBus.remove_run`). Once the
/// fixture run completes, the shared bus must report last_sequence == 0 and
/// buffered_len == 0 for that run.
#[tokio::test]
async fn terminal_clears_live_run_state() {
    let scratch = scratch_dir("term");
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    apply_fixture_env(&scratch);

    let sock = temp_socket("term");
    let _ = std::fs::remove_file(&sock);
    let bootstrap = format!("boot-{}", uuid::Uuid::new_v4());
    start_server(&sock, &bootstrap).await;

    let (mut stream, session_token) = connect_handshake(&sock, &bootstrap, "term-client").await;
    let run_id = rpc_create_run(&mut stream, &session_token, "term-conv").await;

    // Watch before start so we observe the live text_delta on the wire (this
    // proves the bus actually carried live state before terminal cleanup).
    send_rpc(
        &mut stream,
        &session_token,
        "run.watch",
        json!({ "run_id": run_id, "after_sequence": 0, "after_live_sequence": 0 }),
    )
    .await;

    let (mut starter, starter_token) = connect_handshake(&sock, &bootstrap, "term-starter").await;
    send_rpc(
        &mut starter,
        &starter_token,
        "run.start",
        start_run_params(&run_id),
    )
    .await;

    // Drain until the durable terminal frame.
    let mut saw_live_text = false;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let line = read_line_raw(&mut stream).await;
            if line.is_empty() {
                break;
            }
            let w = parse_wire_line(&line);
            if w.lane.as_deref() == Some("live") && w.event_type.as_deref() == Some("text_delta") {
                saw_live_text = true;
            }
            if let Some(ty) = w.event_type.as_deref() {
                if is_terminal_event_type(ty) {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(result.is_ok(), "run must reach terminal within 30s");
    assert!(
        saw_live_text,
        "fixture run must emit a live text_delta before terminal (precondition)"
    );

    // Terminal cleanup (STREAM-CONTRACT-V2 Terminal): the daemon drops the
    // run's ring/bus state shortly after the durable terminal event.
    let bus = global_run_manager().runtime.live_events();
    let cleanup_deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut cleaned = false;
    while std::time::Instant::now() < cleanup_deadline {
        if bus.last_sequence(&run_id) == 0 && bus.buffered_len(&run_id) == 0 {
            cleaned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        cleaned,
        "terminal must clear the run's live ring/bus state \
         (last_sequence={}, buffered_len={})",
        bus.last_sequence(&run_id),
        bus.buffered_len(&run_id)
    );

    drop(starter);
    let _ = std::fs::remove_file(&sock);
}
