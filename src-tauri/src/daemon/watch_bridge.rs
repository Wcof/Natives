//! Host watch bridge — dedicated daemon→host persistent stream.
//!
//! RunWatchStreamV2 frames arriving on the daemon UDS connection are forwarded
//! to the Renderer as Tauri `run-watch-frame` events. One dedicated tokio task
//! per `run_id`; cancelled on `run_watch_stop`, on a terminal durable event, or
//! on stream error (the Renderer reconnects by durable/live cursor).
//!
//! The Renderer never connects to the UDS socket itself — it only talks to
//! these Tauri commands and listens for the emitted frames.

use natives_agent_daemon::{
    authority::WatchEventStream,
    resolve_run_authority_mode,
    stream_protocol::{RunStreamFrameV2, RunStreamLane},
    RunAuthorityMode, UdsAuthority,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Notify;

/// Tauri event emitted once per RunWatchStreamV2 frame.
pub const WATCH_FRAME_EVENT: &str = "run-watch-frame";

/// The daemon's `run.watch` sends a heartbeat every 15s on idle so the 30s
/// client frame timeout never fires; the bridge below only needs the idle
/// window to detect a dead stream.
/// If no frame (including heartbeat) arrives within this window the stream is
/// considered dead and the watch task tears down (heartbeat timeout handling).
const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// One `run-watch-frame` event payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WatchFrameEvent {
    pub run_id: String,
    pub frame: RunStreamFrameV2,
}

/// Shared bridge registry: `run_id` → per-run watch handle.
#[derive(Default)]
pub struct WatchBridgeState {
    /// The whole state is already guarded by `Arc<Mutex<WatchBridgeState>>`,
    /// so this is a plain map (no double Mutex).
    inner: HashMap<String, WatchHandle>,
}

struct WatchHandle {
    cancel: Arc<Notify>,
    last_frame_at: Instant,
    last_durable_sequence: u64,
    last_live_sequence: u64,
    last_heartbeat_at: Option<Instant>,
    terminal: bool,
    error: Option<String>,
}

impl WatchHandle {
    fn new() -> Self {
        Self {
            cancel: Arc::new(Notify::new()),
            last_frame_at: Instant::now(),
            last_durable_sequence: 0,
            last_live_sequence: 0,
            last_heartbeat_at: None,
            terminal: false,
            error: None,
        }
    }
}

/// Start (or restart) a dedicated watch task for a run.
///
/// The task consumes the daemon's `run.watch` stream and emits one
/// `run-watch-frame` Tauri event per frame. A terminal durable event cleanly
/// stops the task; `run_watch_stop` cancels it (unsubscribe); stream errors
/// emit a `resync_required { lane: "durable", reason: "stream_closed:.." }`
/// frame so the Renderer can reconnect by cursor.
#[tauri::command]
pub async fn run_watch_start(
    app: AppHandle,
    state: State<'_, Arc<Mutex<WatchBridgeState>>>,
    run_id: String,
    after_durable_sequence: u64,
    after_live_sequence: u64,
) -> crate::Result<serde_json::Value> {
    let cancel = {
        let mut guard = state
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;
        if let Some(existing) = guard.inner.get(&run_id) {
            existing.cancel.notify_one(); // idempotent restart
        }
        let handle = WatchHandle::new();
        guard.inner.insert(run_id.clone(), handle);
        guard
            .inner
            .get(&run_id)
            .map(|h| h.cancel.clone())
            .ok_or_else(|| crate::Error::Internal("watch bridge state lost".into()))?
    };

    let state_for_task = Arc::clone(&state);
    let app_for_task = app.clone();
    let task_run_id = run_id.clone();
    tokio::spawn(async move {
        run_watch_task(
            app_for_task,
            state_for_task,
            task_run_id,
            after_durable_sequence,
            after_live_sequence,
            cancel,
        )
        .await;
    });
    Ok(serde_json::json!({
        "ok": true,
        "runId": run_id,
        "stream": "run.watch",
        "streamVersion": 2,
    }))
}

