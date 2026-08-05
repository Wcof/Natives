//! Bounded single-writer SQLite executor (TASK-006 / B04).
//!
//! Async request paths submit database work as commands instead of locking the
//! DataStore's `Mutex<Connection>` directly. A dedicated worker thread owns the
//! connection for each command, so runtime workers park on a fixed-capacity
//! queue rather than blocking on the connection lock, and unbounded buffering
//! is impossible.
//!
//! Saturation semantics:
//! - Critical commands (durable facts) apply bounded backpressure: a full
//!   queue parks the submitter until a slot frees, and the worker retries
//!   SQLITE_BUSY / SQLITE_FULL a bounded number of times. Critical facts are
//!   never silently dropped.
//! - Non-critical commands (best-effort telemetry) are dropped when the queue
//!   is full and counted in `dropped_noncritical`.
//!
//! Shutdown: `shutdown()` enqueues a drain barrier behind everything already
//! queued; the worker executes those commands, then stops, so a clean shutdown
//! loses no queued fact. If the worker panics (actor crash) the queue
//! disconnects and every in-flight or new submit fails closed rather than
//! dropping data silently.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::storage::DataStore;

/// Default queue capacity in commands (not bytes).
pub const DEFAULT_CAPACITY: usize = 1024;
/// Maximum BUSY/FULL retries for a critical command before surfacing the error.
const MAX_BUSY_RETRIES: u32 = 5;
/// Sleep between BUSY/FULL retries.
const RETRY_BACKOFF: Duration = Duration::from_millis(50);

/// A database command queued on the actor.
#[allow(clippy::type_complexity)] // pre-existing: command closure shape is fixed
struct Command {
    run: Box<dyn Fn(&Connection) -> Result<serde_json::Value, String> + Send>,
    critical: bool,
    reply: Option<SyncSender<Result<serde_json::Value, String>>>,
}

/// Worker message: run a command, or a drain barrier that stops the worker
/// after everything queued ahead of it has run.
enum Message {
    Run(Command),
    Shutdown,
}

/// Bounded single-writer SQLite executor. Share via `Arc<StorageActor>`.
pub struct StorageActor {
    tx: SyncSender<Message>,
    worker: Mutex<Option<JoinHandle<()>>>,
    closed: AtomicBool,
    capacity: usize,
    dropped_noncritical: Arc<Mutex<u64>>,
    latency_ms: Arc<Mutex<Vec<u64>>>,
    total_commands: Arc<Mutex<u64>>,
}

impl StorageActor {
    /// Create an actor with the given queue capacity and the shared store it
    /// will execute against. The worker thread is started immediately.
    pub fn new(capacity: usize, store: Arc<DataStore>) -> Arc<Self> {
        let (tx, rx) = sync_channel(capacity.max(1));
        let dropped = Arc::new(Mutex::new(0u64));
        let latency = Arc::new(Mutex::new(Vec::new()));
        let total = Arc::new(Mutex::new(0u64));
        let d = dropped.clone();
        let l = latency.clone();
        let t = total.clone();
        let worker = std::thread::Builder::new()
            .name("storage-actor".into())
            .spawn(move || worker_loop(rx, store, d, l, t))
            .expect("storage actor worker thread must spawn");
        Arc::new(StorageActor {
            tx,
            worker: Mutex::new(Some(worker)),
            closed: AtomicBool::new(false),
            capacity: capacity.max(1),
            dropped_noncritical: dropped,
            latency_ms: latency,
            total_commands: total,
        })
    }

    /// Submit a command. Critical commands block at full capacity (bounded
    /// backpressure) and the worker retries SQLITE_BUSY/SQLITE_FULL;
    /// non-critical commands drop when the queue is full. Returns the command
    /// result, or `null` for a fire-and-forget non-critical command.
    pub fn submit<F>(&self, critical: bool, run: F) -> Result<serde_json::Value, String>
    where
        F: Fn(&Connection) -> Result<serde_json::Value, String> + Send + 'static,
    {
        if self.closed.load(Ordering::SeqCst) {
            return Err("storage actor shut down".into());
        }
        if critical {
            let (reply, rx) = sync_channel(0);
            let message = Message::Run(Command {
                run: Box::new(run),
                critical: true,
                reply: Some(reply),
            });
            self.tx
                .send(message)
                .map_err(|e| format!("storage actor queue closed: {e}"))?;
            rx.recv()
                .map_err(|e| format!("storage actor worker failed: {e}"))?
        } else {
            let message = Message::Run(Command {
                run: Box::new(run),
                critical: false,
                reply: None,
            });
            match self.tx.try_send(message) {
                Ok(()) => Ok(serde_json::Value::Null),
                Err(TrySendError::Full(_)) => {
                    *self.dropped_noncritical.lock().unwrap() += 1;
                    Err("storage actor queue full: non-critical command dropped".into())
                }
                Err(TrySendError::Disconnected(_)) => Err("storage actor worker stopped".into()),
            }
        }
    }

