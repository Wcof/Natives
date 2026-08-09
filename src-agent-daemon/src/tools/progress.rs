//! Progress batching: the batched terminal-output sink and terminal output delta emission.

use agent_core::{LiveEventBus, ToolProgressSink, ToolProgressUpdate};
use assistant_protocol::v2::RunEventKind;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Batched progress sink for tool output deltas.
///
/// ToolOutputDelta is a **live-lane** event (STREAM-CONTRACT-V2): it is emitted
/// on the shared [`LiveEventBus`] and never written to the durable
/// `EventSequencer`/SQLite event log. Batching (8KiB / 250ms) and
/// late-drop-after-settle semantics are unchanged.
pub struct DaemonToolProgressSink {
    live: LiveEventBus,
    settled: Arc<Mutex<HashSet<String>>>,
    pending: Arc<Mutex<HashMap<String, (Instant, ToolProgressUpdate)>>>,
    scheduled_flushes: Arc<Mutex<HashSet<String>>>,
    sequence: Arc<AtomicU64>,
}

impl DaemonToolProgressSink {
    pub fn new(live: LiveEventBus) -> Self {
        Self {
            live,
            settled: Arc::new(Mutex::new(HashSet::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            scheduled_flushes: Arc::new(Mutex::new(HashSet::new())),
            sequence: Arc::new(AtomicU64::new(1)),
        }
    }

    fn schedule_flush(&self, call_id: String) {
        let pending = self.pending.clone();
        let settled = self.settled.clone();
        let scheduled_flushes = self.scheduled_flushes.clone();
        let live = self.live.clone();
        let sequence = self.sequence.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let update = {
                let mut pending = pending.lock().await;
                let settled = settled.lock().await;
                let update = if settled.contains(&call_id) {
                    pending.remove(&call_id).map(|(_, value)| value)
                } else {
                    pending.remove(&call_id).map(|(_, mut value)| {
                        value.final_update = false;
                        value
                    })
                };
                // Keep the same pending → settled → scheduled lock order as
                // publish/mark_tool_call_settled. This closes the race where a
                // new update arrives while the timer is flushing the old batch.
                let mut scheduled = scheduled_flushes.lock().await;
                scheduled.remove(&call_id);
                update
            };
            if let Some(update) = update {
                let progress_sequence = sequence.fetch_add(1, AtomicOrdering::Relaxed);
                live.append(
                    &update.run_id,
                    RunEventKind::ToolOutputDelta {
                        tool_call_id: update.tool_call_id,
                        tool_name: Some(update.tool_name),
                        stream: update.stream,
                        text: update.text,
                        truncated: false,
                        turn_id: update.turn_id,
                        message_id: update.message_id,
                        progress_sequence: Some(progress_sequence),
                    },
                );
            }
        });
    }
}

#[async_trait::async_trait]
impl ToolProgressSink for DaemonToolProgressSink {
    async fn publish(&self, update: ToolProgressUpdate) {
        const MAX_BATCH_BYTES: usize = 8 * 1024;
        const MAX_BATCH_AGE: Duration = Duration::from_millis(250);
        let now = Instant::now();
        let call_id = update.tool_call_id.clone();
        let (emit, schedule) = {
            let mut pending = self.pending.lock().await;
            let settled = self.settled.lock().await;
            if settled.contains(&call_id) {
                return;
            }
            if update.final_update {
                let mut final_update = pending.remove(&call_id).map(|(_, mut value)| {
                    value.text.push_str(&update.text);
                    value.final_update = true;
                    value
                });
                if final_update.is_none() {
                    final_update = Some(update);
                }
                (final_update, false)
            } else {
                let flush = pending
                    .get_mut(&call_id)
                    .map(|(started, buffered)| {
                        buffered.text.push_str(&update.text);
                        buffered.text.len() >= MAX_BATCH_BYTES
                            || now.duration_since(*started) >= MAX_BATCH_AGE
                    })
                    .unwrap_or(false);
                if flush {
                    (pending.remove(&call_id).map(|(_, value)| value), false)
                } else if pending.contains_key(&call_id) {
                    (None, false)
                } else {
                    pending.insert(call_id.clone(), (now, update));
                    let mut scheduled = self.scheduled_flushes.lock().await;
                    (None, scheduled.insert(call_id.clone()))
                }
            }
        };
        if schedule {
            self.schedule_flush(call_id);
        }
        if let Some(update) = emit {
            let progress_sequence = self.sequence.fetch_add(1, AtomicOrdering::Relaxed);
            self.live.append(
                &update.run_id,
                RunEventKind::ToolOutputDelta {
                    tool_call_id: update.tool_call_id,
                    tool_name: Some(update.tool_name),
                    stream: update.stream,
                    text: update.text,
                    truncated: false,
                    turn_id: update.turn_id,
                    message_id: update.message_id,
                    progress_sequence: Some(progress_sequence),
                },
            );
        }
    }

