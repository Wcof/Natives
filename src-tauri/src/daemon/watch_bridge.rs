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
use std::collections::{HashMap, VecDeque};
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

/// Bound on retained per-run cursor records (`last_cursors`) so the reconnect
/// cursor history can never grow unboundedly (FIFO eviction).
const CURSOR_RECORD_CAP: usize = 64;

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
    /// Last known dual cursors for runs whose watch task has ended (terminal /
    /// stream_closed / error). `run_watch_state` still answers these so the
    /// Renderer can recover `lastDurableSequence`/`lastLiveSequence` on
    /// reconnect. Bounded by `CURSOR_RECORD_CAP` (FIFO eviction).
    last_cursors: HashMap<String, CursorRecord>,
    /// Insertion order for FIFO eviction of `last_cursors`.
    cursor_order: VecDeque<String>,
}

impl WatchBridgeState {
    /// Record the final dual cursors of an ended watch task, bounded by
    /// `CURSOR_RECORD_CAP`.
    fn retain_cursor(&mut self, run_id: &str, rec: CursorRecord) {
        if !self.last_cursors.contains_key(run_id) {
            self.cursor_order.push_back(run_id.to_string());
        }
        self.last_cursors.insert(run_id.to_string(), rec);
        while self.cursor_order.len() > CURSOR_RECORD_CAP {
            if let Some(oldest) = self.cursor_order.pop_front() {
                self.last_cursors.remove(&oldest);
            }
        }
    }
}

/// Final cursor snapshot of a run whose watch task has ended. Kept (bounded)
/// so `run_watch_state` can report `lastDurableSequence`/`lastLiveSequence`
/// for reconnect cursor recovery after the task is gone.
#[derive(Clone, Debug)]
struct CursorRecord {
    last_durable_sequence: u64,
    last_live_sequence: u64,
    terminal: bool,
    error: Option<String>,
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
    Ok(snapshot_state(&guard, &run_id))
}

/// Pure state snapshot shared by the `run_watch_state` command and tests.
///
/// Active watch → live handle cursors; ended watch → retained cursor record
/// (`active: false`, last dual cursors preserved for reconnect recovery);
/// unknown run → `{ runId, active: false }`.
fn snapshot_state(state: &WatchBridgeState, run_id: &str) -> serde_json::Value {
    if let Some(handle) = state.inner.get(run_id) {
        let last_heartbeat_ms = handle
            .last_heartbeat_at
            .map(|t| t.elapsed().as_millis() as u64);
        let idle_ms = handle.last_frame_at.elapsed().as_millis() as u64;
        return serde_json::json!({
            "runId": run_id,
            "active": !handle.terminal,
            "lastDurableSequence": handle.last_durable_sequence,
            "lastLiveSequence": handle.last_live_sequence,
            "lastHeartbeatMs": last_heartbeat_ms,
            "idleMs": idle_ms,
            "terminal": handle.terminal,
            "error": handle.error,
        });
    }
    if let Some(rec) = state.last_cursors.get(run_id) {
        return serde_json::json!({
            "runId": run_id,
            "active": false,
            "lastDurableSequence": rec.last_durable_sequence,
            "lastLiveSequence": rec.last_live_sequence,
            "lastHeartbeatMs": serde_json::Value::Null,
            "idleMs": serde_json::Value::Null,
            "terminal": rec.terminal,
            "error": rec.error,
        });
    }
    serde_json::json!({ "runId": run_id, "active": false })
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
            // surface the error / retry (the legacy `run.subscribe` long-poll
            // fallback is retired, MIG-004).
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
            remove_handle(&state, &run_id, &cancel);
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
    remove_handle(&state, &run_id, &cancel);
}

/// Open the daemon stream. Requires UDS mode; the embedded authority has no
/// sidecar stream (the Renderer gets a WatchStreamUnavailableError — the
/// legacy `run.subscribe` long-poll fallback is retired, MIG-004).
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