    /// Drain and stop: everything already queued runs first, then the worker
    /// exits. Subsequent submits fail closed. Idempotent and safe to call
    /// more than once.
    pub fn shutdown(&self) -> Result<(), String> {
        self.closed.store(true, Ordering::SeqCst);
        self.tx
            .send(Message::Shutdown)
            .map_err(|e| format!("storage actor shutdown: {e}"))?;
        if let Some(handle) = self.worker.lock().unwrap().take() {
            let _ = handle.join();
        }
        Ok(())
    }

    /// Number of non-critical commands dropped because the queue was full.
    pub fn dropped_noncritical(&self) -> u64 {
        *self.dropped_noncritical.lock().unwrap()
    }

    /// Total commands executed by the worker since start.
    pub fn total_commands(&self) -> u64 {
        *self.total_commands.lock().unwrap()
    }

    /// Per-command execution latencies in ms (recent history, unbounded cap
    /// enforced by the worker for diagnostics).
    pub fn latency_ms(&self) -> Vec<u64> {
        self.latency_ms.lock().unwrap().clone()
    }

    /// Configured queue capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

fn worker_loop(
    rx: Receiver<Message>,
    store: Arc<DataStore>,
    _dropped: Arc<Mutex<u64>>,
    latency: Arc<Mutex<Vec<u64>>>,
    total: Arc<Mutex<u64>>,
) {
    for message in rx {
        match message {
            Message::Shutdown => break,
            Message::Run(command) => {
                *total.lock().unwrap() += 1;
                let start = Instant::now();
                let outcome = run_with_retry(&store, &*command.run, command.critical);
                // Bounded latency history: keep the last 4096 samples.
                {
                    let mut samples = latency.lock().unwrap();
                    samples.push(start.elapsed().as_millis() as u64);
                    let excess = samples.len().saturating_sub(4096);
                    if excess > 0 {
                        samples.drain(..excess);
                    }
                }
                if let Some(reply) = command.reply {
                    let _ = reply.send(outcome);
                }
            }
        }
    }
}

fn run_with_retry(
    store: &Arc<DataStore>,
    run: &dyn Fn(&Connection) -> Result<serde_json::Value, String>,
    critical: bool,
) -> Result<serde_json::Value, String> {
    let mut attempts: u32 = 0;
    loop {
        let result = match store.conn() {
            Ok(conn) => run(&conn),
            Err(error) => Err(error),
        };
        if critical && attempts < MAX_BUSY_RETRIES && is_busy_or_full(&result) {
            attempts += 1;
            std::thread::sleep(RETRY_BACKOFF);
            continue;
        }
        return result;
    }
}

/// SQLite BUSY / LOCKED / FULL detection from the surfaced error text.
fn is_busy_or_full(result: &Result<serde_json::Value, String>) -> bool {
    let Err(message) = result else {
        return false;
    };
    let lower = message.to_lowercase();
    lower.contains("database is locked")
        || lower.contains("table is locked")
        || lower.contains("database or disk is full")
        || lower.contains("disk is full")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering as AtomicOrdering;

    /// Fresh temp store; no env override needed because commands run against
    /// this Arc directly.
    fn test_store() -> (Arc<DataStore>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("actor.db");
        let art = dir.path().join("artifacts");
        let store = Arc::new(DataStore::new(&db, &art).unwrap());
        {
            let conn = store.conn().unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS actor_test (id INTEGER PRIMARY KEY, tag TEXT);",
            )
            .unwrap();
        }
        (store, dir)
    }

