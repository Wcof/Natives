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
fn parse_error_category(category: &str) -> Option<HookErrorCategory> {
    match category {
        "handler_failure" => Some(HookErrorCategory::HandlerFailure),
        "spawn_failure" => Some(HookErrorCategory::SpawnFailure),
        "timeout" => Some(HookErrorCategory::Timeout),
        "persistence_failure" => Some(HookErrorCategory::PersistenceFailure),
        "policy_resolution" => Some(HookErrorCategory::PolicyResolution),
        _ => None,
    }
}