/// Tear down a watch task's handle, snapshotting its final dual cursors into
/// `last_cursors` so `run_watch_state` can still answer reconnect cursor
/// recovery after the task is gone.
///
/// `cancel` is the task's own cancel token: a stale task that was replaced by
/// a restart (`run_watch_start` re-inserts a fresh handle) must never remove
/// the new handle (double-cursor reconnect invariant).
fn remove_handle(state: &Arc<Mutex<WatchBridgeState>>, run_id: &str, cancel: &Arc<Notify>) {
    if let Ok(mut guard) = state.lock() {
        let mut retained: Option<CursorRecord> = None;
        if let Some(handle) = guard.inner.get(run_id) {
            if Arc::ptr_eq(&handle.cancel, cancel) {
                retained = Some(CursorRecord {
                    last_durable_sequence: handle.last_durable_sequence,
                    last_live_sequence: handle.last_live_sequence,
                    terminal: handle.terminal,
                    error: handle.error.clone(),
                });
                guard.inner.remove(run_id);
            }
        }
        if let Some(rec) = retained {
            guard.retain_cursor(run_id, rec);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn live_text_delta(run_id: &str, live_sequence: u64, text: &str) -> RunStreamFrameV2 {
        RunStreamFrameV2::Event {
            lane: RunStreamLane::Live,
            run_id: run_id.to_string(),
            durable_sequence: None,
            live_sequence: Some(live_sequence),
            event_type: "text_delta".to_string(),
            payload: json!({ "text": text }),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn durable_frame(run_id: &str, durable_sequence: u64, event_type: &str) -> RunStreamFrameV2 {
        RunStreamFrameV2::Event {
            lane: RunStreamLane::Durable,
            run_id: run_id.to_string(),
            durable_sequence: Some(durable_sequence),
            live_sequence: None,
            event_type: event_type.to_string(),
            payload: json!({}),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn register_handle(state: &Arc<Mutex<WatchBridgeState>>, run_id: &str) -> Arc<Notify> {
        let mut guard = state.lock().unwrap();
        let handle = WatchHandle::new();
        let cancel = handle.cancel.clone();
        guard.inner.insert(run_id.to_string(), handle);
        cancel
    }

    #[test]
    fn live_text_delta_advances_live_cursor_but_never_durable() {
        let state = Arc::new(Mutex::new(WatchBridgeState::default()));
        register_handle(&state, "run-1");
        // A real RunWatchStreamV2 live TextDelta frame arrives on the stream.
        update_handle(
            &state,
            "run-1",
            &live_text_delta("run-1", 101, "Hello"),
            false,
        );
        let snap = snapshot_state(&state.lock().unwrap(), "run-1");
        assert_eq!(snap["active"], true);
        assert_eq!(
            snap["lastDurableSequence"].as_u64(),
            Some(0),
            "live lane must never advance the durable cursor"
        );
        assert_eq!(
            snap["lastLiveSequence"].as_u64(),
            Some(101),
            "live cursor advanced to the TextDelta live_sequence"
        );
    }

    #[test]
    fn durable_completion_advances_durable_cursor_and_marks_terminal() {
        let state = Arc::new(Mutex::new(WatchBridgeState::default()));
        register_handle(&state, "run-1");
        update_handle(
            &state,
            "run-1",
            &durable_frame("run-1", 7, "message_completed"),
            false,
        );
        update_handle(
            &state,
            "run-1",
            &durable_frame("run-1", 9, "completed"),
            true,
        );
        let snap = snapshot_state(&state.lock().unwrap(), "run-1");
        assert_eq!(snap["lastDurableSequence"].as_u64(), Some(9));
        assert_eq!(snap["lastLiveSequence"].as_u64(), Some(0));
        assert_eq!(snap["terminal"], true);
        assert_eq!(snap["active"], false, "terminal watch is no longer active");
    }

    #[test]
    fn heartbeat_advances_both_cursors_and_records_idle_origin() {
        let state = Arc::new(Mutex::new(WatchBridgeState::default()));
        register_handle(&state, "run-1");
        update_handle(
            &state,
            "run-1",
            &RunStreamFrameV2::heartbeat("run-1", 42, 500),
            false,
        );
        let snap = snapshot_state(&state.lock().unwrap(), "run-1");
        assert_eq!(snap["lastDurableSequence"].as_u64(), Some(42));
        assert_eq!(snap["lastLiveSequence"].as_u64(), Some(500));
        assert!(
            snap["lastHeartbeatMs"].is_number(),
            "heartbeat must record lastHeartbeatAt"
        );
    }

    #[test]
    fn ended_task_retains_dual_cursors_for_reconnect_recovery() {
        let state = Arc::new(Mutex::new(WatchBridgeState::default()));
        let cancel = register_handle(&state, "run-1");
        update_handle(&state, "run-1", &live_text_delta("run-1", 101, "Hi"), false);
        update_handle(
            &state,
            "run-1",
            &durable_frame("run-1", 9, "message_completed"),
            false,
        );
        // Stream closes → the task tears down and snapshots its cursors.
        remove_handle(&state, "run-1", &cancel);
        let snap = snapshot_state(&state.lock().unwrap(), "run-1");
        assert_eq!(snap["active"], false);
        assert_eq!(snap["lastDurableSequence"].as_u64(), Some(9));
        assert_eq!(snap["lastLiveSequence"].as_u64(), Some(101));
        assert!(!state.lock().unwrap().inner.contains_key("run-1"));
    }

    #[test]
    fn stale_task_cannot_remove_replaced_handle() {
        let state = Arc::new(Mutex::new(WatchBridgeState::default()));
        let old_cancel = register_handle(&state, "run-1");
        // Idempotent restart: `run_watch_start` replaces the handle.
        let new_cancel = register_handle(&state, "run-1");
        // The OLD task finally observes its cancel and tears down.
        remove_handle(&state, "run-1", &old_cancel);
        let guard = state.lock().unwrap();
        assert!(
            guard.inner.contains_key("run-1"),
            "stale task removed the replaced (new) handle"
        );
        assert!(
            Arc::ptr_eq(&guard.inner.get("run-1").unwrap().cancel, &new_cancel),
            "the surviving handle must be the new one"
        );
    }

    #[test]
    fn cursor_record_eviction_is_bounded() {
        let state = Arc::new(Mutex::new(WatchBridgeState::default()));
        for i in 0..(CURSOR_RECORD_CAP as u64 + 10) {
            let run_id = format!("r-{i}");
            let cancel = register_handle(&state, &run_id);
            remove_handle(&state, &run_id, &cancel);
        }
        let guard = state.lock().unwrap();
        assert!(guard.last_cursors.len() <= CURSOR_RECORD_CAP);
        assert!(guard.cursor_order.len() <= CURSOR_RECORD_CAP);
    }

    #[tokio::test]
    async fn open_stream_fails_closed_in_embedded_mode() {
        std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
        std::env::remove_var("NATIVES_DAEMON_BOOTSTRAP");
        std::env::remove_var("NATIVES_DAEMON_SOCKET");
        let err = open_stream("run-x", 0, 0).await;
        let message = err
            .err()
            .expect("embedded mode must fail, no silent fallback");
        assert!(
            message.contains("UDS"),
            "embedded must fail closed naming UDS, got: {message}"
        );
    }
}
