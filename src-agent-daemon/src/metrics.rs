//! Daemon-side metrics adapter.
//!
//! Wires the agent-core `RuntimeMetrics` trait into the Daemon's
//! production runtime. The `DaemonMetrics` implementation records
//! metrics using atomic counters, providing a bounded, non-authoritative
//! sink that never changes execution facts.
//!
//! # Label cardinality
//!
//! Every metric label is hard-coded in the implementation. No runtime
//! value (path, prompt, credential, or user data) becomes a label.
//! Tool name is the only label with runtime-determined cardinality;
//! it is a well-known tool name, not a user-controlled value.

use agent_core::metrics::{MetricsSink, RuntimeMetrics};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Atomic-backed metrics sink used by the production Daemon.
///
/// Every counter is a separate `AtomicU64`. The sink is `Send + Sync`
/// and never blocks. Metrics are not authoritative — they are advisory
/// observability data that may be lost on process exit.
#[derive(Default)]
pub struct DaemonMetrics {
    run_started: AtomicU64,
    run_completed: AtomicU64,
    run_failed: AtomicU64,
    turn_started: AtomicU64,
    turn_completed: AtomicU64,
    provider_requests: AtomicU64,
    provider_errors: AtomicU64,
    tool_calls: AtomicU64,
    tool_results: AtomicU64,
    queue_enqueues: AtomicU64,
    queue_dequeues: AtomicU64,
    storage_writes: AtomicU64,
    storage_reads: AtomicU64,
    storage_errors: AtomicU64,
}

impl DaemonMetrics {
    /// Create a new metrics sink with all counters at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total run started count.
    pub fn total_run_started(&self) -> u64 {
        self.run_started.load(Ordering::Relaxed)
    }
    /// Total run completed count.
    pub fn total_run_completed(&self) -> u64 {
        self.run_completed.load(Ordering::Relaxed)
    }
    /// Total run failed count.
    pub fn total_run_failed(&self) -> u64 {
        self.run_failed.load(Ordering::Relaxed)
    }
    /// Total turn started count.
    pub fn total_turn_started(&self) -> u64 {
        self.turn_started.load(Ordering::Relaxed)
    }
    /// Total provider requests.
    pub fn total_provider_requests(&self) -> u64 {
        self.provider_requests.load(Ordering::Relaxed)
    }
    /// Total provider errors.
    pub fn total_provider_errors(&self) -> u64 {
        self.provider_errors.load(Ordering::Relaxed)
    }
    /// Total tool calls.
    pub fn total_tool_calls(&self) -> u64 {
        self.tool_calls.load(Ordering::Relaxed)
    }
    /// Total tool results.
    pub fn total_tool_results(&self) -> u64 {
        self.tool_results.load(Ordering::Relaxed)
    }
    /// Total queue enqueues.
    pub fn total_queue_enqueues(&self) -> u64 {
        self.queue_enqueues.load(Ordering::Relaxed)
    }
    /// Total queue dequeues.
    pub fn total_queue_dequeues(&self) -> u64 {
        self.queue_dequeues.load(Ordering::Relaxed)
    }
    /// Total storage writes.
    pub fn total_storage_writes(&self) -> u64 {
        self.storage_writes.load(Ordering::Relaxed)
    }
    /// Total storage reads.
    pub fn total_storage_reads(&self) -> u64 {
        self.storage_reads.load(Ordering::Relaxed)
    }
    /// Total storage errors.
    pub fn total_storage_errors(&self) -> u64 {
        self.storage_errors.load(Ordering::Relaxed)
    }
}

impl RuntimeMetrics for DaemonMetrics {
    fn record_run_started(&self) {
        self.run_started.fetch_add(1, Ordering::Relaxed);
    }
    fn record_run_completed(&self) {
        self.run_completed.fetch_add(1, Ordering::Relaxed);
    }
    fn record_run_failed(&self, _reason: &str) {
        self.run_failed.fetch_add(1, Ordering::Relaxed);
    }
    fn record_turn_started(&self) {
        self.turn_started.fetch_add(1, Ordering::Relaxed);
    }
    fn record_turn_completed(&self) {
        self.turn_completed.fetch_add(1, Ordering::Relaxed);
    }
    fn record_provider_request(&self, _latency_ms: u64) {
        self.provider_requests.fetch_add(1, Ordering::Relaxed);
    }
    fn record_provider_error(&self, _code: &str) {
        self.provider_errors.fetch_add(1, Ordering::Relaxed);
    }
    fn record_tool_call(&self, _tool_name: &str) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
    }
    fn record_tool_result(&self, _latency_ms: u64, _is_error: bool) {
        self.tool_results.fetch_add(1, Ordering::Relaxed);
    }
    fn record_queue_enqueue(&self) {
        self.queue_enqueues.fetch_add(1, Ordering::Relaxed);
    }
    fn record_queue_dequeue(&self) {
        self.queue_dequeues.fetch_add(1, Ordering::Relaxed);
    }
    fn record_storage_write(&self, _latency_ms: u64) {
        self.storage_writes.fetch_add(1, Ordering::Relaxed);
    }
    fn record_storage_read(&self, _latency_ms: u64) {
        self.storage_reads.fetch_add(1, Ordering::Relaxed);
    }
    fn record_storage_error(&self) {
        self.storage_errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// Create a production metrics sink wired to the Daemon.
pub fn daemon_metrics_sink() -> MetricsSink {
    Arc::new(DaemonMetrics::new())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_metrics_records_all_points() {
        let m = DaemonMetrics::new();
        m.record_run_started();
        m.record_run_completed();
        m.record_run_failed("timeout");
        m.record_turn_started();
        m.record_turn_completed();
        m.record_provider_request(100);
        m.record_provider_error("rate_limited");
        m.record_tool_call("read_file");
        m.record_tool_result(50, false);
        m.record_queue_enqueue();
        m.record_queue_dequeue();
        m.record_storage_write(10);
        m.record_storage_read(5);
        m.record_storage_error();

        assert_eq!(m.total_run_started(), 1);
        assert_eq!(m.total_run_completed(), 1);
        assert_eq!(m.total_run_failed(), 1);
        assert_eq!(m.total_turn_started(), 1);
        assert_eq!(m.total_provider_requests(), 1);
        assert_eq!(m.total_provider_errors(), 1);
        assert_eq!(m.total_tool_calls(), 1);
        assert_eq!(m.total_tool_results(), 1);
        assert_eq!(m.total_queue_enqueues(), 1);
        assert_eq!(m.total_queue_dequeues(), 1);
        assert_eq!(m.total_storage_writes(), 1);
        assert_eq!(m.total_storage_reads(), 1);
        assert_eq!(m.total_storage_errors(), 1);
    }

    #[test]
    fn daemon_metrics_sink_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<DaemonMetrics>();
        assert_sync::<DaemonMetrics>();
    }

    #[test]
    fn daemon_metrics_works_as_trait_object() {
        let sink: MetricsSink = daemon_metrics_sink();
        sink.record_run_started();
        sink.record_turn_started();
        // No panic — the sink is wired correctly.
    }

    #[test]
    fn daemon_metrics_concurrent_increment() {
        let m = Arc::new(DaemonMetrics::new());
        let mut handles = Vec::new();
        for _ in 0..10 {
            let m = m.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    m.record_run_started();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(m.total_run_started(), 1000);
    }
}
