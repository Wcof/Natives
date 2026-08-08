//! RunWatchStreamV2 — persistent daemon→host wire protocol.
//!
//! Contract: `docs/contracts/STREAM-CONTRACT-V2.md`.
//!
//! The stream runs over the same UDS connection that carried the `run.watch`
//! RPC. The first line is a **normal** RPC ACK:
//!
//! ```json
//! { "success": true, "data": { "stream": "run.watch", "streamVersion": 2 } }
//! ```
//!
//! Every subsequent line is one [`RunStreamFrameV2`] (newline-delimited JSON):
//!
//! - `Event { lane: durable|live, run_id, durable_sequence, live_sequence,
//!   event_type, payload, timestamp }`
//! - `Heartbeat { run_id, durable_sequence, live_sequence, timestamp }`
//! - `ResyncRequired { run_id, lane, reason }`
//!
//! Ordering rules (frozen):
//! - The durable lane is monotonic and replayable.
//! - The live lane is monotonic but only a bounded in-memory replay.
//! - The two lanes share **no** sequence namespace — a live cursor never
//!   advances the durable projection watermark.
//!
//! The module is registered by the main Agent (`pub mod stream_protocol;` in
//! `lib.rs`) — see NEEDS-INTEGRATION in the S2 handoff.

use serde::{Deserialize, Serialize};

/// Method name for the RunWatchStreamV2 RPC (stable wire name).
pub const STREAM_METHOD: &str = "run.watch";

/// Wire version advertised in the ACK (`data.streamVersion`).
pub const STREAM_VERSION: u64 = 2;

/// Lane selector on the RunWatchStreamV2 frame stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStreamLane {
    Durable,
    Live,
}

impl RunStreamLane {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStreamLane::Durable => "durable",
            RunStreamLane::Live => "live",
        }
    }
}

/// One frame on the persistent `run.watch` stream.
///
/// Serialized shape follows `STREAM-CONTRACT-V2.md` exactly: `frame_type` tags
/// the variant; the durable/live sequence fields are always present with the
/// non-applicable one set to `null` (no `skip_serializing_if`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "frame_type", rename_all = "snake_case")]
pub enum RunStreamFrameV2 {
    Event {
        lane: RunStreamLane,
        run_id: String,
        /// Durable per-run sequence; always `null` on the live lane.
        #[serde(default)]
        durable_sequence: Option<u64>,
        /// Ephemeral per-run live sequence; always `null` on the durable lane.
        #[serde(default)]
        live_sequence: Option<u64>,
        event_type: String,
        payload: serde_json::Value,
        timestamp: String,
    },
    Heartbeat {
        run_id: String,
        durable_sequence: u64,
        live_sequence: u64,
        timestamp: String,
    },
    ResyncRequired {
        run_id: String,
        lane: RunStreamLane,
        reason: String,
    },
}

impl RunStreamFrameV2 {
    /// Build a durable-lane event frame from a committed [`RunEventV2`].
    pub fn durable_event(run_id: &str, event: &assistant_protocol::v2::RunEventV2) -> Self {
        Self::Event {
            lane: RunStreamLane::Durable,
            run_id: run_id.to_string(),
            durable_sequence: Some(event.effective_run_sequence()),
            live_sequence: None,
            event_type: event.payload.type_name().to_string(),
            payload: serde_json::to_value(&event.payload).unwrap_or_default(),
            timestamp: event.timestamp.to_rfc3339(),
        }
    }

