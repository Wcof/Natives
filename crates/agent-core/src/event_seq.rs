//! In-memory monotonic event sequence per run (persist-first seam).
//!
//! Persistence is **on by default** (full remediation §十三). Override with:
//! - `NATIVES_EVENT_LOG_DIR` — explicit directory
//! - `NATIVES_RUNTIME_DIR/events` — when runtime dir is set
//! - `~/.natives/events` — default home path
//! - `NATIVES_EVENT_LOG_DISABLE=1` — memory-only (tests / explicit opt-out)

use assistant_protocol::v2::{redact_secrets, RunEventKind, RunEventV2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Append-only event log with fan-out subscribers.
///
/// Default durability: JSONL under the resolved event log dir; reloaded on first
/// access per run_id. Broadcast happens only after memory + disk append.
#[derive(Clone, Default)]
pub struct EventSequencer {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    sequences: HashMap<String, u64>,
    events: HashMap<String, Vec<RunEventV2>>,
    buses: HashMap<String, broadcast::Sender<RunEventV2>>,
    loaded: HashMap<String, bool>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            sequences: HashMap::new(),
            events: HashMap::new(),
            buses: HashMap::new(),
            loaded: HashMap::new(),
        }
    }
}

/// Resolve durable event directory. `None` only when explicitly disabled.
pub fn event_log_dir() -> Option<PathBuf> {
    if std::env::var("NATIVES_EVENT_LOG_DISABLE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return None;
    }
    if let Ok(dir) = std::env::var("NATIVES_EVENT_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    if let Ok(runtime) = std::env::var("NATIVES_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return Some(PathBuf::from(runtime).join("events"));
        }
    }
    // Default on: ~/.natives/events (or temp when HOME missing — still durable for session).
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    Some(base.join(".natives").join("events"))
}

impl EventSequencer {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_loaded(inner: &mut Inner, run_id: &str) {
        if inner.loaded.get(run_id).copied().unwrap_or(false) {
            return;
        }
        inner.loaded.insert(run_id.to_string(), true);
        let Some(dir) = event_log_dir() else {
            return;
        };
        let path = dir.join(format!("{run_id}.jsonl"));
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let mut max_seq = 0u64;
        let mut loaded = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(ev) = serde_json::from_str::<RunEventV2>(line) {
                max_seq = max_seq.max(ev.sequence);
                loaded.push(ev);
            }
        }
        if !loaded.is_empty() {
            inner.sequences.insert(run_id.to_string(), max_seq);
            inner.events.insert(run_id.to_string(), loaded);
        }
    }

    fn persist_event(run_id: &str, event: &RunEventV2) {
        let Some(dir) = event_log_dir() else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{run_id}.jsonl"));
        if let Ok(line) = serde_json::to_string(event) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    /// Persist (memory + default disk) then broadcast. Returns the assigned sequence.
    pub fn append(&self, run_id: &str, mut payload: RunEventKind) -> RunEventV2 {
        // Never let secrets leak into the event bus.
        payload = sanitize_payload(payload);
        let mut inner = self.inner.lock().expect("event sequencer lock");
        Self::ensure_loaded(&mut inner, run_id);
        let next = inner.sequences.entry(run_id.to_string()).or_insert(0);
        *next += 1;
        let sequence = *next;
        let event = RunEventV2::new(run_id, sequence, payload);
        // Persist-first: memory + disk before broadcast (never reverse this order).
        inner
            .events
            .entry(run_id.to_string())
            .or_default()
            .push(event.clone());
        Self::persist_event(run_id, &event);
        let sender = inner
            .buses
            .entry(run_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0);
        let _ = sender.send(event.clone());
        event
    }

    pub fn replay_after(&self, run_id: &str, after_sequence: u64) -> Vec<RunEventV2> {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        Self::ensure_loaded(&mut inner, run_id);
        inner
            .events
            .get(run_id)
            .map(|events| {
                events
                    .iter()
                    .filter(|e| e.sequence > after_sequence)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn subscribe(&self, run_id: &str) -> broadcast::Receiver<RunEventV2> {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        Self::ensure_loaded(&mut inner, run_id);
        let sender = inner
            .buses
            .entry(run_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0);
        sender.subscribe()
    }

    pub fn last_sequence(&self, run_id: &str) -> u64 {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        Self::ensure_loaded(&mut inner, run_id);
        inner.sequences.get(run_id).copied().unwrap_or(0)
    }
}

fn sanitize_payload(payload: RunEventKind) -> RunEventKind {
    match payload {
        RunEventKind::Failed { error, code } => RunEventKind::Failed {
            error: redact_secrets(&error),
            code,
        },
        RunEventKind::Interrupted { reason } => RunEventKind::Interrupted {
            reason: redact_secrets(&reason),
        },
        RunEventKind::Progress { message, percentage } => RunEventKind::Progress {
            message: redact_secrets(&message),
            percentage,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_are_monotonic_and_replayable() {
        // Unique run_id: default disk persistence may retain prior "r1" fixtures.
        let run_id = format!("r-seq-{}", uuid::Uuid::new_v4());
        let log = EventSequencer::new();
        let a = log.append(&run_id, RunEventKind::Started);
        let b = log.append(&run_id, RunEventKind::TextDelta { text: "hi".into() });
        assert_eq!(a.sequence, 1);
        assert_eq!(b.sequence, 2);
        let replay = log.replay_after(&run_id, 1);
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].sequence, 2);
    }

    #[test]
    fn redacts_failed_errors() {
        let log = EventSequencer::new();
        let run_id = format!("r-redact-{}", uuid::Uuid::new_v4());
        let event = log.append(
            &run_id,
            RunEventKind::Failed {
                error: "Authorization: Bearer sk-secretvalue123".into(),
                code: "auth".into(),
            },
        );
        match event.payload {
            RunEventKind::Failed { error, .. } => {
                assert!(!error.contains("sk-secretvalue"));
                assert!(error.contains("REDACTED"));
            }
            _ => panic!("expected failed"),
        }
    }

    #[test]
    fn default_event_log_dir_is_some_unless_disabled() {
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
        assert!(event_log_dir().is_some());
        std::env::set_var("NATIVES_EVENT_LOG_DISABLE", "1");
        assert!(event_log_dir().is_none());
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
    }

    #[test]
    fn persists_and_reloads_when_dir_set() {
        let dir = std::env::temp_dir().join(format!(
            "natives-ev-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("NATIVES_EVENT_LOG_DIR", &dir);
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        {
            let log = EventSequencer::new();
            log.append(&run_id, RunEventKind::Started);
            log.append(
                &run_id,
                RunEventKind::TextDelta {
                    text: "hello".into(),
                },
            );
        }
        // Fresh sequencer must reload from disk.
        let log2 = EventSequencer::new();
        let replay = log2.replay_after(&run_id, 0);
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].sequence, 1);
        assert_eq!(replay[1].sequence, 2);
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("NATIVES_EVENT_LOG_DIR");
    }
}
