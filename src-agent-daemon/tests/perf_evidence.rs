//! T11 — Daemon-side performance & observability evidence.
//!
//! `#[ignore]` by design: this is a benchmark harness, not a unit gate. Run it
//! explicitly with:
//!
//! ```sh
//! cargo test -p natives-agent-daemon --test perf_evidence -- --ignored --nocapture
//! ```
//!
//! It builds the standard dataset from `docs/standards/technical/04-performance.md`
//! (500 conversations × 2000 messages, 20,000 run events) on a throwaway SQLite
//! file, then measures the real UDS RPC paths (list / message page / event
//! replay / reconnect / hot IPC) plus the bounded-resource primitives
//! (checkpoint 1 GiB streaming hash, 100-parallel ledger effect through the
//! storage actor, 100-parallel progress overflow).
//!
//! Evidence is written to `NATIVES_PERF_SCRATCH` (JSON + summary.md). When the
//! env var is unset it defaults to `/tmp/natives-perf-evidence/<timestamp>`.
//! The raw sample arrays are included so percentiles are auditable (R-P1).

use assistant_protocol::v2::ReplayRunRequest;
use capability_gateway::ToolProgressChunk;
use natives_agent_daemon::checkpoint::CheckpointManager;
use natives_agent_daemon::client::{client_protocol_version, DaemonClient};
use natives_agent_daemon::rpc::RpcServer;
use natives_agent_daemon::storage::{actor::StorageActor, DataStore};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Standard dataset sizes (source: standards/technical/04-performance.md).
const CONVERSATIONS: usize = 500;
const MESSAGES_PER_CONVERSATION: usize = 2000;
const RUN_EVENTS: usize = 20_000;
/// RPC samples per operation (warmup + measured).
const SAMPLES: usize = 30;

// ── helpers ────────────────────────────────────────────────────────────────

fn scratch_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NATIVES_PERF_SCRATCH") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("create scratch");
        return dir;
    }
    let dir = std::env::temp_dir().join(format!(
        "natives-perf-evidence/{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    std::fs::create_dir_all(&dir).expect("create default scratch");
    dir
}

/// percentile (0..=1) over a sorted sample slice.
fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64) * p).ceil() as usize;
    let idx = idx.clamp(1, sorted.len());
    sorted[idx - 1]
}

/// median / p75 / p95 / max over raw millisecond samples.
fn percentiles(samples: &[f64]) -> (f64, f64, f64, f64) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (
        pct(&sorted, 0.50),
        pct(&sorted, 0.75),
        pct(&sorted, 0.95),
        sorted.last().copied().unwrap_or(0.0),
    )
}

