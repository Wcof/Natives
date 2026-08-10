//! HookInvocation trace mapping tests (extracted from `production_hooks.rs`,
//! task-01 structure).

use super::*;
use harness_core::hooks::HookInvocationStatus;

/// The trace mapping round-trips the durable run-event payloads into the
/// metadata model, including the structured error category.
#[test]
fn trace_mapping_round_trips_run_event_payloads() {
    let events = EventSequencer::memory_only();
    let started = events.append(
        "run-map",
        RunEventKind::HookInvocationStarted {
            invocation_id: "inv-1".into(),
            hook_id: "builtin/allow-all#Notification".into(),
            hook_event: "Notification".into(),
            source: "allow-all".into(),
            ordinal: 0,
            input_summary: "{\"tool\":\"notification\"}".into(),
            input_truncated: false,
        },
    );
    let trace = trace_from_started_event(&started, "plan-hash").expect("started maps");
    assert_eq!(trace.plan_hash, "plan-hash");
    assert_eq!(trace.status, HookInvocationStatus::Started);
    assert_eq!(trace.hook_event, "Notification");
    assert_eq!(trace.hook_id, "builtin/allow-all#Notification");
    assert!(!trace.input_truncated);

    let completed = events.append(
        "run-map",
        RunEventKind::HookInvocationCompleted {
            invocation_id: "inv-1".into(),
            hook_id: "builtin/allow-all#Notification".into(),
            hook_event: "Notification".into(),
            source: "allow-all".into(),
            ordinal: 0,
            status: "completed".into(),
            effective_decision: Some("Allow".into()),
            error_category: Some("timeout".into()),
            duration_ms: 7,
            output_summary: "decision".into(),
            output_truncated: false,
        },
    );
    let trace = trace_from_completed_event(&completed, "plan-hash").expect("completed maps");
    assert_eq!(trace.status, HookInvocationStatus::Completed);
    assert_eq!(trace.duration_ms, 7);
    assert_eq!(trace.effective_decision.as_deref(), Some("Allow"));
    assert_eq!(
        trace.error_category,
        Some(HookErrorCategory::Timeout),
        "the structured error category must survive the mapping"
    );
    assert_eq!(
        parse_error_category("handler_failure"),
        Some(HookErrorCategory::HandlerFailure)
    );
    assert_eq!(parse_error_category("unknown"), None);
}
