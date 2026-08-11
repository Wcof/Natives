//! HookInvocation trace mapping (extracted from `production_hooks.rs`,
//! task-01 structure).
//!
//! Maps the durable `HookInvocationStarted` / `HookInvocationCompleted` run
//! event payloads onto the [`HookInvocationTrace`] metadata model shared with
//! harness-core, so a trace can be attributed to the frozen plan it ran under.

use super::*;

/// Map a `HookInvocationStarted` run event into the [`HookInvocationTrace`]
/// metadata model.
///
/// This is the daemon side of the contract the model specifies in
/// `harness-core`: the single mapping between the durable event payload and
/// the model, so a trace can be attributed to the frozen plan it ran under.
pub fn trace_from_started_event(
    event: &assistant_protocol::v2::RunEventV2,
    plan_hash: &str,
) -> Option<HookInvocationTrace> {
    let assistant_protocol::v2::RunEventKind::HookInvocationStarted {
        invocation_id,
        hook_id,
        hook_event,
        source,
        ordinal,
        input_summary,
        input_truncated,
    } = &event.payload
    else {
        return None;
    };
    Some(HookInvocationTrace::started(
        event.run_id.clone(),
        plan_hash,
        invocation_id.clone(),
        hook_id.clone(),
        hook_event.clone(),
        source.clone(),
        *ordinal,
        input_summary.clone(),
        *input_truncated,
    ))
}

/// Map a `HookInvocationCompleted` run event into the [`HookInvocationTrace`]
/// metadata model.
pub fn trace_from_completed_event(
    event: &assistant_protocol::v2::RunEventV2,
    plan_hash: &str,
) -> Option<HookInvocationTrace> {
    let assistant_protocol::v2::RunEventKind::HookInvocationCompleted {
        invocation_id,
        hook_id,
        hook_event,
        source,
        ordinal,
        effective_decision,
        error_category,
        duration_ms,
        output_summary,
        output_truncated,
        ..
    } = &event.payload
    else {
        return None;
    };
    Some(
        HookInvocationTrace::started(
            event.run_id.clone(),
            plan_hash,
            invocation_id.clone(),
            hook_id.clone(),
            hook_event.clone(),
            source.clone(),
            *ordinal,
            String::new(),
            false,
        )
        .completed(
            *duration_ms,
            effective_decision.clone(),
            error_category.as_deref().and_then(parse_error_category),
            output_summary.clone(),
            *output_truncated,
        ),
    )
}

/// Map the registry's error-category string onto the structured model enum.
pub(crate) fn parse_error_category(category: &str) -> Option<HookErrorCategory> {
    match category {
        "handler_failure" => Some(HookErrorCategory::HandlerFailure),
        "spawn_failure" => Some(HookErrorCategory::SpawnFailure),
        "timeout" => Some(HookErrorCategory::Timeout),
        "persistence_failure" => Some(HookErrorCategory::PersistenceFailure),
        "policy_resolution" => Some(HookErrorCategory::PolicyResolution),
        _ => None,
    }
}

/// §19.5 trace attribution tests: the trace mapping carries the frozen
/// plan_hash the invocation ran under, and a Completed event's fields survive
/// the mapping intact (including the structured error category).
#[cfg(test)]
mod trace_attribution_tests {
    use super::*;

    /// §19.5: the trace mapping must carry the dispatcher's frozen plan_hash
    /// onto the Started trace, so the trace is attributed to the exact plan the
    /// Handler dispatched under — never a different plan.
    #[test]
    fn started_trace_carries_the_frozen_plan_hash() {
        let events = EventSequencer::memory_only();
        let event = events.append(
            "run-attr",
            RunEventKind::HookInvocationStarted {
                invocation_id: "inv-start".into(),
                hook_id: "builtin/allow-all#Notification".into(),
                hook_event: "Notification".into(),
                source: "allow-all".into(),
                ordinal: 0,
                input_summary: "{\"tool\":\"notification\"}".into(),
                input_truncated: false,
            },
        );
        let trace =
            trace_from_started_event(&event, "frozen-plan-abc").expect("started event maps");
        assert_eq!(trace.plan_hash, "frozen-plan-abc");
        assert_eq!(
            trace.status,
            harness_core::hooks::HookInvocationStatus::Started
        );
        assert_eq!(trace.invocation_id, "inv-start");
        assert_eq!(trace.hook_event, "Notification");
    }

    /// §19.5: a Completed event maps to a trace with the frozen plan_hash and
    /// every structured outcome field preserved — effective decision, duration,
    /// output summary, and the structured error category.
    #[test]
    fn completed_trace_preserves_plan_hash_and_outcome_fields() {
        let events = EventSequencer::memory_only();
        let event = events.append(
            "run-attr",
            RunEventKind::HookInvocationCompleted {
                invocation_id: "inv-complete".into(),
                hook_id: "builtin/allow-all#Notification".into(),
                hook_event: "Notification".into(),
                source: "allow-all".into(),
                ordinal: 0,
                status: "completed".into(),
                effective_decision: Some("Allow".into()),
                error_category: Some("spawn_failure".into()),
                duration_ms: 42,
                output_summary: "decision".into(),
                output_truncated: false,
            },
        );
        let trace =
            trace_from_completed_event(&event, "frozen-plan-xyz").expect("completed event maps");
        assert_eq!(trace.plan_hash, "frozen-plan-xyz");
        assert_eq!(
            trace.status,
            harness_core::hooks::HookInvocationStatus::Completed
        );
        assert_eq!(trace.invocation_id, "inv-complete");
        assert_eq!(trace.duration_ms, 42);
        assert_eq!(trace.effective_decision.as_deref(), Some("Allow"));
        assert_eq!(
            trace.error_category,
            Some(harness_core::hooks::HookErrorCategory::SpawnFailure),
            "the structured error category must survive the completed mapping"
        );
    }

    /// §19.5: a mismatched event payload (not HookInvocationStarted) maps to
    /// None, so the trace authority cannot accidentally attribute a non-hook
    /// event to a plan.
    #[test]
    fn mismatched_event_payload_maps_to_none() {
        let events = EventSequencer::memory_only();
        let event = events.append(
            "run-attr",
            RunEventKind::ToolCallStarted {
                id: "tc-1".into(),
                name: "read_file".into(),
            },
        );
        assert!(trace_from_started_event(&event, "plan").is_none());
        assert!(trace_from_completed_event(&event, "plan").is_none());
    }
}