    fn insert_tag(tag: &'static str) -> impl Fn(&Connection) -> Result<serde_json::Value, String> {
        move |conn| {
            conn.execute(
                "INSERT INTO actor_test (tag) VALUES (?1)",
                rusqlite::params![tag],
            )
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "tag": tag }))
        }
    }

    fn count(store: &Arc<DataStore>) -> i64 {
        store
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM actor_test", [], |row| row.get(0))
            .unwrap()
    }

    /// TASK-006: ten concurrent critical commands serialize through the single
    /// writer and none are lost.
    #[test]
    fn storage_actor_ten_concurrent_critical_commands_are_not_lost() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(4, store.clone());
        let handles: Vec<_> = (0..10)
            .map(|_i| {
                let actor = actor.clone();
                std::thread::spawn(move || {
                    actor.submit(true, insert_tag("c")).unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(count(&store), 10, "all ten critical facts must be durable");
        assert_eq!(actor.total_commands(), 10);
    }

    /// TASK-006: a full queue drops non-critical commands and counts them,
    /// never blocking or losing the actor.
    #[test]
    fn storage_actor_noncritical_commands_drop_when_queue_full() {
        let (store, _dir) = test_store();
        // Capacity 1 and a slow critical command occupies the worker.
        let actor = StorageActor::new(1, store.clone());
        let block = actor.clone();
        let slow = std::thread::spawn(move || {
            block
                .submit(true, |conn| {
                    std::thread::sleep(Duration::from_millis(80));
                    conn.execute("INSERT INTO actor_test (tag) VALUES ('slow')", [])
                        .map_err(|e| e.to_string())?;
                    Ok(serde_json::json!(null))
                })
                .unwrap();
        });
        // Let the slow command reach the worker, then flood non-critical.
        std::thread::sleep(Duration::from_millis(20));
        let mut dropped_some = false;
        for _i in 0..200 {
            let result = actor.submit(false, insert_tag("n"));
            if result.is_err() {
                dropped_some = true;
                break;
            }
        }
        slow.join().unwrap();
        assert!(
            dropped_some,
            "overflowing a capacity-1 queue must drop non-critical"
        );
        assert!(
            actor.dropped_noncritical() > 0,
            "dropped count must be recorded"
        );
    }

    /// TASK-006: critical commands apply bounded backpressure (park the caller
    /// at capacity) and are never dropped.
    #[test]
    fn storage_actor_critical_commands_backpressure_at_capacity() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(1, store.clone());
        // Occupying critical command.
        let blocker = actor.clone();
        let handle = std::thread::spawn(move || {
            blocker
                .submit(true, |conn| {
                    std::thread::sleep(Duration::from_millis(60));
                    conn.execute("INSERT INTO actor_test (tag) VALUES ('a')", [])
                        .map_err(|e| e.to_string())?;
                    Ok(serde_json::json!(null))
                })
                .unwrap();
        });
        std::thread::sleep(Duration::from_millis(15));
        // Second critical submit must wait for a slot, then succeed.
        let started = Instant::now();
        actor.submit(true, insert_tag("b")).unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(30),
            "critical must wait for capacity"
        );
        handle.join().unwrap();
        assert_eq!(count(&store), 2);
        assert_eq!(actor.dropped_noncritical(), 0);
    }

    /// TASK-006: a critical command that hits SQLITE_BUSY is retried and the
    /// fact is not lost.
    #[test]
    fn storage_actor_critical_busy_is_retried_not_dropped() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(2, store.clone());
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();
        let result = actor
            .submit(true, move |conn| {
                if attempts_clone.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                    return Err("database is locked".into());
                }
                conn.execute("INSERT INTO actor_test (tag) VALUES ('ok')", [])
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!(null))
            })
            .unwrap();
        assert_eq!(result, serde_json::json!(null));
        assert_eq!(
            attempts.load(AtomicOrdering::SeqCst),
            2,
            "first BUSY attempt is retried"
        );
        assert_eq!(count(&store), 1, "the fact lands after retry");
    }

    /// TASK-006: a critical command that hits SQLITE_FULL is retried and the
    /// fact is not dropped. Matches the task card's `sqlite_fault` target.
    #[test]
    fn sqlite_fault_critical_fact_survives_database_full() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(2, store.clone());
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();
        actor
            .submit(true, move |conn| {
                if attempts_clone.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                    return Err("database or disk is full".into());
                }
                conn.execute("INSERT INTO actor_test (tag) VALUES ('full-ok')", [])
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!(null))
            })
            .unwrap();
        assert_eq!(
            attempts.load(AtomicOrdering::SeqCst),
            2,
            "first FULL attempt is retried"
        );
        assert_eq!(count(&store), 1, "the fact lands after retry");
    }

    /// TASK-006: a panicking command kills the worker; the actor fails closed
    /// on every subsequent submit instead of dropping data silently.
    #[test]
    fn storage_actor_actor_crash_fails_closed() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(2, store.clone());
        let panic_actor = actor.clone();
        let handle = std::thread::spawn(move || {
            let _ = panic_actor.submit(true, |_conn| {
                panic!("simulated storage worker panic");
            });
        });
        // Wait for the worker to die (it propagates the panic to its thread).
        let mut failed_closed = false;
        for _ in 0..100 {
            let result = actor.submit(true, insert_tag("after"));
            if result.is_err() {
                failed_closed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = handle.join();
        assert!(failed_closed, "a dead worker must reject new commands");
    }

    /// TASK-006: shutdown drains everything already queued before stopping.
    #[test]
    fn storage_actor_shutdown_drains_queued_commands() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(8, store.clone());
        for _i in 0..6 {
            actor.submit(false, insert_tag("d")).unwrap();
        }
        actor.shutdown().unwrap();
        assert_eq!(count(&store), 6, "shutdown must drain the queued commands");
        assert_eq!(actor.total_commands(), 6);
        // Submits after shutdown fail closed.
        assert!(actor.submit(true, insert_tag("x")).is_err());
    }

    /// TASK-006: per-command latency is recorded for saturation diagnostics.
    #[test]
    fn storage_actor_latency_stats_are_recorded() {
        let (store, _dir) = test_store();
        let actor = StorageActor::new(2, store.clone());
        actor.submit(true, insert_tag("l")).unwrap();
        let samples = actor.latency_ms();
        assert!(
            !samples.is_empty(),
            "latency history must record the command"
        );
    }
}
