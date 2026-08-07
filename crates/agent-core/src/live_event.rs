//! Live (in-memory, ephemeral) event bus for high-frequency deltas.
//!
//! Contract: `.contracts/live-durable-event-contract.md`
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
    /// Monotonic ephemeral per-run sequence. Does NOT depend on SQLite.
    sequences: HashMap<String, u64>,
    buses: HashMap<String, broadcast::Sender<LiveEvent>>,
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

impl LiveEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a live event to the ephemeral bus.
    ///
    /// Returns the assigned `live_sequence`. **No persistence, no SQLite wait.**
    /// If no subscriber is connected the send is a no-op (broadcast returns
    /// `Err` for lag/no-receivers, which we ignore).
    pub fn append(&self, run_id: &str, kind: RunEventKind) -> u64 {
        let mut inner = self.inner.lock().expect("live event bus lock");
        let next = inner.sequences.entry(run_id.to_string()).or_insert(0);
        *next += 1;
        let live_sequence = *next;
        let sender = inner
            .buses
            .entry(run_id.to_string())
            .or_insert_with(|| broadcast::channel(LIVE_CHANNEL_CAPACITY).0);
        let event = LiveEvent {
            run_id: run_id.to_string(),
            live_sequence,
            kind,
        };
        // Ignore send errors: lagged or no receivers — live deltas may drop.
        let _ = sender.send(event);
        live_sequence
    }

    /// Subscribe to the live event stream for a run.
    pub fn subscribe(&self, run_id: &str) -> broadcast::Receiver<LiveEvent> {
        let mut inner = self.inner.lock().expect("live event bus lock");
        let sender = inner
            .buses
            .entry(run_id.to_string())
            .or_insert_with(|| broadcast::channel(LIVE_CHANNEL_CAPACITY).0);
        sender.subscribe()
    }

    /// Current ephemeral per-run sequence (for tests / observability).
    pub fn last_sequence(&self, run_id: &str) -> u64 {
        let inner = self.inner.lock().expect("live event bus lock");
        inner.sequences.get(run_id).copied().unwrap_or(0)
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
}
