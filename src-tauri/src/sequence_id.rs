use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    /// Global monotonic sequence counter for event ordering (KI-4).
    /// Incremented atomically before each `db-state-changed` emission.
    static ref GLOBAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    /// Cache of the last sequence ID sent to each module/view.
    /// Used to discard out-of-order or stale events.
    static ref LAST_SEQUENCE: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());
}

/// Get the next monotonic sequence ID.
pub fn next_sequence() -> u64 {
    GLOBAL_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

/// Record that a module/view has processed up to a given sequence ID.
pub fn update_last_sequence(session_id: &str, sequence: u64) {
    let mut cache = LAST_SEQUENCE.lock().unwrap();
    // Remove existing entry for this session
    cache.retain(|(id, _)| id != session_id);
    cache.push((session_id.to_string(), sequence));
}

/// Check if a sequence ID should be processed (i.e., is newer than the last seen).
/// Returns true if the event should be processed, false if it should be discarded.
pub fn should_process(session_id: &str, sequence: u64) -> bool {
    let cache = LAST_SEQUENCE.lock().unwrap();
    if let Some((_, last)) = cache.iter().find(|(id, _)| id == session_id) {
        sequence > *last
    } else {
        true
    }
}

/// Build a payload envelope with version and sequence for event emission.
pub fn envelope(channel: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "channel": channel,
        "data": data,
        "version": 1,
        "sequence": next_sequence(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_monotonic() {
        let a = next_sequence();
        let b = next_sequence();
        assert!(b > a, "Sequence must be monotonically increasing");
    }

    #[test]
    fn test_should_process_new_events() {
        let session = "test-session";
        let seq1 = next_sequence();
        
        // First event should always be processed
        assert!(should_process(session, seq1));
        
        // Record it
        update_last_sequence(session, seq1);
        
        // Same sequence should not be processed
        assert!(!should_process(session, seq1));
        
        // Older sequence should not be processed
        assert!(!should_process(session, seq1 - 1));
        
        // Newer sequence should be processed
        let seq2 = next_sequence();
        assert!(should_process(session, seq2));
    }

    #[test]
    fn test_different_sessions_independent() {
        let session_a = "session-a";
        let session_b = "session-b";
        
        let seq_a = next_sequence();
        let seq_b = next_sequence();
        
        update_last_sequence(session_a, seq_a);
        
        // Session B should still process even if its sequence is older than A's last
        assert!(should_process(session_b, seq_b));
        
        // Session A should not reprocess same
        assert!(!should_process(session_a, seq_a));
    }

    #[test]
    fn test_envelope_format() {
        let payload = envelope("module", serde_json::json!({"action": "update"}));
        assert_eq!(payload["channel"], "module");
        assert_eq!(payload["version"], 1);
        assert!(payload["sequence"].as_u64().is_some());
        assert_eq!(payload["data"]["action"], "update");
    }
}