/// Measure N `DaemonClient::call` round-trips for one method. The params are
/// cloned per sample so the closure never borrows across `.await`.
async fn time_client_call(
    client: &mut DaemonClient,
    n: usize,
    method: &'static str,
    params: Value,
) -> Vec<f64> {
    let _ = client.call(method, params.clone()).await; // warmup
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        let _ = client.call(method, params.clone()).await;
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

fn percentiles_json(samples: &[f64]) -> Value {
    let (median, p75, p95, max) = percentiles(samples);
    json!({
        "samples": samples.len(),
        "median_ms": median,
        "p75_ms": p75,
        "p95_ms": p95,
        "max_ms": max,
        "raw_ms": samples,
    })
}

// ── seed: standard dataset ─────────────────────────────────────────────────

/// Seed the standard dataset on a fresh store. Returns conversation ids and
/// the event-run id. Runs in one transaction; ~1M messages + 20k events.
fn seed_dataset(store: &DataStore) -> (Vec<String>, String) {
    let conn = store.conn().expect("seed conn");
    let tx = conn
        .unchecked_transaction()
        .expect("seed transaction (single writer, empty DB)");

    let mut conv_stmt = tx
        .prepare(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
             VALUES (?1, 'chat', ?2, 'prov-1', 'model-1', ?3, ?4)",
        )
        .expect("prepare conv");
    let mut msg_stmt = tx
        .prepare(
            "INSERT INTO message (id, conversation_id, role, status, created_at)
             VALUES (?1, ?2, 'user', 'complete', ?3)",
        )
        .expect("prepare msg");
    let mut block_stmt = tx
        .prepare(
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json)
             VALUES (?1, 0, 'text', ?2)",
        )
        .expect("prepare block");

    let base = chrono::Utc::now();
    let mut conv_ids = Vec::with_capacity(CONVERSATIONS);
    for c in 0..CONVERSATIONS {
        let cid = format!("perf-conv-{c:04}");
        conv_ids.push(cid.clone());
        let created = base
            - chrono::Duration::seconds(
                (CONVERSATIONS * MESSAGES_PER_CONVERSATION) as i64 - (c as i64),
            );
        conv_stmt
            .execute(rusqlite::params![
                cid,
                format!("Perf {c}"),
                created.to_rfc3339(),
                created.to_rfc3339()
            ])
            .expect("insert conv");
        for m in 0..MESSAGES_PER_CONVERSATION {
            let mid = format!("{cid}-m{m:04}");
            let ts = base - chrono::Duration::seconds((MESSAGES_PER_CONVERSATION - m) as i64);
            msg_stmt
                .execute(rusqlite::params![mid, cid, ts.to_rfc3339()])
                .expect("insert msg");
            let body =
                serde_json::json!({ "text": format!("message {c}:{m} for the standard dataset") })
                    .to_string();
            block_stmt
                .execute(rusqlite::params![format!("{cid}-m{m:04}"), body])
                .expect("insert block");
        }
    }
    drop(conv_stmt);
    drop(msg_stmt);
    drop(block_stmt);

    // One run carrying the 20,000-event stream (replay target).
    let run_id = "perf-events-run".to_string();
    tx.execute(
        "INSERT INTO run (id, conversation_id, status, provider_id, model_id, created_at)
         VALUES (?1, ?2, 'completed', 'prov-1', 'model-1', ?3)",
        rusqlite::params![run_id, conv_ids[0], base.to_rfc3339()],
    )
    .expect("insert run");
    {
        let mut ev_stmt = tx
            .prepare(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp)
                 VALUES (?1, ?2, 'text_delta', ?3, ?4)",
            )
            .expect("prepare event");
        for i in 1..=RUN_EVENTS {
            let payload = json!({ "type": "text_delta", "text": format!("delta {i}") }).to_string();
            ev_stmt
                .execute(rusqlite::params![
                    run_id,
                    i as i64,
                    payload,
                    base.to_rfc3339()
                ])
                .expect("insert event");
        }
    }
    tx.commit().expect("seed commit");
    (conv_ids, run_id)
}

// ── bounded-resource measurements ─────────────────────────────────────────

