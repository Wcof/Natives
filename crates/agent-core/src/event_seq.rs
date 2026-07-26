//! In-memory monotonic event sequence per run (persist-first seam).
//!
//! Persistence is **on by default** (full remediation 第十三节). Override with:
//! - `NATIVES_EVENT_LOG_DIR` — explicit directory
//! - `NATIVES_RUNTIME_DIR/events` — when runtime dir is set
//! - `~/.natives/events` — default home path
//! - `NATIVES_EVENT_LOG_DISABLE=1` — memory-only (tests / explicit opt-out)

use assistant_protocol::v2::{redact_secrets, RunEventKind, RunEventV2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub trait EventPersistence: Send + Sync {
    fn append(&self, event: &RunEventV2) -> Result<(), String>;
    fn replay_after(&self, run_id: &str, after_sequence: u64) -> Result<Vec<RunEventV2>, String>;
    fn last_sequence(&self, run_id: &str) -> Result<u64, String>;
}

/// Append-only event log with fan-out subscribers.
///
/// Default durability: JSONL under the resolved event log dir; reloaded on first
/// access per run_id. Broadcast happens only after memory + disk append.
#[derive(Clone, Default)]
pub struct EventSequencer {
    inner: Arc<Mutex<Inner>>,
    persistence: Option<Arc<dyn EventPersistence>>,
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

    pub fn with_persistence(persistence: Arc<dyn EventPersistence>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            persistence: Some(persistence),
        }
    }

    fn ensure_loaded(&self, inner: &mut Inner, run_id: &str) {
        if inner.loaded.get(run_id).copied().unwrap_or(false) {
            return;
        }
        inner.loaded.insert(run_id.to_string(), true);
        if let Some(persistence) = &self.persistence {
            if let Ok(loaded) = persistence.replay_after(run_id, 0) {
                if !loaded.is_empty() {
                    let max_seq = loaded.iter().map(|ev| ev.effective_run_sequence()).max().unwrap_or(0);
                    inner.sequences.insert(run_id.to_string(), max_seq);
                    inner.events.insert(run_id.to_string(), loaded);
                    return;
                }
            }
            if let Ok(max_seq) = persistence.last_sequence(run_id) {
                if max_seq > 0 {
                    inner.sequences.insert(run_id.to_string(), max_seq);
                }
            }
            return;
        }
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
                max_seq = max_seq.max(ev.effective_run_sequence());
                loaded.push(ev);
            }
        }
        if !loaded.is_empty() {
            inner.sequences.insert(run_id.to_string(), max_seq);
            inner.events.insert(run_id.to_string(), loaded);
        }
    }

    fn persist_event(&self, run_id: &str, event: &RunEventV2) -> Result<(), String> {
        if let Some(persistence) = &self.persistence {
            return persistence.append(event);
        }
        let Some(dir) = event_log_dir() else {
            return Ok(());
        };
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("PERSISTENCE_FAILED create event dir: {e}"))?;
        let path = dir.join(format!("{run_id}.jsonl"));
        let line = serde_json::to_string(event)
            .map_err(|e| format!("PERSISTENCE_FAILED serialize event: {e}"))?;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("PERSISTENCE_FAILED open event log: {e}"))?;
        writeln!(f, "{line}").map_err(|e| format!("PERSISTENCE_FAILED write event log: {e}"))?;
        Ok(())
    }

    /// Persist (memory + default disk) then broadcast. Returns the assigned sequence.
    pub fn append(&self, run_id: &str, mut payload: RunEventKind) -> RunEventV2 {
        // Never let secrets leak into the event bus.
        payload = sanitize_payload(payload);
        let mut inner = self.inner.lock().expect("event sequencer lock");
        self.ensure_loaded(&mut inner, run_id);
        let next = inner.sequences.entry(run_id.to_string()).or_insert(0);
        *next += 1;
        let sequence = *next;
        let event = RunEventV2::new(run_id, sequence, payload);
        // Persist-first: disk before memory/broadcast. If persistence fails,
        // do not publish a fake-success event.
        if let Err(error) = self.persist_event(run_id, &event) {
            *next -= 1;
            return RunEventV2::new(
                run_id,
                sequence,
                RunEventKind::Failed {
                    error: redact_secrets(&error),
                    code: "PERSISTENCE_FAILED".into(),
                },
            );
        }
        inner
            .events
            .entry(run_id.to_string())
            .or_default()
            .push(event.clone());
        let sender = inner
            .buses
            .entry(run_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0);
        let _ = sender.send(event.clone());
        event
    }

    /// Inject an already-persisted (or memory-CAS-committed) event into the
    /// sequencer memory + broadcast without re-persisting.
    ///
    /// Used by `RunManager::commit_transition` after the SQLite run+lifecycle
    /// transaction succeeds. Sequence must be monotonic for the run.
    pub fn inject_committed(&self, event: RunEventV2) {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        self.ensure_loaded(&mut inner, &event.run_id);
        let seq = event.effective_run_sequence();
        let entry = inner
            .sequences
            .entry(event.run_id.clone())
            .or_insert(0);
        if seq > *entry {
            *entry = seq;
        }
        let events = inner.events.entry(event.run_id.clone()).or_default();
        if events.iter().any(|e| e.effective_run_sequence() == seq) {
            return;
        }
        events.push(event.clone());
        events.sort_by_key(|e| e.effective_run_sequence());
        let sender = inner
            .buses
            .entry(event.run_id.clone())
            .or_insert_with(|| broadcast::channel(256).0);
        let _ = sender.send(event);
    }

    pub fn replay_after(&self, run_id: &str, after_sequence: u64) -> Vec<RunEventV2> {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        self.ensure_loaded(&mut inner, run_id);
        inner
            .events
            .get(run_id)
            .map(|events| {
                events
                    .iter()
                    .filter(|e| e.effective_run_sequence() > after_sequence)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn subscribe(&self, run_id: &str) -> broadcast::Receiver<RunEventV2> {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        self.ensure_loaded(&mut inner, run_id);
        let sender = inner
            .buses
            .entry(run_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0);
        sender.subscribe()
    }

    pub fn last_sequence(&self, run_id: &str) -> u64 {
        let mut inner = self.inner.lock().expect("event sequencer lock");
        self.ensure_loaded(&mut inner, run_id);
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
        RunEventKind::Progress {
            message,
            percentage,
        } => RunEventKind::Progress {
            message: redact_secrets(&message),
            percentage,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("event env lock")
    }

    #[test]
    fn sequences_are_monotonic_and_replayable() {
        let _guard = env_lock();
        std::env::remove_var("NATIVES_EVENT_LOG_DIR");
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
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
        let _guard = env_lock();
        std::env::remove_var("NATIVES_EVENT_LOG_DIR");
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
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
        let _guard = env_lock();
        std::env::remove_var("NATIVES_EVENT_LOG_DIR");
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
        assert!(event_log_dir().is_some());
        std::env::set_var("NATIVES_EVENT_LOG_DISABLE", "1");
        assert!(event_log_dir().is_none());
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");
    }

    #[test]
    fn persists_and_reloads_when_dir_set() {
        let _guard = env_lock();
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

    #[test]
    fn persistence_failure_is_not_replayed_or_broadcast() {
        let _guard = env_lock();
        let path = std::env::temp_dir().join(format!("natives-ev-file-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"not a dir").unwrap();
        std::env::set_var("NATIVES_EVENT_LOG_DIR", &path);
        std::env::remove_var("NATIVES_EVENT_LOG_DISABLE");

        let log = EventSequencer::new();
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let mut rx = log.subscribe(&run_id);
        let event = log.append(&run_id, RunEventKind::Started);

        assert_eq!(event.effective_run_sequence(), 1);
        assert!(matches!(
            event.payload,
            RunEventKind::Failed { ref code, .. } if code == "PERSISTENCE_FAILED"
        ));
        assert!(log.replay_after(&run_id, 0).is_empty());
        assert!(rx.try_recv().is_err());

        let _ = std::fs::remove_file(&path);
        std::env::remove_var("NATIVES_EVENT_LOG_DIR");
    }
}
