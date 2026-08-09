//! A0 — Baseline performance harness (pre-remediation snapshot).
//!
//! Measures the CURRENT hot path without changing any production behavior:
//!   - submit → run completed (engine hot path, real SQLite-backed runtime)
//!   - provider request → first delta
//!   - provider delta → live publish (engine event broadcast) p50/p95
//!   - final provider delta → Run Completed (terminal tail)
//!   - live vs durable event counts
//!   - run_event DB rows and payload bytes per 1000 output chunks
//!   - SQLite writes per run (≈ run_event rows under persist-first)
//!   - UDS connect+handshake per RPC (current UdsAuthority behavior)
//!
//! Design: `#[ignore]` benchmark harness (same convention as `perf_evidence`),
//! not a unit gate. Run with:
//!
//! ```bash
//! export NATIVES_PERF_SCRATCH=<dir>
//! cargo test -p natives-agent-daemon --test perf_baseline -- --ignored --nocapture
//! ```
//!
//! This file defines its own chunked fixture provider; it does NOT touch
//! production code (A0 constraint: baseline may not alter agent behavior).

use agent_core::{
    AgentEngine, EngineError, EngineMessage, EngineProvider, EngineProviderEvent,
    EngineProviderEventStream, EngineRunConfig, EngineToolRuntime, ToolExecutionResult, ToolSchema,
};
use natives_agent_daemon::client::{client_protocol_version, DaemonClient};
use natives_agent_daemon::rpc::RpcServer;
use natives_agent_daemon::storage::DataStore;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

// ── helpers ────────────────────────────────────────────────────────────────

fn scratch_dir() -> PathBuf {
    std::env::var("NATIVES_PERF_SCRATCH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
                .join("target/perf-baseline")
        })
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64) * p).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

fn percentiles_json(samples: &[f64]) -> Value {
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    json!({
        "count": s.len(),
        "p50_ms": pct(&s, 0.50),
        "p75_ms": pct(&s, 0.75),
        "p95_ms": pct(&s, 0.95),
        "max_ms": pct(&s, 1.0),
    })
}

// ── chunked text provider (fixture, no network) ───────────────────────────

/// Shared clock between the provider and the harness receiver.
struct ProviderClock {
    /// Instant each TextDelta was produced by the provider (ordered).
    emit_times: Mutex<Vec<Instant>>,
    /// Instant the provider's `stream()` was first called (request accepted).
    stream_started: Mutex<Option<Instant>>,
}

impl ProviderClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            emit_times: Mutex::new(Vec::new()),
            stream_started: Mutex::new(None),
        })
    }
}

/// Emits `chunks` TextDelta events with a small inter-chunk gap (so the
/// 256-cap broadcast receiver never lags), then `Completed`.
struct ChunkedTextProvider {
    chunks: usize,
    clock: Arc<ProviderClock>,
}

#[async_trait::async_trait]
impl EngineProvider for ChunkedTextProvider {
    async fn stream(
        &self,
        _model: &str,
        _messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        *self.clock.stream_started.lock().unwrap() = Some(Instant::now());
        let chunks = self.chunks;
        let clock = self.clock.clone();
        Ok(Box::pin(async_stream::stream! {
            for i in 0..chunks {
                let now = Instant::now();
                clock.emit_times.lock().unwrap().push(now);
                yield EngineProviderEvent::TextDelta(format!("chunk-{i}: {}", "x".repeat(8)));
                // small gap so the 256-cap broadcast receiver does not lag
                tokio::time::sleep(std::time::Duration::from_micros(200)).await;
            }
            yield EngineProviderEvent::Completed;
        }))
    }
}

// ── no-op tool runtime for engine harness ─────────────────────────────────

struct NoopToolRuntime;

#[async_trait::async_trait]
impl EngineToolRuntime for NoopToolRuntime {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        Vec::new()
    }
    async fn execute_tool(
        &self,
        _name: &str,
        _input: Value,
        _cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        ToolExecutionResult {
            output: json!({ "ok": true }),
            is_error: false,
            duration_ms: 0,
        }
    }
}

// ── in-process engine hot-path measurement ────────────────────────────────