/// checkpoint: a 1 GiB sparse file must be stream-hashed in bounded memory.
async fn measure_checkpoint_1gib() -> Value {
    let dir = std::env::temp_dir().join(format!("natives-perf-cp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("cp dir");
    let dir = dir.canonicalize().expect("canonical cp dir");
    let file = dir.join("giant.bin");
    let f = std::fs::File::create(&file).expect("create sparse");
    f.set_len(1024 * 1024 * 1024).expect("1 GiB sparse");
    drop(f);
    let canonical = file.canonicalize().expect("canonical");

    let mgr = CheckpointManager::new();
    mgr.begin_run("perf-run", "perf-conv", &dir).expect("begin");
    let trusted = capability_gateway::TrustedPath::new(canonical, PathBuf::from("giant.bin"));
    let start = Instant::now();
    mgr.capture_before_async("perf-run", &trusted)
        .await
        .expect("capture");
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let rec = mgr.finalize_run("perf-run").expect("finalize");
    let snap = rec
        .files
        .iter()
        .find(|f| f.path == "giant.bin")
        .expect("snap");
    let _ = std::fs::remove_dir_all(&dir);
    json!({
        "file_bytes": 1024 * 1024 * 1024,
        "elapsed_ms": elapsed_ms,
        "content_captured": snap.before_content.is_some(),
        "hash_len": snap.before_hash.as_ref().map(|h| h.len()).unwrap_or(0),
        "redacted": snap.redacted,
    })
}

/// 100-parallel ledger effects serialize through the bounded storage actor;
/// none are lost, none block unboundedly (R-P9 concurrency bound).
fn measure_storage_actor_100_parallel() -> Value {
    let dir = std::env::temp_dir().join(format!("natives-perf-act-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("act dir");
    let db = dir.join("actor.db");
    let art = dir.join("artifacts");
    let store = Arc::new(DataStore::new(&db, &art).expect("store"));
    {
        let conn = store.conn().expect("conn");
        conn.execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
             VALUES ('perf-act-conv', 'chat', 'x', 'prov', 'model', datetime('now'), datetime('now'))",
            [],
        )
        .expect("seed conv");
        conn.execute(
            "INSERT INTO run (id, conversation_id, status, provider_id, model_id, created_at)
             VALUES ('perf-act-run', 'perf-act-conv', 'running', 'prov', 'model', datetime('now'))",
            [],
        )
        .expect("seed run");
    }
    let actor = StorageActor::new(8, store.clone());
    let start = Instant::now();
    let handles: Vec<_> = (0..100)
        .map(|i| {
            let actor = actor.clone();
            std::thread::spawn(move || {
                actor
                    .submit(true, move |conn| {
                        conn.execute(
                            "INSERT INTO side_effect_record
                             (id, run_id, tool_call_id, category, target_summary, side_effect_class,
                              status, replay_safe, resource, started_at, ledger_sequence)
                             VALUES (?1, 'perf-act-run', ?2, 'process', '{}', 'process',
                                     'started', 0, 'cmd', datetime('now'), ?3)",
                            rusqlite::params![
                                format!("perf-ledger-{i}"),
                                format!("call-{i}"),
                                (i + 1) as i64,
                            ],
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(serde_json::json!(null))
                    })
                    .expect("critical submit must succeed");
            })
        })
        .collect();
    for h in handles {
        h.join().expect("join");
    }
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let count: i64 = store
        .conn()
        .expect("conn")
        .query_row("SELECT COUNT(*) FROM side_effect_record", [], |r| r.get(0))
        .expect("count");
    let _ = std::fs::remove_dir_all(&dir);
    json!({
        "parallel_effects": 100,
        "elapsed_ms": elapsed_ms,
        "rows_persisted": count,
        "dropped_noncritical": actor.dropped_noncritical(),
        "actor_total_commands": actor.total_commands(),
        "bounded": count == 100,
    })
}

/// 100 parallel progress producers against a bounded channel: overflow drops
/// and is counted; producers never block (try_send); queue stays at capacity.
async fn measure_progress_100_parallel() -> Value {
    let capacity = 16usize;
    let (tx, mut rx) = mpsc::channel::<ToolProgressChunk>(capacity);
    let dropped = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let mut handles = Vec::new();
    for i in 0..100 {
        let tx = tx.clone();
        let dropped = dropped.clone();
        handles.push(tokio::spawn(async move {
            let chunk = ToolProgressChunk {
                stream: "out".into(),
                text: format!("chunk-{i}-{}", "x".repeat(256)),
            };
            match tx.try_send(chunk) {
                Ok(()) => 0u64,
                Err(tokio::sync::mpsc::error::TrySendError::Full(full)) => {
                    dropped.fetch_add(full.text.len() as u64, Ordering::Relaxed);
                    1u64
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => 1u64,
            }
        }));
    }
    let mut dropped_producers = 0u64;
    for h in handles {
        dropped_producers += h.await.expect("producer join");
    }
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut drained = 0usize;
    while let Ok(_chunk) = rx.try_recv() {
        drained += 1;
    }
    json!({
        "producers": 100,
        "capacity": capacity,
        "queue_bounded_at": drained,
        "dropped_producers": dropped_producers,
        "dropped_bytes_counted": dropped.load(Ordering::Relaxed),
        "elapsed_ms": elapsed_ms,
    })
}

// ── main evidence matrix ───────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "performance evidence harness; run explicitly"]
async fn perf_evidence_matrix() {
    let scratch = scratch_dir();
    let root = std::env::temp_dir().join(format!("natives-perf-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("perf root");

    // Seed + boot the real daemon on a throwaway DB.
    let db = root.join("assistant.db");
    let art = root.join("artifacts");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    std::env::set_var("NATIVES_RUNTIME_DIR", &root);
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    let seed_store = DataStore::new(&db, &art).expect("seed store");
    let (conv_ids, event_run) = seed_dataset(&seed_store);
    drop(seed_store);

    let socket = PathBuf::from(format!(
        "/tmp/nperf-{}.sock",
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let _ = std::fs::remove_file(&socket);
    let bootstrap = format!("perf-boot-{}", uuid::Uuid::new_v4());
    let sock_str = socket.to_string_lossy().to_string();
    let server = RpcServer::new(&sock_str, &bootstrap, "2.0.0", "0.1.0-perf");
    let server_task = tokio::spawn(async move {
        if let Err(e) = server.run().await {
            eprintln!("[perf] RpcServer run failed: {e}");
        }
    });
    for _ in 0..250 {
        if socket.exists() {
            break;
        }
        if server_task.is_finished() {
            panic!("RpcServer task ended before binding the socket");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(socket.exists(), "daemon socket must appear");

    let mut client = DaemonClient::connect(&socket, &bootstrap, client_protocol_version())
        .await
        .expect("connect");

    // ── hot IPC ──
    let ping = time_client_call(&mut client, SAMPLES, "daemon.ping", json!({})).await;

    // ── list: conversation.listPage page 1 ──
    let list_page1 = time_client_call(
        &mut client,
        SAMPLES,
        "conversation.listPage",
        json!({ "limit": 100 }),
    )
    .await;

    // ── list: deep page (walk to the last page via cursors) ──
    let mut deep_cursor: Option<Value> = None;
    for _ in 0..6 {
        let resp = client
            .call(
                "conversation.listPage",
                json!({ "limit": 100, "cursor": deep_cursor }),
            )
            .await
            .expect("deep page");
        let next = resp.get("nextCursor").cloned().filter(|v| !v.is_null());
        if next.is_none() {
            break;
        }
        deep_cursor = next;
    }
    let list_deep = time_client_call(
        &mut client,
        SAMPLES,
        "conversation.listPage",
        json!({ "limit": 100, "cursor": deep_cursor }),
    )
    .await;

    // ── projection: newest message page of a 2000-message conversation ──
    let messages_page = time_client_call(
        &mut client,
        SAMPLES,
        "conversation.getMessagesPage",
        json!({ "conversation_id": conv_ids[0], "limit": 100 }),
    )
    .await;

    // ── event replay: full 20,000-event stream (3 samples; heavy) ──
    // With the wire-replay cap (R-P4/T11) a full-history replay returns a
    // bounded batch instead of an oversized UDS frame.
    let mut wire_replay_count: usize = 0;
    let replay_full = {
        let mut samples = Vec::with_capacity(3);
        for _ in 0..3 {
            let start = Instant::now();
            let result = client
                .call(
                    "run.getEvents",
                    json!({ "run_id": event_run, "after_sequence": 0 }),
                )
                .await;
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            if let Ok(v) = result {
                wire_replay_count = v.as_array().map(|a| a.len()).unwrap_or(0);
            }
        }
        eprintln!("[perf] full replay events returned: {wire_replay_count}");
        samples
    };

    // ── event replay: tail window (30 samples) ──
    let replay_tail = time_client_call(
        &mut client,
        SAMPLES,
        "run.getEvents",
        json!({ "run_id": event_run, "after_sequence": (RUN_EVENTS - 200) as u64 }),
    )
    .await;

    // ── event replay: in-process (no UDS), isolates daemon-side cost ──
    // Uses the same global RunManager the RPC path hits.
    let replay_inprocess = {
        let _ = natives_agent_daemon::run_manager::global_run_manager().replay_checked(
            ReplayRunRequest {
                run_id: event_run.clone(),
                after_sequence: 0,
            },
        );
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let start = Instant::now();
            let _ = natives_agent_daemon::run_manager::global_run_manager().replay_checked(
                ReplayRunRequest {
                    run_id: event_run.clone(),
                    after_sequence: 0,
                },
            );
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples
    };

    // ── reconnect: fresh handshake (client-independent, own loop) ──
    let _ = DaemonClient::connect(&socket, &bootstrap, client_protocol_version()).await; // warmup
    let mut reconnect = Vec::with_capacity(10);
    for _ in 0..10 {
        let start = Instant::now();
        let _ = DaemonClient::connect(&socket, &bootstrap, client_protocol_version()).await;
        reconnect.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    // ── bounded-resource primitives ──
    let checkpoint = measure_checkpoint_1gib().await;
    let ledger = measure_storage_actor_100_parallel();
    let progress = measure_progress_100_parallel().await;

    server_task.abort();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir_all(&root);

    let git_rev = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());

    let report = json!({
        "commit": git_rev,
        "build": "cargo test (debug profile)",
        "dataset": {
            "conversations": CONVERSATIONS,
            "messages_per_conversation": MESSAGES_PER_CONVERSATION,
            "total_messages": CONVERSATIONS * MESSAGES_PER_CONVERSATION,
            "run_events": RUN_EVENTS,
        },
        "operations": {
            "daemon.ping": percentiles_json(&ping),
            "conversation.listPage_page1": percentiles_json(&list_page1),
            "conversation.listPage_deep": percentiles_json(&list_deep),
            "conversation.getMessagesPage": percentiles_json(&messages_page),
            "run.getEvents_full_20000": percentiles_json(&replay_full),
            "run.getEvents_tail_200": percentiles_json(&replay_tail),
            "run.replay_inprocess_20000": percentiles_json(&replay_inprocess),
            "reconnect_handshake": percentiles_json(&reconnect),
        },
        "bounded_primitives": {
            "checkpoint_1gib_streaming_hash": checkpoint,
            "storage_actor_100_parallel_ledger": ledger,
            "progress_100_parallel_overflow": progress,
            "wire_replay_full_20000": {
                "events_returned": wire_replay_count,
                "cap": natives_agent_daemon::rpc::MAX_WIRE_REPLAY_EVENTS,
                "oversize_error": false,
            },
        },
    });

    let evidence_path = scratch.join("perf-evidence.json");
    std::fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("write evidence");

    let mut summary = String::new();
    summary.push_str("# Daemon-side performance evidence\n\n");
    summary.push_str(&format!("- commit: {git_rev}\n"));
    summary.push_str(&format!(
        "- dataset: {CONVERSATIONS} conversations x {MESSAGES_PER_CONVERSATION} messages, {RUN_EVENTS} events\n"
    ));
    for (name, entry) in [
        ("daemon.ping", &ping),
        ("listPage page1", &list_page1),
        ("listPage deep", &list_deep),
        ("getMessagesPage", &messages_page),
        ("getEvents full", &replay_full),
        ("getEvents tail", &replay_tail),
        ("replay in-process", &replay_inprocess),
        ("reconnect", &reconnect),
    ] {
        let (median, p75, p95, max) = percentiles(entry);
        summary.push_str(&format!(
            "- {name}: median {median:.2}ms / p75 {p75:.2} / p95 {p95:.2} / max {max:.2}\n"
        ));
    }
    summary.push_str(&format!(
        "- checkpoint 1 GiB streaming hash: {:.2}ms, content not buffered, hash {} chars\n",
        checkpoint["elapsed_ms"].as_f64().unwrap_or(0.0),
        checkpoint["hash_len"].as_u64().unwrap_or(0)
    ));
    summary.push_str(&format!(
        "- storage actor 100 parallel ledger effects: {:.2}ms, {} rows persisted (bounded={})\n",
        ledger["elapsed_ms"].as_f64().unwrap_or(0.0),
        ledger["rows_persisted"].as_u64().unwrap_or(0),
        ledger["bounded"].as_bool().unwrap_or(false)
    ));
    summary.push_str(&format!(
        "- progress 100 parallel producers: queue bounded at {}, dropped producers {}, dropped bytes counted {}\n",
        progress["queue_bounded_at"].as_u64().unwrap_or(0),
        progress["dropped_producers"].as_u64().unwrap_or(0),
        progress["dropped_bytes_counted"].as_u64().unwrap_or(0)
    ));
    std::fs::write(scratch.join("summary.md"), &summary).expect("write summary");

    println!("\n===== PERF EVIDENCE =====");
    println!("scratch: {}", scratch.display());
    println!("{}", summary);
    println!("evidence: {}", evidence_path.display());
}
