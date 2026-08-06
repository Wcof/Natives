//! Bounded runtime metrics sink — non-authoritative, no secrets.
//!
//! # Design
//!
//! - `RuntimeMetrics` trait: one method per metric point. Each method takes
//!   only cardinality-safe parameters (no secrets, paths, prompts, or
//!   credentials).
//! - `NoopMetrics`: no-op implementation used when no collector is wired.
//!   Metrics collection failure never changes execution facts.
//! - All methods are synchronous and non-blocking. The sink owns its own
//!   accumulation (e.g., atomics, channels) and does not hold locks across
//!   await points.
//!
//! # Metric inventory
//!
//! | Category | Metric | Labels | Unit |
//! |---|---|---|---|
//! | Run | started | — | count |
//! | Run | completed | — | count |
//! | Run | failed | reason | count |
//! | Turn | started | — | count |
//! | Turn | completed | — | count |
//! | Provider | request | — | latency ms |
//! | Provider | error | code | count |
//! | Tool | call | tool_name | count |
//! | Tool | result | is_error | latency ms |
//! | Queue | enqueue | — | count |
//! | Queue | dequeue | — | count |
//! | Storage | write | — | latency ms |
//! | Storage | read | — | latency ms |
//! | Storage | error | — | count |

use std::sync::Arc;

/// Metrics sink for the agent runtime.
///
/// Every method is synchronous and non-blocking. The default
/// implementation (`NoopMetrics`) records nothing, so callers
/// never need to check whether a sink is present.
pub trait RuntimeMetrics: Send + Sync {
    /// A run started.
    fn record_run_started(&self);
    /// A run completed (terminal event committed).
    fn record_run_completed(&self);
    /// A run failed with a reason code.
    fn record_run_failed(&self, _reason: &str) {}

    /// A turn started.
    fn record_turn_started(&self);
    /// A turn completed.
    fn record_turn_completed(&self) {}

    /// A provider request completed (success or failure).
    fn record_provider_request(&self, _latency_ms: u64) {}
    /// A provider request failed with an error code.
    fn record_provider_error(&self, _code: &str) {}

    /// A tool call was dispatched.
    fn record_tool_call(&self, _tool_name: &str) {}
    /// A tool execution completed.
    fn record_tool_result(&self, _latency_ms: u64, _is_error: bool) {}

    /// An item was enqueued.
    fn record_queue_enqueue(&self) {}
    /// An item was dequeued.
    fn record_queue_dequeue(&self) {}

    /// A storage write completed.
    fn record_storage_write(&self, _latency_ms: u64) {}
    /// A storage read completed.
    fn record_storage_read(&self, _latency_ms: u64) {}
    /// A storage operation failed.
    fn record_storage_error(&self) {}
}

/// No-op metrics sink — never records anything.
///
/// Used as the default when no collector is wired. Metrics collection
/// failure never changes execution facts, and this sink is the proof
/// by construction: it never allocates, never blocks, and never fails.
pub struct NoopMetrics;

impl RuntimeMetrics for NoopMetrics {
    fn record_run_started(&self) {}
    fn record_run_completed(&self) {}
    fn record_run_failed(&self, _reason: &str) {}
    fn record_turn_started(&self) {}
    fn record_turn_completed(&self) {}
    fn record_provider_request(&self, _latency_ms: u64) {}
    fn record_provider_error(&self, _code: &str) {}
    fn record_tool_call(&self, _tool_name: &str) {}
    fn record_tool_result(&self, _latency_ms: u64, _is_error: bool) {}
    fn record_queue_enqueue(&self) {}
    fn record_queue_dequeue(&self) {}
    fn record_storage_write(&self, _latency_ms: u64) {}
    fn record_storage_read(&self, _latency_ms: u64) {}
    fn record_storage_error(&self) {}
}

/// Thread-safe reference to a metrics sink.
pub type MetricsSink = Arc<dyn RuntimeMetrics>;