/// Cancel the watch task for a run (unsubscribe). Idempotent.
#[tauri::command]
pub async fn run_watch_stop(
    state: State<'_, Arc<Mutex<WatchBridgeState>>>,
    run_id: String,
) -> crate::Result<()> {
    let guard = state
        .lock()
        .map_err(|e| crate::Error::Internal(e.to_string()))?;
    if let Some(handle) = guard.inner.get(&run_id) {
        handle.cancel.notify_one();
    }
    Ok(())
}

/// Current watch state for a run (active / cursors / heartbeat idle / error).
#[tauri::command]
pub async fn run_watch_state(
    state: State<'_, Arc<Mutex<WatchBridgeState>>>,
    run_id: String,
) -> crate::Result<serde_json::Value> {
    let guard = state
        .lock()
        .map_err(|e| crate::Error::Internal(e.to_string()))?;
    let Some(handle) = guard.inner.get(&run_id) else {
        return Ok(serde_json::json!({ "runId": run_id, "active": false }));
    };
    let last_heartbeat_ms = handle
        .last_heartbeat_at
        .map(|t| t.elapsed().as_millis() as u64);
    let idle_ms = handle.last_frame_at.elapsed().as_millis() as u64;
    Ok(serde_json::json!({
        "runId": run_id,
        "active": !handle.terminal,
        "lastDurableSequence": handle.last_durable_sequence,
        "lastLiveSequence": handle.last_live_sequence,
        "lastHeartbeatMs": last_heartbeat_ms,
        "idleMs": idle_ms,
        "terminal": handle.terminal,
        "error": handle.error,
    }))
}

async fn run_watch_task(
    app: AppHandle,
    state: Arc<Mutex<WatchBridgeState>>,
    run_id: String,
    after_durable_sequence: u64,
    after_live_sequence: u64,
    cancel: Arc<Notify>,
) {
    let mut stream = match open_stream(&run_id, after_durable_sequence, after_live_sequence).await {
        Ok(stream) => stream,
        Err(error) => {
            set_error(&state, &run_id, error.clone());
            // Tell the Renderer the persistent path is unavailable so it can
            // fall back to legacy `run.subscribe`.
            let _ = app.emit(
                WATCH_FRAME_EVENT,
                WatchFrameEvent {
                    run_id: run_id.clone(),
                    frame: RunStreamFrameV2::resync_required(
                        &run_id,
                        RunStreamLane::Durable,
                        format!("watch_start_failed:{error}"),
                    ),
                },
            );
            remove_handle(&state, &run_id);
            return;
        }
    };

    let mut idle = tokio::time::interval(STREAM_IDLE_TIMEOUT);
    idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    idle.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            _ = cancel.notified() => break,
            _ = idle.tick() => {
                // Heartbeat timeout handling: a healthy daemon sends a heartbeat
                // every 15s; silence beyond the idle window means the stream is
                // dead — tear down so the Renderer reconnects by cursor.
                let idle_since = {
                    let Ok(guard) = state.lock() else {
                        break;
                    };
                    guard.inner.get(&run_id).map(|h| h.last_frame_at.elapsed()).unwrap_or_default()
                };
                if idle_since >= STREAM_IDLE_TIMEOUT {
                    emit_stream_closed(&app, &state, &run_id, "heartbeat_timeout");
                    break;
                }
            }
            next = stream.next_frame() => {
                match next {
                    Some(Ok(frame)) => {
                        let is_terminal = matches!(
                            &frame,
                            RunStreamFrameV2::Event { lane: RunStreamLane::Durable, event_type, .. }
                                if matches!(event_type.as_str(), "completed" | "failed" | "cancelled" | "interrupted")
                        );
                        update_handle(&state, &run_id, &frame, is_terminal);
                        let _ = app.emit(
                            WATCH_FRAME_EVENT,
                            WatchFrameEvent { run_id: run_id.clone(), frame },
                        );
                        if is_terminal {
                            break; // clean close after the terminal durable event
                        }
                    }
                    Some(Err(error)) => {
                        emit_stream_closed(&app, &state, &run_id, &format!("transport:{error}"));
                        break;
                    }
                    None => {
                        // Clean EOF without a terminal frame (cancel/disconnect).
                        emit_stream_closed(&app, &state, &run_id, "stream_closed");
                        break;
                    }
                }
            }
        }
    }
    remove_handle(&state, &run_id);
}

