//! Live (in-memory, ephemeral) event bus for high-frequency deltas.
//!
//! Contract: `docs/contracts/STREAM-CONTRACT-V2.md` (RunWatchStreamV2 live lane)
//! and `.contracts/live-durable-event-contract.md`
//!
//! Live events are memory-first, bounded-broadcast, and **never** touch the
//! durable SQLite event store. They may be lost on process crash; reconnect
//! returns to the last durable fact boundary.
//!
//! Variants routed here:
//! - `TextDelta`
//! - `ReasoningDelta`
//! - `ToolCallDelta`
//! - `ToolOutputDelta`
//! - high-frequency `Progress`
//!
//! The durable [`crate::event_seq::EventSequencer`] remains the authority for
//! committed facts (run lifecycle, message started/completed, tool
//! requested/prepared/started/completed, permission/interaction, checkpoint,
//! context snapshot, usage summary, terminal).
//!
//! # Frozen API (STREAM-CONTRACT-V2)
//!
//! - `subscribe_after(run_id, after_live_sequence) -> LiveSubscription`:
//!   atomically returns buffered events after the cursor plus a live receiver.
//!   If the cursor already fell before the bounded buffer start, `gap = true`.
//! - `remove_run(run_id)`: drops per-run ring state after terminal.
//! - `append` returns the monotonic per-run `live_sequence`.
//!
//! Bound: at most `RING_MAX_EVENTS` events and `RING_MAX_BYTES` bytes per run;
//! whichever limit is hit first evicts the oldest live events.

use assistant_protocol::v2::RunEventKind;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Bounded broadcast channel capacity per run.
///
/// Live deltas are ephemeral; if a subscriber falls behind the oldest delta is
/// silently dropped (broadcast lag). This keeps the hot path wait-free with
/// respect to disk.
const LIVE_CHANNEL_CAPACITY: usize = 256;

/// Maximum buffered live events per run (bounded ring replay).
const RING_MAX_EVENTS: usize = 1024;

/// Maximum buffered live bytes per run (bounded ring replay).
const RING_MAX_BYTES: usize = 1024 * 1024;

/// Memory-only, bounded-broadcast bus for high-frequency live events.
///
/// Send+Sync+Clone-friendly (like [`crate::event_seq::EventSequencer`]): the
/// cheap `Clone` shares the single inner state.
#[derive(Clone, Default)]
pub struct LiveEventBus {
    inner: Arc<Mutex<LiveInner>>,
}

#[derive(Default)]
struct LiveInner {
    /// Per-run ephemeral state (sequence + bus + bounded ring replay).
    runs: HashMap<String, RunLiveState>,
}

/// Per-run live state: monotonic sequence, broadcast bus, bounded ring replay.
struct RunLiveState {
    sequence: u64,
    sender: broadcast::Sender<LiveEvent>,
    ring: Vec<LiveEvent>,
    ring_bytes: usize,
}

impl Default for RunLiveState {
    fn default() -> Self {
        Self {
            sequence: 0,
            sender: broadcast::channel(LIVE_CHANNEL_CAPACITY).0,
            ring: Vec::new(),
            ring_bytes: 0,
        }
    }
}

/// A single live event on the ephemeral bus.
///
/// `live_sequence` is per-run, monotonic, and independent of the durable
/// `run_sequence`. UI ordering uses `{ durable_sequence_anchor, live_sequence }`.
#[derive(Debug, Clone)]
pub struct LiveEvent {
    pub run_id: String,
    pub live_sequence: u64,
    pub kind: RunEventKind,
}

/// Result of an atomic `subscribe_after` call.
#[derive(Debug)]
pub struct LiveSubscription {
    /// Events already buffered after the requested cursor (bounded replay).
    pub buffered: Vec<LiveEvent>,
    /// `true` when the requested cursor fell before the buffer start, meaning
    /// some live events are unrecoverable (they may be re-derived from durable
    /// facts or resynced). Not a durable-fact gap.
    pub gap: bool,
    /// Live receiver for events appended after this call.
    pub receiver: broadcast::Receiver<LiveEvent>,
    /// Current last live sequence at subscription time.
    pub last_sequence: u64,
}

