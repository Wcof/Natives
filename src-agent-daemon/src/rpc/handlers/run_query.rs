//! Run query handlers — listChildren / getActivity (W9 split from
//! rpc/handlers/run.rs). All fields are projected from existing authorities
//! (RunManager run record, child projection, persisted interaction table);
//! nothing is synthesised.

use super::run::{required_param, run_manager};

// {A2-03} handle_run_list_children (moved verbatim from rpc.rs)
/// `run.listChildren` — direct children of `parent_run_id`.
///
/// Source of truth is the RunManager projection (the sole owner of run identity), not
/// the live ExecutionRegistry: children of a finished parent must still be listable.
/// Depth-1 only; callers recurse if they want the whole tree.
pub(crate) fn handle_run_list_children(
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let parent = required_param(params, &["parent_run_id", "parentRunId", "run_id", "runId"])?;
    let mut children: Vec<assistant_protocol::v2::RunV2> = run_manager()
        .list_runs(None)
        .into_iter()
        .filter(|r| r.parent_run_id.as_deref() == Some(parent))
        .collect();
    children.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(serde_json::json!({
        "parent_run_id": parent,
        "children": children,
    }))
}

// {A2-03} handle_run_get_activity (moved verbatim from rpc.rs)
/// `run.getActivity` — point-in-time activity snapshot for one run.
///
/// Every field is projected from an existing source (RunManager run record, the run's
/// child projection, and the persisted interaction table). Nothing is synthesised.
pub(crate) fn handle_run_get_activity(
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let run_id = required_param(params, &["run_id", "runId"])?;
    let run = run_manager()
        .get_run(run_id)
        .ok_or_else(|| format!("run not found: {run_id}"))?;
    let child_run_ids: Vec<String> = run_manager()
        .list_runs(None)
        .into_iter()
        .filter(|r| r.parent_run_id.as_deref() == Some(run_id))
        .map(|r| r.id)
        .collect();
    // Best-effort: a missing/locked interaction table must not fail the snapshot.
    let pending = crate::interaction_store::list_pending(serde_json::json!({ "run_id": run_id }))
        .ok()
        .and_then(|v| v.get("interactions").cloned())
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let pending_count = pending.as_array().map(|a| a.len()).unwrap_or(0);
    Ok(serde_json::json!({
        "run_id": run.id,
        "conversation_id": run.conversation_id,
        "status": run.status,
        "runtime_id": run.runtime_id,
        "provider_id": run.provider_id,
        "model_id": run.model_id,
        "agent_profile_id": run.agent_profile_id,
        "step_count": run.step_count,
        "max_steps": run.max_steps,
        "retry_count": run.retry_count,
        "last_event_sequence": run.last_event_sequence,
        "created_at": run.created_at,
        "started_at": run.started_at,
        "finished_at": run.finished_at,
        "error_code": run.error_code,
        "child_run_ids": child_run_ids,
        "pending_interaction_count": pending_count,
        "pending_interactions": pending,
    }))
}