/// Open the daemon stream. Requires UDS mode; the embedded authority has no
/// sidecar stream and the Renderer falls back to legacy `run.subscribe`.
async fn open_stream(
    run_id: &str,
    after_durable_sequence: u64,
    after_live_sequence: u64,
) -> Result<WatchEventStream, String> {
    match resolve_run_authority_mode() {
        RunAuthorityMode::Embedded => Err(
            "run.watch persistent stream requires UDS mode (embedded authority has no stream)"
                .into(),
        ),
        RunAuthorityMode::Uds {
            socket,
            bootstrap_token,
        } => {
            if bootstrap_token.is_empty() {
                return Err(
                    "NATIVES_DAEMON_BOOTSTRAP required for UDS watch stream (no embedded fallback)"
                        .into(),
                );
            }
            let auth = UdsAuthority::from_mode(socket, bootstrap_token).map_err(|e| e.message())?;
            auth.watch_events(run_id, after_durable_sequence, after_live_sequence)
                .await
                .map_err(|e| e.message())
        }
    }
}

fn emit_stream_closed(
    app: &AppHandle,
    state: &Arc<Mutex<WatchBridgeState>>,
    run_id: &str,
    reason: &str,
) {
    set_error(state, run_id, reason.to_string());
    let _ = app.emit(
        WATCH_FRAME_EVENT,
        WatchFrameEvent {
            run_id: run_id.to_string(),
            frame: RunStreamFrameV2::resync_required(run_id, RunStreamLane::Durable, reason),
        },
    );
}

fn set_error(state: &Arc<Mutex<WatchBridgeState>>, run_id: &str, error: String) {
    if let Ok(mut guard) = state.lock() {
        if let Some(handle) = guard.inner.get_mut(run_id) {
            handle.error = Some(error);
            handle.terminal = true;
        }
    }
}

fn update_handle(
    state: &Arc<Mutex<WatchBridgeState>>,
    run_id: &str,
    frame: &RunStreamFrameV2,
    terminal: bool,
) {
    if let Ok(mut guard) = state.lock() {
        if let Some(handle) = guard.inner.get_mut(run_id) {
            handle.last_frame_at = Instant::now();
            match frame {
                RunStreamFrameV2::Event {
                    lane: RunStreamLane::Durable,
                    durable_sequence: Some(seq),
                    ..
                } => {
                    handle.last_durable_sequence = *seq;
                }
                RunStreamFrameV2::Event {
                    lane: RunStreamLane::Live,
                    live_sequence: Some(seq),
                    ..
                } => {
                    handle.last_live_sequence = *seq;
                }
                RunStreamFrameV2::Heartbeat {
                    durable_sequence,
                    live_sequence,
                    ..
                } => {
                    handle.last_heartbeat_at = Some(Instant::now());
                    handle.last_durable_sequence = *durable_sequence;
                    handle.last_live_sequence = *live_sequence;
                }
                RunStreamFrameV2::Event { .. } | RunStreamFrameV2::ResyncRequired { .. } => {}
            }
            if terminal {
                handle.terminal = true;
            }
        }
    }
}

fn remove_handle(state: &Arc<Mutex<WatchBridgeState>>, run_id: &str) {
    if let Ok(mut guard) = state.lock() {
        guard.inner.remove(run_id);
    }
}