    /// Build a live-lane event frame from an ephemeral [`agent_core::LiveEvent`].
    ///
    /// Live events carry no durable sequence — they are transient and never
    /// advance the durable projection watermark.
    pub fn live_event(run_id: &str, event: &agent_core::LiveEvent) -> Self {
        Self::Event {
            lane: RunStreamLane::Live,
            run_id: run_id.to_string(),
            durable_sequence: None,
            live_sequence: Some(event.live_sequence),
            event_type: event.kind.type_name().to_string(),
            payload: serde_json::to_value(&event.kind).unwrap_or_default(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Build a heartbeat frame. Sent on long idle so the 30s client frame
    /// timeout never fires on a healthy stream.
    pub fn heartbeat(run_id: &str, durable_sequence: u64, live_sequence: u64) -> Self {
        Self::Heartbeat {
            run_id: run_id.to_string(),
            durable_sequence,
            live_sequence,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Build a resync frame. The live-lane gap variant tells the receiver that
    /// some ephemeral live deltas are unrecoverable; the durable lane is never
    /// resynced from the live bus.
    pub fn resync_required(run_id: &str, lane: RunStreamLane, reason: impl Into<String>) -> Self {
        Self::ResyncRequired {
            run_id: run_id.to_string(),
            lane,
            reason: reason.into(),
        }
    }

    /// Stable wire `frame_type` for this frame.
    pub fn frame_type(&self) -> &'static str {
        match self {
            Self::Event { .. } => "event",
            Self::Heartbeat { .. } => "heartbeat",
            Self::ResyncRequired { .. } => "resync_required",
        }
    }
}

/// The ACK data payload of a successful `run.watch` RPC (`STREAM-CONTRACT-V2`).
pub fn stream_ack() -> serde_json::Value {
    serde_json::json!({
        "stream": STREAM_METHOD,
        "streamVersion": STREAM_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_event_serializes_to_contract_shape() {
        let event = assistant_protocol::v2::RunEventV2::new(
            "run-123",
            43,
            assistant_protocol::v2::RunEventKind::MessageCompleted {
                turn_id: "t1".into(),
                message_id: "m1".into(),
                role: "assistant".into(),
                content: None,
            },
        );
        let frame = RunStreamFrameV2::durable_event("run-123", &event);
        let json = serde_json::to_value(&frame).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj["frame_type"], "event");
        assert_eq!(obj["lane"], "durable");
        assert_eq!(obj["run_id"], "run-123");
        assert_eq!(obj["durable_sequence"], 43);
        assert!(obj["live_sequence"].is_null());
        assert_eq!(obj["event_type"], "message_completed");
        assert_eq!(obj["payload"]["type"], "message_completed");
        assert_eq!(obj["payload"]["message_id"], "m1");
        assert!(obj["timestamp"].as_str().unwrap().contains('T'));
    }

    #[test]
    fn live_event_serializes_to_contract_shape() {
        let live = agent_core::LiveEvent {
            run_id: "run-123".into(),
            live_sequence: 121,
            kind: assistant_protocol::v2::RunEventKind::TextDelta { text: "hi".into() },
        };
        let frame = RunStreamFrameV2::live_event("run-123", &live);
        let json = serde_json::to_value(&frame).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj["frame_type"], "event");
        assert_eq!(obj["lane"], "live");
        assert_eq!(obj["durable_sequence"].as_u64(), None);
        assert!(obj["durable_sequence"].is_null());
        assert_eq!(obj["live_sequence"], 121);
        assert_eq!(obj["event_type"], "text_delta");
        assert_eq!(obj["payload"]["text"], "hi");
    }

    #[test]
    fn heartbeat_and_resync_serialize_to_contract_shape() {
        let hb = RunStreamFrameV2::heartbeat("run-123", 43, 130);
        let hb_obj = serde_json::to_value(&hb).unwrap();
        assert_eq!(hb_obj["frame_type"], "heartbeat");
        assert_eq!(hb_obj["durable_sequence"], 43);
        assert_eq!(hb_obj["live_sequence"], 130);

        let rs =
            RunStreamFrameV2::resync_required("run-123", RunStreamLane::Live, "live_buffer_gap");
        let rs_obj = serde_json::to_value(&rs).unwrap();
        assert_eq!(rs_obj["frame_type"], "resync_required");
        assert_eq!(rs_obj["run_id"], "run-123");
        assert_eq!(rs_obj["lane"], "live");
        assert_eq!(rs_obj["reason"], "live_buffer_gap");
    }

    #[test]
    fn frames_round_trip_through_deserialize() {
        let event = assistant_protocol::v2::RunEventV2::new(
            "r",
            1,
            assistant_protocol::v2::RunEventKind::Started,
        );
        for frame in [
            RunStreamFrameV2::durable_event("r", &event),
            RunStreamFrameV2::live_event(
                "r",
                &agent_core::LiveEvent {
                    run_id: "r".into(),
                    live_sequence: 1,
                    kind: assistant_protocol::v2::RunEventKind::ReasoningDelta { text: "x".into() },
                },
            ),
            RunStreamFrameV2::heartbeat("r", 1, 1),
            RunStreamFrameV2::resync_required("r", RunStreamLane::Durable, "stream_closed"),
        ] {
            let json = serde_json::to_string(&frame).unwrap();
            let back: RunStreamFrameV2 = serde_json::from_str(&json).unwrap();
            assert_eq!(back.frame_type(), frame.frame_type());
        }
    }

    #[test]
    fn stream_ack_matches_contract() {
        let ack = stream_ack();
        assert_eq!(ack["stream"], "run.watch");
        assert_eq!(ack["streamVersion"], 2);
    }
}