    async fn mark_tool_call_settled(&self, tool_call_id: &str) {
        // Keep the same lock order as `publish`: either the event is appended
        // before settlement, or settlement wins and the late update is dropped.
        let mut pending = self.pending.lock().await;
        let mut settled = self.settled.lock().await;
        let mut scheduled = self.scheduled_flushes.lock().await;
        settled.insert(tool_call_id.to_string());
        pending.remove(tool_call_id);
        // Drain any scheduled flush task for this call so progress work is
        // immediately zero after settle (T04); a flush already running is a
        // no-op because `settled` is set and `pending` is empty.
        scheduled.remove(tool_call_id);
    }
}

/// Emit batched terminal stdout/stderr as ToolOutputDelta (≤8KB chunks, ≤1MB total).
///
/// ToolOutputDelta is live-lane only (STREAM-CONTRACT-V2): chunks go to the
/// shared [`LiveEventBus`], never the durable EventSequencer.
pub(crate) fn emit_terminal_output_deltas(
    live: &LiveEventBus,
    run_id: &str,
    tool_call_id: &str,
    result: &Value,
) {
    const CHUNK: usize = 8 * 1024;
    const MAX_PERSIST: usize = 1024 * 1024;
    let stdout = result
        .get("stdout")
        .and_then(|v| v.as_str())
        .or_else(|| result.get("output").and_then(|v| v.as_str()))
        .unwrap_or("");
    let stderr = result.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
    let mut persisted = 0usize;
    for (stream, text) in [("stdout", stdout), ("stderr", stderr)] {
        if text.is_empty() {
            continue;
        }
        let bytes = text.as_bytes();
        let mut offset = 0usize;
        while offset < bytes.len() {
            if persisted >= MAX_PERSIST {
                live.append(
                    run_id,
                    RunEventKind::ToolOutputDelta {
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: Some("run_terminal".into()),
                        stream: stream.into(),
                        text: String::new(),
                        truncated: true,
                        turn_id: None,
                        message_id: None,
                        progress_sequence: None,
                    },
                );
                return;
            }
            let end = (offset + CHUNK).min(bytes.len());
            let take = (end - offset).min(MAX_PERSIST - persisted);
            let end = offset + take;
            let chunk = String::from_utf8_lossy(&bytes[offset..end]).into_owned();
            persisted += chunk.len();
            live.append(
                run_id,
                RunEventKind::ToolOutputDelta {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: Some("run_terminal".into()),
                    stream: stream.into(),
                    text: chunk,
                    truncated: persisted >= MAX_PERSIST || end < bytes.len() && take < CHUNK,
                    turn_id: None,
                    message_id: None,
                    progress_sequence: None,
                },
            );
            offset = end;
            if take == 0 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn settled_tool_drops_late_progress() {
        let live = LiveEventBus::new();
        let sink = DaemonToolProgressSink::new(live.clone());
        let update = ToolProgressUpdate {
            run_id: "progress-run".into(),
            tool_call_id: "progress-call".into(),
            tool_name: "run_terminal".into(),
            stream: "stdout".into(),
            text: "before settlement".into(),
            final_update: true,
            turn_id: Some("turn-1".into()),
            message_id: Some("message-1".into()),
            progress_sequence: 0,
        };
        sink.publish(update.clone()).await;
        sink.mark_tool_call_settled(&update.tool_call_id).await;
        sink.publish(update).await;

        // ToolOutputDelta is live-lane only: it lands on the LiveEventBus.
        let sub = live.subscribe_after("progress-run", 0);
        assert!(!sub.gap, "fresh bus must not report a gap");
        assert_eq!(sub.buffered.len(), 1);
        assert!(matches!(
            sub.buffered[0].kind,
            RunEventKind::ToolOutputDelta { .. }
        ));
    }

    #[tokio::test]
    async fn progress_flushes_after_batch_window_without_next_update() {
        let live = LiveEventBus::new();
        let sink = DaemonToolProgressSink::new(live.clone());
        sink.publish(ToolProgressUpdate {
            run_id: "progress-timer-run".into(),
            tool_call_id: "progress-timer-call".into(),
            tool_name: "run_terminal".into(),
            stream: "stdout".into(),
            text: "idle batch".into(),
            final_update: false,
            turn_id: Some("turn-1".into()),
            message_id: Some("message-1".into()),
            progress_sequence: 0,
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let sub = live.subscribe_after("progress-timer-run", 0);
        assert!(sub.buffered.iter().any(|event| matches!(
            event.kind,
            RunEventKind::ToolOutputDelta { ref text, .. } if text == "idle batch"
        )));
    }
    /// TASK-007 (H03): after a tool call settles, a flood of late progress
    /// updates is rejected — zero events appended (terminal is authoritative).
    #[tokio::test]
    async fn progress_backpressure_rejects_updates_after_terminal() {
        let live = LiveEventBus::new();
        let sink = DaemonToolProgressSink::new(live.clone());
        sink.mark_tool_call_settled("late-call").await;
        for i in 0..100 {
            sink.publish(ToolProgressUpdate {
                run_id: "bp-run".into(),
                tool_call_id: "late-call".into(),
                tool_name: "run_terminal".into(),
                stream: "stdout".into(),
                text: format!("late line {i}"),
                final_update: false,
                turn_id: None,
                message_id: None,
                progress_sequence: 0,
            })
            .await;
        }
        assert_eq!(
            live.buffered_len("bp-run"),
            0,
            "no progress event may be appended after the call settles"
        );
    }
    /// T04: settling an MCP call must drain the progress sink's pending buffer
    /// and scheduled flush tasks — after a cancel no progress work is left.
    #[tokio::test]
    async fn mcp_settle_drains_progress_tasks_to_zero() {
        let sink = DaemonToolProgressSink::new(LiveEventBus::new());
        // A pending non-final update buffers into the sink and schedules a flush.
        sink.publish(ToolProgressUpdate {
            run_id: "drain-run".into(),
            tool_call_id: "drain-call".into(),
            tool_name: "mcp_call".into(),
            stream: "mcp".into(),
            text: "pending delta".into(),
            final_update: false,
            turn_id: Some("turn-1".into()),
            message_id: Some("message-1".into()),
            progress_sequence: 0,
        })
        .await;
        assert!(
            !sink.pending.lock().await.is_empty(),
            "buffered progress is pending before settle"
        );
        sink.mark_tool_call_settled("drain-call").await;
        assert!(
            sink.pending.lock().await.is_empty(),
            "pending buffer drained after settle"
        );
        assert!(
            sink.scheduled_flushes.lock().await.is_empty(),
            "no scheduled flush task remains after settle"
        );
        // A late update after settle is rejected entirely.
        sink.publish(ToolProgressUpdate {
            run_id: "drain-run".into(),
            tool_call_id: "drain-call".into(),
            tool_name: "mcp_call".into(),
            stream: "mcp".into(),
            text: "late".into(),
            final_update: false,
            turn_id: None,
            message_id: None,
            progress_sequence: 0,
        })
        .await;
        assert!(
            sink.pending.lock().await.is_empty(),
            "late update must not repopulate the buffer"
        );
    }
}
