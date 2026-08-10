//! Wire replay bounding helpers (W9 split from rpc/handlers/run.rs).

use assistant_protocol::v2::RunEventV2;

// {A2-03} MAX_WIRE_REPLAY_EVENTS const (moved verbatim from rpc.rs)
/// Cap on how many run events a single `run.getEvents` / `run.replay` / `run.watch`
/// wire response carries (R-P4 / T11). A run's full history can exceed
/// `MAX_FRAME_BYTES` once serialized; the client pages forward through its
/// `last_sequence` cursor, and the Renderer keeps only a 2000-event window.
/// In-process callers (resume / subagent reconciliation) keep the full replay
/// through `replay_checked` — only the UDS boundary is bounded.
pub const MAX_WIRE_REPLAY_EVENTS: usize = 2000;

// {A2-03} cap_wire_replay (moved verbatim from rpc.rs)
/// Keep the oldest `MAX_WIRE_REPLAY_EVENTS` events of a replay batch
/// (ascending sequence order) so a bounded batch always advances the client
/// cursor monotonically.
pub(crate) fn cap_wire_replay(events: Vec<RunEventV2>) -> Vec<RunEventV2> {
    if events.len() <= MAX_WIRE_REPLAY_EVENTS {
        events
    } else {
        events.into_iter().take(MAX_WIRE_REPLAY_EVENTS).collect()
    }
}

// {A2-03} wire_replay_cap_tests mod (moved verbatim from rpc.rs)
#[cfg(test)]
mod wire_replay_cap_tests {
    use super::*;

    fn event(run_id: &str, seq: u64) -> RunEventV2 {
        RunEventV2 {
            event_id: format!("e-{seq}"),
            global_sequence: seq,
            run_id: run_id.to_string(),
            run_sequence: seq,
            timestamp: chrono::Utc::now(),
            payload: assistant_protocol::v2::RunEventKind::TextDelta {
                text: format!("d{seq}"),
            },
        }
    }

    #[test]
    fn cap_keeps_batches_within_bounds() {
        let small = (1..=10).map(|s| event("r", s)).collect::<Vec<_>>();
        assert_eq!(cap_wire_replay(small).len(), 10, "small batch is untouched");
        let big = (1..=(MAX_WIRE_REPLAY_EVENTS + 500) as u64)
            .map(|s| event("r", s))
            .collect::<Vec<_>>();
        let capped = cap_wire_replay(big);
        assert_eq!(capped.len(), MAX_WIRE_REPLAY_EVENTS);
        assert_eq!(
            capped.first().map(|e| e.run_sequence),
            Some(1),
            "oldest events are kept so the client cursor advances monotonically"
        );
        assert_eq!(
            capped.last().map(|e| e.run_sequence),
            Some(MAX_WIRE_REPLAY_EVENTS as u64)
        );
    }
}