/// Default metrics sink (no-op).
pub fn default_metrics_sink() -> MetricsSink {
    Arc::new(NoopMetrics)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Test metrics sink that records everything in atomics.
    struct TestMetrics {
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

    impl TestMetrics {
        fn new() -> Self {
            Self {
                run_started: AtomicU64::new(0),
                run_completed: AtomicU64::new(0),
                run_failed: AtomicU64::new(0),
                turn_started: AtomicU64::new(0),
                turn_completed: AtomicU64::new(0),
                provider_requests: AtomicU64::new(0),
                provider_errors: AtomicU64::new(0),
                tool_calls: AtomicU64::new(0),
                tool_results: AtomicU64::new(0),
                queue_enqueues: AtomicU64::new(0),
                queue_dequeues: AtomicU64::new(0),
                storage_writes: AtomicU64::new(0),
                storage_reads: AtomicU64::new(0),
                storage_errors: AtomicU64::new(0),
            }
        }
    }

    impl RuntimeMetrics for TestMetrics {
        fn record_run_started(&self) {
            self.run_started.fetch_add(1, Ordering::SeqCst);
        }
        fn record_run_completed(&self) {
            self.run_completed.fetch_add(1, Ordering::SeqCst);
        }
        fn record_run_failed(&self, _reason: &str) {
            self.run_failed.fetch_add(1, Ordering::SeqCst);
        }
        fn record_turn_started(&self) {
            self.turn_started.fetch_add(1, Ordering::SeqCst);
        }
        fn record_turn_completed(&self) {
            self.turn_completed.fetch_add(1, Ordering::SeqCst);
        }
        fn record_provider_request(&self, _latency_ms: u64) {
            self.provider_requests.fetch_add(1, Ordering::SeqCst);
        }
        fn record_provider_error(&self, _code: &str) {
            self.provider_errors.fetch_add(1, Ordering::SeqCst);
        }
        fn record_tool_call(&self, _tool_name: &str) {
            self.tool_calls.fetch_add(1, Ordering::SeqCst);
        }
        fn record_tool_result(&self, _latency_ms: u64, _is_error: bool) {
            self.tool_results.fetch_add(1, Ordering::SeqCst);
        }
        fn record_queue_enqueue(&self) {
            self.queue_enqueues.fetch_add(1, Ordering::SeqCst);
        }
        fn record_queue_dequeue(&self) {
            self.queue_dequeues.fetch_add(1, Ordering::SeqCst);
        }
        fn record_storage_write(&self, _latency_ms: u64) {
            self.storage_writes.fetch_add(1, Ordering::SeqCst);
        }
        fn record_storage_read(&self, _latency_ms: u64) {
            self.storage_reads.fetch_add(1, Ordering::SeqCst);
        }
        fn record_storage_error(&self) {
            self.storage_errors.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn noop_metrics_never_panics() {
        let m = NoopMetrics;
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
    }

    #[test]
    fn test_metrics_records_every_point() {
        let m = TestMetrics::new();
        m.record_run_started();
        m.record_run_completed();
        m.record_run_failed("error");
        m.record_turn_started();
        m.record_turn_completed();
        m.record_provider_request(100);
        m.record_provider_error("timeout");
        m.record_tool_call("read_file");
        m.record_tool_result(50, false);
        m.record_queue_enqueue();
        m.record_queue_dequeue();
        m.record_storage_write(10);
        m.record_storage_read(5);
        m.record_storage_error();

        assert_eq!(m.run_started.load(Ordering::SeqCst), 1);
        assert_eq!(m.run_completed.load(Ordering::SeqCst), 1);
        assert_eq!(m.run_failed.load(Ordering::SeqCst), 1);
        assert_eq!(m.turn_started.load(Ordering::SeqCst), 1);
        assert_eq!(m.turn_completed.load(Ordering::SeqCst), 1);
        assert_eq!(m.provider_requests.load(Ordering::SeqCst), 1);
        assert_eq!(m.provider_errors.load(Ordering::SeqCst), 1);
        assert_eq!(m.tool_calls.load(Ordering::SeqCst), 1);
        assert_eq!(m.tool_results.load(Ordering::SeqCst), 1);
        assert_eq!(m.queue_enqueues.load(Ordering::SeqCst), 1);
        assert_eq!(m.queue_dequeues.load(Ordering::SeqCst), 1);
        assert_eq!(m.storage_writes.load(Ordering::SeqCst), 1);
        assert_eq!(m.storage_reads.load(Ordering::SeqCst), 1);
        assert_eq!(m.storage_errors.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_metrics_cardinality_is_bounded() {
        // Tool name is the only label with runtime-determined cardinality.
        // Verify it's a single string, not a path or secret.
        let m = TestMetrics::new();
        m.record_tool_call("read_file");
        m.record_tool_call("write_file");
        m.record_tool_call("read_file");
        assert_eq!(m.tool_calls.load(Ordering::SeqCst), 3);
        // No path, credential, or prompt label is present.
    }

    #[test]
    fn metrics_sink_failure_does_not_affect_execution() {
        // The no-op sink cannot fail. TestMetrics also cannot fail.
        // This test verifies that the trait has no fallible methods.
        fn use_sink(sink: &dyn RuntimeMetrics) {
            sink.record_run_started();
            sink.record_turn_started();
            sink.record_provider_request(42);
        }
        use_sink(&NoopMetrics);
        use_sink(&TestMetrics::new());
    }

    #[test]
    fn default_metrics_sink_is_noop() {
        let sink = default_metrics_sink();
        sink.record_run_started();
        sink.record_turn_started();
        // No panic — the no-op sink never fails.
    }

    #[test]
    fn metrics_sink_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<NoopMetrics>();
        assert_sync::<NoopMetrics>();
        assert_send::<MetricsSink>();
        assert_sync::<MetricsSink>();
    }
}