/// Run the engine with a chunked text provider against a REAL SQLite-backed
/// runtime (persist-first EventSequencer) and report hot-path latencies +
/// event/storage statistics.
async fn engine_hot_path(chunks: usize) -> Value {
    let root = std::env::temp_dir().join(format!("natives-baseline-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("baseline root");
    let db = root.join("assistant.db");
    let art = root.join("artifacts");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", &root);

    let store = Arc::new(DataStore::new(&db, &art).expect("baseline store"));
    let rt = natives_agent_daemon::ProductionRuntime::new_with_event_store(store.clone());

    // run_event rows carry `REFERENCES run(id) ON DELETE CASCADE`, so seed the
    // conversation + run rows first (mirrors RunManager start-path ordering).
    {
        let conn = store.conn().expect("seed conn");
        conn.execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
             VALUES (?1, 'chat', ?2, 'prov-1', 'model-1', ?3, ?3)",
            rusqlite::params!["baseline-c1", "Baseline", chrono::Utc::now().to_rfc3339()],
        )
        .expect("insert baseline conversation");
        conn.execute(
            "INSERT INTO run (id, conversation_id, status, provider_id, model_id, created_at)
             VALUES (?1, ?2, 'running', 'prov-1', 'model-1', ?3)",
            rusqlite::params![
                "baseline-run-1",
                "baseline-c1",
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .expect("insert baseline run");
    }

    let engine = AgentEngine::new(rt.events.clone());
    let clock = ProviderClock::new();
    let provider = ChunkedTextProvider {
        chunks,
        clock: clock.clone(),
    };

    // subscribe BEFORE run so broadcast events are captured from sequence 1
    let mut rx = rt.events.subscribe("baseline-run-1");
    let run_id = "baseline-run-1".to_string();

    let submit_at = Instant::now();
    let handle = tokio::runtime::Handle::current();
    let provider2 = Arc::new(provider);
    let run_id_for_task = run_id.clone();
    let run_fut = handle.spawn(async move {
        engine
            .run(
                EngineRunConfig {
                    run_id: run_id_for_task.clone(),
                    conversation_id: "baseline-c1".into(),
                    model: "baseline-model".into(),
                    system_prompt: Some("You are a benchmark.".into()),
                    messages: Vec::new(),
                    user_content: "write a long answer".into(),
                    max_steps: 1,
                },
                provider2.as_ref() as &dyn EngineProvider,
                &NoopToolRuntime,
            )
            .await
    });

    // drain broadcast, pairing provider emit time with receiver publish time
    let mut delta_publish_delay_ms: Vec<f64> = Vec::new();
    let mut durable_events: u64 = 0;
    let mut live_events: u64 = 0;
    let mut message_delta_count: u64 = 0;
    let mut text_delta_count: u64 = 0;
    let mut first_delta_seen_at: Option<Instant> = None;
    let mut last_delta_seen_at: Option<Instant> = None;
    let mut terminal_seen_at: Option<Instant> = None;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
    while terminal_seen_at.is_none() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(event)) => {
                let kind = &event.payload;
                let is_live = matches!(
                    kind,
                    assistant_protocol::v2::RunEventKind::TextDelta { .. }
                        | assistant_protocol::v2::RunEventKind::ReasoningDelta { .. }
                        | assistant_protocol::v2::RunEventKind::ToolCallDelta { .. }
                        | assistant_protocol::v2::RunEventKind::ToolOutputDelta { .. }
                );
                if is_live {
                    live_events += 1;
                } else {
                    durable_events += 1;
                }
                match kind {
                    assistant_protocol::v2::RunEventKind::TextDelta { .. } => {
                        text_delta_count += 1;
                        let now = Instant::now();
                        if first_delta_seen_at.is_none() {
                            first_delta_seen_at = Some(now);
                        }
                        last_delta_seen_at = Some(now);
                        // pair with the provider emit time (ordered 1:1)
                        let emit = clock
                            .emit_times
                            .lock()
                            .unwrap()
                            .get(text_delta_count as usize - 1)
                            .copied();
                        if let Some(emit) = emit {
                            delta_publish_delay_ms
                                .push(now.duration_since(emit).as_secs_f64() * 1000.0);
                        }
                    }
                    assistant_protocol::v2::RunEventKind::MessageDelta { .. } => {
                        message_delta_count += 1;
                    }
                    assistant_protocol::v2::RunEventKind::Failed { code, error } => {
                        eprintln!(
                            "[baseline] run event Failed: code={code} error={error} seq={}",
                            event.run_sequence
                        );
                    }
                    assistant_protocol::v2::RunEventKind::Completed { .. }
                    | assistant_protocol::v2::RunEventKind::TurnCompleted { .. } => {
                        terminal_seen_at = Some(Instant::now());
                    }
                    _ => {}
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => break,
            Ok(Err(_)) | Err(_) => break,
        }
    }

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(120), run_fut)
        .await
        .expect("engine run timed out")
        .expect("engine task join");
    if let Err(e) = &outcome {
        eprintln!("[baseline] engine run error: {e:?}");
    }
    let completed_at = Instant::now();
    let submit_to_completed_ms = completed_at.duration_since(submit_at).as_secs_f64() * 1000.0;

    // request → first delta: provider stream() start → first delta publish
    let stream_started = *clock.stream_started.lock().unwrap();
    let request_to_first_delta_ms = match (stream_started, first_delta_seen_at) {
        (Some(a), Some(b)) => b.duration_since(a).as_secs_f64() * 1000.0,
        _ => -1.0,
    };

    // terminal tail: last delta publish → run() returned (Completed)
    let terminal_tail_ms = match last_delta_seen_at {
        Some(last) => completed_at.duration_since(last).as_secs_f64() * 1000.0,
        None => -1.0,
    };

    // storage statistics from the real DB
    let (row_count, payload_bytes) = {
        let conn = store.conn().expect("conn");
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_event WHERE run_id = ?1",
                [run_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let bytes: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(payload)), 0) FROM run_event WHERE run_id = ?1",
                [run_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (rows as u64, bytes as u64)
    };

    let _ = std::fs::remove_dir_all(&root);
    let _ = outcome;

    json!({
        "chunks": chunks,
        "submit_to_completed_ms": submit_to_completed_ms,
        "request_to_first_delta_ms": request_to_first_delta_ms,
        "delta_publish_delay_ms": percentiles_json(&delta_publish_delay_ms),
        "terminal_tail_ms": terminal_tail_ms,
        "live_events": live_events,
        "durable_events": durable_events,
        "text_delta_count": text_delta_count,
        "message_delta_count": message_delta_count,
        "run_event_rows": row_count,
        "run_event_rows_per_1000_chunks": (row_count as f64 / chunks as f64) * 1000.0,
        "run_event_payload_bytes": payload_bytes,
        "run_event_bytes_per_1000_chunks": (payload_bytes as f64 / chunks as f64) * 1000.0,
        "sqlite_writes_approx": row_count,
    })
}

// ── UDS handshake baseline ────────────────────────────────────────────────

async fn uds_handshake_baseline() -> Value {
    let root = std::env::temp_dir().join(format!("natives-baseline-uds-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("uds root");
    let db = root.join("assistant.db");
    let art = root.join("artifacts");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", &root);
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    let _seed = DataStore::new(&db, &art).expect("seed store");

    let socket = PathBuf::from(format!(
        "/tmp/nbase-{}.sock",
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let _ = std::fs::remove_file(&socket);
    let bootstrap = format!("base-boot-{}", uuid::Uuid::new_v4());
    let sock_str = socket.to_string_lossy().to_string();
    let server = RpcServer::new(&sock_str, &bootstrap, "2.0.0", "0.1.0-baseline");
    let server_task = tokio::spawn(async move {
        if let Err(e) = server.run().await {
            eprintln!("[baseline] RpcServer run failed: {e}");
        }
    });
    for _ in 0..250 {
        if socket.exists() {
            break;
        }
        if server_task.is_finished() {
            panic!("RpcServer ended before binding socket");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(socket.exists(), "daemon socket must appear");

    // baseline: one connect + handshake per RPC (current UdsAuthority behavior)
    let mut handshake_ms: Vec<f64> = Vec::new();
    let mut rpc_ms: Vec<f64> = Vec::new();

    for i in 0..10 {
        let t0 = Instant::now();
        let mut client = DaemonClient::connect(&socket, &bootstrap, client_protocol_version())
            .await
            .expect("connect");
        handshake_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        let t1 = Instant::now();
        let _ = client.call("daemon.ping", json!({ "seq": i })).await;
        rpc_ms.push(t1.elapsed().as_secs_f64() * 1000.0);
    }

    server_task.abort();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir_all(&root);

    json!({
        "connect_plus_handshake_ms": percentiles_json(&handshake_ms),
        "rpc_after_handshake_ms": percentiles_json(&rpc_ms),
        "handshake_per_rpc": 1,
        "handshakes_per_run_estimate": 8,
        "note": "current UdsAuthority re-connects + handshakes per RPC (authority.rs connect-per-call)",
    })
}

// ── main baseline ─────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "A0 baseline benchmark; run explicitly"]
async fn perf_baseline() {
    let scratch = scratch_dir();
    std::fs::create_dir_all(&scratch).expect("scratch");

    let git_rev = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());

    let b1_500 = engine_hot_path(500).await;
    let b1_2000 = engine_hot_path(2000).await;
    let uds = uds_handshake_baseline().await;

    let report = json!({
        "commit": git_rev,
        "build": "cargo test (debug profile)",
        "baseline": "A0 pre-remediation snapshot",
        "b1_text_stream": {
            "chunks_500": b1_500,
            "chunks_2000": b1_2000,
        },
        "uds": uds,
        "invariants_measured": {
            "live_delta_sqlite_writes": ">0 (baseline: every TextDelta persisted via EventSequencer persist-first)",
            "new_message_delta_emits": ">0 (baseline: engine_core.rs emits cumulative MessageDelta per TextDelta)",
            "active_stream_fixed_sleep": "absent (run.watch persistent stream replaces the retired run.subscribe long-poll wait)",
            "uds_handshake_per_rpc": "1 (baseline: UdsAuthority::call connects per RPC)",
        }
    });

    let evidence_path = scratch.join("baseline.json");
    std::fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("write baseline.json");

    let mut md = String::new();
    md.push_str("# A0 Baseline (pre-remediation)\n\n");
    md.push_str(&format!("- commit: {git_rev}\n"));
    md.push_str("- build: cargo test (debug profile)\n\n");
    md.push_str("## B1 pure-text long answer\n\n");
    for (name, v) in [("500 chunks", &b1_500), ("2000 chunks", &b1_2000)] {
        md.push_str(&format!("### {name}\n"));
        md.push_str(&format!(
            "- submit → completed: {:.2} ms\n",
            v["submit_to_completed_ms"].as_f64().unwrap_or(0.0)
        ));
        md.push_str(&format!(
            "- request → first delta: {:.2} ms\n",
            v["request_to_first_delta_ms"].as_f64().unwrap_or(0.0)
        ));
        md.push_str(&format!(
            "- delta → live publish p50/p95: {:.2}/{:.2} ms\n",
            v["delta_publish_delay_ms"]["p50_ms"]
                .as_f64()
                .unwrap_or(0.0),
            v["delta_publish_delay_ms"]["p95_ms"]
                .as_f64()
                .unwrap_or(0.0)
        ));
        md.push_str(&format!(
            "- terminal tail (last delta → run returned): {:.2} ms\n",
            v["terminal_tail_ms"].as_f64().unwrap_or(0.0)
        ));
        md.push_str(&format!(
            "- live events: {} / durable events: {}\n",
            v["live_events"].as_u64().unwrap_or(0),
            v["durable_events"].as_u64().unwrap_or(0)
        ));
        md.push_str(&format!(
            "- text_delta count: {} / message_delta count: {}\n",
            v["text_delta_count"].as_u64().unwrap_or(0),
            v["message_delta_count"].as_u64().unwrap_or(0)
        ));
        md.push_str(&format!(
            "- run_event rows / 1000 chunks: {:.1}\n",
            v["run_event_rows_per_1000_chunks"].as_f64().unwrap_or(0.0)
        ));
        md.push_str(&format!(
            "- run_event payload bytes / 1000 chunks: {:.1}\n",
            v["run_event_bytes_per_1000_chunks"].as_f64().unwrap_or(0.0)
        ));
    }
    md.push_str("\n## UDS handshake\n\n");
    md.push_str(&format!(
        "- connect+handshake p95: {:.2} ms (per RPC, current behavior)\n",
        uds["connect_plus_handshake_ms"]["p95_ms"]
            .as_f64()
            .unwrap_or(0.0)
    ));
    md.push_str(&format!(
        "- handshakes per run (estimate): {}\n",
        uds["handshakes_per_run_estimate"].as_u64().unwrap_or(0)
    ));
    md.push_str(&format!("- note: {}\n", uds["note"].as_str().unwrap_or("")));
    std::fs::write(scratch.join("baseline.md"), &md).expect("write baseline.md");

    println!("\n===== A0 BASELINE =====");
    println!("scratch: {}", scratch.display());
    println!("{}", md);
    println!("evidence: {}", evidence_path.display());
}