impl LiveEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Approximate serialized byte size of a live event for the ring cap.
    fn event_bytes(kind: &RunEventKind) -> usize {
        match serde_json::to_string(kind) {
            Ok(s) => s.len() + 64,
            Err(_) => 128,
        }
    }

    /// Append a live event to the ephemeral bus.
    ///
    /// Returns the assigned `live_sequence`. **No persistence, no SQLite wait.**
    /// If no subscriber is connected the send is a no-op (broadcast returns
    /// `Err` for lag/no-receivers, which we ignore).
    pub fn append(&self, run_id: &str, kind: RunEventKind) -> u64 {
        let mut inner = self.inner.lock().expect("live event bus lock");
        let state = inner.runs.entry(run_id.to_string()).or_default();
        state.sequence += 1;
        let live_sequence = state.sequence;
        let event = LiveEvent {
            run_id: run_id.to_string(),
            live_sequence,
            kind,
        };
        // Bounded ring replay: evict oldest until both caps hold.
        let event_bytes = Self::event_bytes(&event.kind);
        state.ring.push(event.clone());
        state.ring_bytes += event_bytes;
        while state.ring.len() > RING_MAX_EVENTS || state.ring_bytes > RING_MAX_BYTES {
            if let Some(oldest) = state.ring.first() {
                state.ring_bytes = state
                    .ring_bytes
                    .saturating_sub(Self::event_bytes(&oldest.kind));
            }
            state.ring.remove(0);
        }
        // Ignore send errors: lagged or no receivers — live deltas may drop.
        let _ = state.sender.send(event);
        live_sequence
    }

    /// Subscribe to the live event stream for a run (compat: no replay).
    pub fn subscribe(&self, run_id: &str) -> broadcast::Receiver<LiveEvent> {
        let mut inner = self.inner.lock().expect("live event bus lock");
        let state = inner.runs.entry(run_id.to_string()).or_default();
        state.sender.subscribe()
    }

    /// Atomically replay buffered events after `after_live_sequence` and attach
    /// a live receiver. If the cursor fell before the buffer start, `gap=true`.
    ///
    /// Frozen for S2 (RunWatchStreamV2): the server establishes the live
    /// receiver *before* the RPC ACK so no replay→subscribe window is lost.
    pub fn subscribe_after(&self, run_id: &str, after_live_sequence: u64) -> LiveSubscription {
        let mut inner = self.inner.lock().expect("live event bus lock");
        let state = inner.runs.entry(run_id.to_string()).or_default();
        let receiver = state.sender.subscribe();
        let last_sequence = state.sequence;
        let mut gap = false;
        let buffered: Vec<LiveEvent> = state
            .ring
            .iter()
            .filter(|event| event.live_sequence > after_live_sequence)
            .cloned()
            .collect();
        if after_live_sequence > 0 {
            if let Some(first) = state.ring.first() {
                // Cursor older than the oldest retained event ⇒ unrecoverable live prefix.
                if first.live_sequence > after_live_sequence + 1 {
                    gap = true;
                }
            } else if state.sequence > after_live_sequence {
                // Run has emitted events but the ring is already gone (terminal cleanup).
                gap = true;
            }
        }
        LiveSubscription {
            buffered,
            gap,
            receiver,
            last_sequence,
        }
    }

    /// Drop per-run bus + ring state after terminal (see STREAM-CONTRACT-V2
    /// Terminal). New subscribers on a removed run get a fresh empty bus.
    pub fn remove_run(&self, run_id: &str) {
        let mut inner = self.inner.lock().expect("live event bus lock");
        inner.runs.remove(run_id);
    }

    /// Current ephemeral per-run sequence (for tests / observability).
    pub fn last_sequence(&self, run_id: &str) -> u64 {
        let inner = self.inner.lock().expect("live event bus lock");
        inner.runs.get(run_id).map_or(0, |s| s.sequence)
    }

    /// Number of buffered live events for a run (metrics / tests).
    pub fn buffered_len(&self, run_id: &str) -> usize {
        let inner = self.inner.lock().expect("live event bus lock");
        inner.runs.get(run_id).map_or(0, |s| s.ring.len())
    }

    /// Approximate buffered live bytes for a run (metrics / tests).
    pub fn buffered_bytes(&self, run_id: &str) -> usize {
        let inner = self.inner.lock().expect("live event bus lock");
        inner.runs.get(run_id).map_or(0, |s| s.ring_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_bus_delivers_deltas_without_persistence() {
        let bus = LiveEventBus::new();
        let mut rx = bus.subscribe("run-1");
        let first = bus.append("run-1", RunEventKind::TextDelta { text: "a".into() });
        let second = bus.append("run-1", RunEventKind::TextDelta { text: "b".into() });
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        let e1 = rx.recv().await.unwrap();
        let e2 = rx.recv().await.unwrap();
        assert_eq!(e1.live_sequence, 1);
        assert_eq!(e2.live_sequence, 2);
        assert_eq!(bus.last_sequence("run-1"), 2);
    }

    #[tokio::test]
    async fn live_bus_per_run_isolation() {
        let bus = LiveEventBus::new();
        let mut rx_a = bus.subscribe("run-a");
        let _ = bus.append("run-a", RunEventKind::TextDelta { text: "x".into() });
        let _ = bus.append("run-b", RunEventKind::TextDelta { text: "y".into() });
        let e = rx_a.recv().await.unwrap();
        assert_eq!(e.run_id, "run-a");
        assert_eq!(bus.last_sequence("run-b"), 1);
    }

    #[tokio::test]
    async fn subscribe_after_replays_bounded_prefix_without_gap() {
        let bus = LiveEventBus::new();
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "a".into() });
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "b".into() });
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "c".into() });
        let sub = bus.subscribe_after("run-1", 1);
        assert!(!sub.gap, "cursor 1 must still be in buffer");
        assert_eq!(sub.last_sequence, 3);
        let texts: Vec<_> = sub
            .buffered
            .iter()
            .filter_map(|e| match &e.kind {
                RunEventKind::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["b", "c"]);
    }

    #[tokio::test]
    async fn subscribe_after_at_zero_replays_full_prefix() {
        let bus = LiveEventBus::new();
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "a".into() });
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "b".into() });
        let sub = bus.subscribe_after("run-1", 0);
        assert!(!sub.gap);
        assert_eq!(sub.buffered.len(), 2);
    }

    #[tokio::test]
    async fn subscribe_after_detects_gap_when_cursor_fell_before_buffer() {
        let bus = LiveEventBus::new();
        for i in 0..(RING_MAX_EVENTS + 64) as u64 {
            let _ = bus.append(
                "run-1",
                RunEventKind::TextDelta {
                    text: i.to_string(),
                },
            );
        }
        // Cursor 1 is far before the ring start ⇒ live gap.
        let sub = bus.subscribe_after("run-1", 1);
        assert!(sub.gap, "cursor before ring start must report gap");
        assert_eq!(bus.buffered_len("run-1"), RING_MAX_EVENTS);
    }

    #[tokio::test]
    async fn ring_is_bounded_by_bytes_cap() {
        let bus = LiveEventBus::new();
        let big = RunEventKind::TextDelta {
            text: "x".repeat(200_000),
        };
        // 8 × 200KiB > 1MiB byte cap ⇒ ring must evict.
        for _ in 0..8 {
            let _ = bus.append("run-1", big.clone());
        }
        assert!(
            bus.buffered_bytes("run-1") <= RING_MAX_BYTES,
            "byte cap violated: {}",
            bus.buffered_bytes("run-1")
        );
        assert!(bus.buffered_len("run-1") < 8);
    }

    #[tokio::test]
    async fn remove_run_clears_live_state() {
        let bus = LiveEventBus::new();
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "a".into() });
        bus.remove_run("run-1");
        assert_eq!(bus.last_sequence("run-1"), 0);
        assert_eq!(bus.buffered_len("run-1"), 0);
        assert_eq!(bus.buffered_bytes("run-1"), 0);
        // New subscriber after terminal gets a fresh empty bus.
        let sub = bus.subscribe_after("run-1", 0);
        assert!(sub.buffered.is_empty());
        assert!(!sub.gap);
    }

    #[tokio::test]
    async fn subscribe_after_live_receiver_continues_after_replay() {
        let bus = LiveEventBus::new();
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "a".into() });
        let mut sub = bus.subscribe_after("run-1", 0);
        assert_eq!(sub.buffered.len(), 1);
        let _ = bus.append("run-1", RunEventKind::TextDelta { text: "b".into() });
        let e = sub.receiver.recv().await.unwrap();
        assert_eq!(e.live_sequence, 2);
    }
}
