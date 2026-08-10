//! Terminal settlement of a subagent child (split from `subagent.rs` by
//! responsibility, ARCH-002): usage deltas from replayed events, completed and
//! failed terminal handlers, and the shared failure tail (slot release, session
//! close, parent event, sibling aggregation, SubagentStop hook).

use agent_core::{
    ChildFailureEffect, EventSequencer, FailurePolicy, SubAgentManager, SubAgentStatus,
};
use assistant_protocol::v2::RunEventKind;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::production::TaskRecord;

use super::subagent_requeue::{
    fail_parent_and_cancel_siblings, fire_subagent_stop, requeue_child_run, sibling_settled_state,
};
use super::subagent_watcher::spawn_retry_watcher;

/// Sum of input+output tokens in a batch of replayed events (provider deltas).
pub(crate) fn usage_delta_from_events(events: &[assistant_protocol::v2::RunEventV2]) -> u64 {
    events
        .iter()
        .filter_map(|e| match &e.payload {
            RunEventKind::UsageUpdated {
                input_tokens,
                output_tokens,
                ..
            } => Some((*input_tokens).saturating_add(*output_tokens)),
            _ => None,
        })
        .fold(0u64, u64::saturating_add)
}

/// Settle a child that reached `completed`: persist usage, release the slot
/// exactly once, emit the parent event, and let RequireAll fail the parent
/// when the batch aggregate failed.
#[allow(clippy::too_many_arguments)] // terminal settlement needs full context
pub(crate) async fn child_completed_terminal(
    events: EventSequencer,
    subagents: &Arc<SubAgentManager>,
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: &str,
    session_id: &str,
    task_id: &str,
    mem_task_id: &str,
    child_run_id: &str,
    project_path: &Option<String>,
    text: &str,
    failure_policy: FailurePolicy,
) {
    let _ = subagents
        .update_status(mem_task_id, SubAgentStatus::Completed)
        .await;
    let _ = crate::subagent_store::release_subagent_slot(session_id, None);
    let _ = crate::subagent_store::close_subagent_session(session_id, "completed", None);
    let mut final_status = "completed".to_string();
    let mut task_output = text.to_string();
    if let Err(error) = events.append_checked(
        parent_run_id,
        RunEventKind::SubagentCompleted {
            sub_run_id: child_run_id.to_string(),
            result: text.to_string(),
        },
    ) {
        final_status = "failed".into();
        task_output = format!("PERSISTENCE_FAILED: subagent completion event: {error}");
        let _ = subagents
            .update_status(mem_task_id, SubAgentStatus::Failed(task_output.clone()))
            .await;
        let _ =
            crate::subagent_store::close_subagent_session(session_id, "failed", Some(&task_output));
    }
    // RequireAll: the batch fails when every sibling is settled and any failed.
    if failure_policy == FailurePolicy::RequireAll {
        let (all_terminal, any_failed) = sibling_settled_state(task_outputs, task_id).await;
        if all_terminal && any_failed {
            fail_parent_and_cancel_siblings(
                subagents,
                parent_run_id,
                "subagent batch failed under require_all policy",
            )
            .await;
        }
    }
    task_outputs.lock().await.insert(
        task_id.to_string(),
        TaskRecord {
            run_id: child_run_id.to_string(),
            status: final_status,
            output: if task_output.is_empty() {
                None
            } else {
                Some(task_output)
            },
        },
    );
    fire_subagent_stop(project_path, parent_run_id, child_run_id, "completed", text).await;
}

/// Handle a child that failed (or was cancelled / interrupted / timed out /
/// budget-exceeded). Consumes the failure policy: Isolate keeps the parent
/// running, FailFast/RequireAll fail it, Retry re-queues transient failures.
/// Returns true when a retry was launched (the caller must not release).
#[allow(clippy::too_many_arguments)] // terminal settlement needs full context
pub(crate) async fn child_failed_terminal(
    events: EventSequencer,
    subagents: &Arc<SubAgentManager>,
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: &str,
    session_id: &str,
    task_id: &str,
    mem_task_id: &str,
    child_run_id: &str,
    project_path: &Option<String>,
    message: &str,
    failure_policy: FailurePolicy,
    max_retries: u32,
    binding: &crate::subagent_store::RouteBinding,
    child_conversation_id: &str,
    child_perm: &str,
    child_allowlist: &[String],
    child_profile_id: &Option<String>,
    child_directive: &Option<String>,
    child_max_steps: u32,
) -> bool {
    let _ = subagents
        .update_status(mem_task_id, SubAgentStatus::Failed(message.to_string()))
        .await;
    let retry_state = crate::subagent_store::get_subagent_session(session_id)
        .ok()
        .flatten();
    let retries_remaining = retry_state
        .as_ref()
        .map(|s| s.max_retries.saturating_sub(s.retry_count))
        .unwrap_or(0);
    let retryable = crate::subagent_store::is_failover_eligible_error(message);
    let effect = if retryable {
        failure_policy.on_child_failed(false, retries_remaining)
    } else {
        // Budget / permission / max-steps / deadlock errors are not retried:
        // re-queueing would just burn more budget on the same outcome.
        failure_policy.on_child_failed(false, 0)
    };

    if effect == ChildFailureEffect::Retry {
        // Keep the reservation and slot; re-queue a fresh run on the same
        // hidden conversation. `requeue_child_run` bumps the retry counter.
        let retry = retries_remaining.saturating_sub(1);
        match requeue_child_run(
            session_id,
            child_conversation_id,
            parent_run_id,
            binding,
            &retry,
            message,
            child_perm,
            child_allowlist,
            child_profile_id,
            child_directive,
            child_max_steps,
        )
        .await
        {
            Ok(new_run_id) => {
                let _ = subagents.update_run_id(mem_task_id, &new_run_id).await;
                let _ = crate::subagent_store::update_subagent_session_status(
                    session_id, "running", None,
                );
                if let Some(rec) = task_outputs.lock().await.get_mut(task_id) {
                    rec.run_id = new_run_id.clone();
                    rec.status = "running".into();
                    rec.output = None;
                }
                let _ = events.append_checked(
                    parent_run_id,
                    RunEventKind::Progress {
                        message: format!("subagent retry #{retry} after: {message}"),
                        percentage: None,
                    },
                );
                spawn_retry_watcher(
                    events.clone(),
                    subagents.clone(),
                    task_outputs.clone(),
                    parent_run_id.to_string(),
                    session_id.to_string(),
                    task_id.to_string(),
                    child_conversation_id.to_string(),
                    mem_task_id.to_string(),
                    project_path.clone(),
                    parent_run_id.to_string(),
                    subagents.config().max_tokens_per_child,
                    binding.clone(),
                    child_perm.to_string(),
                    child_allowlist.to_vec(),
                    child_profile_id.clone(),
                    child_directive.clone(),
                    child_max_steps,
                    failure_policy,
                    max_retries,
                    new_run_id,
                );
                return true;
            }
            Err(requeue_error) => {
                // Fall through to failure handling with the enriched message.
                let enriched = format!("{message}; requeue failed: {requeue_error}");
                let _ = subagents
                    .update_status(mem_task_id, SubAgentStatus::Failed(enriched.clone()))
                    .await;
                finalize_child_failure(
                    events,
                    subagents,
                    task_outputs,
                    parent_run_id,
                    session_id,
                    task_id,
                    child_run_id,
                    project_path,
                    &enriched,
                    failure_policy,
                )
                .await;
                return false;
            }
        }
    }

    finalize_child_failure(
        events,
        subagents,
        task_outputs,
        parent_run_id,
        session_id,
        task_id,
        child_run_id,
        project_path,
        message,
        failure_policy,
    )
    .await;
    false
}

/// Shared tail of child failure: release slot + close session + emit parent
/// event + apply FailFast/RequireAll parent action + update task record.
#[allow(clippy::too_many_arguments)] // terminal settlement needs full context
async fn finalize_child_failure(
    events: EventSequencer,
    subagents: &Arc<SubAgentManager>,
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: &str,
    session_id: &str,
    task_id: &str,
    child_run_id: &str,
    project_path: &Option<String>,
    message: &str,
    failure_policy: FailurePolicy,
) {
    let _ = crate::subagent_store::release_subagent_slot(session_id, Some(message));
    let _ = crate::subagent_store::close_subagent_session(session_id, "failed", Some(message));
    let task_output = message.to_string();
    let _ = events.append_checked(
        parent_run_id,
        RunEventKind::SubagentFailed {
            sub_run_id: child_run_id.to_string(),
            error: message.to_string(),
        },
    );
    // FailFast fails the parent immediately; RequireAll waits until every
    // sibling has settled, then fails the parent on the aggregate failure.
    let (all_terminal, _any_failed) = sibling_settled_state(task_outputs, task_id).await;
    if matches!(
        failure_policy.on_child_failed(all_terminal, 0),
        ChildFailureEffect::FailParent
    ) {
        fail_parent_and_cancel_siblings(
            subagents,
            parent_run_id,
            &format!("subagent failed under {:?}: {message}", failure_policy),
        )
        .await;
    }
    task_outputs.lock().await.insert(
        task_id.to_string(),
        TaskRecord {
            run_id: child_run_id.to_string(),
            status: "failed".into(),
            output: Some(task_output),
        },
    );
    fire_subagent_stop(project_path, parent_run_id, child_run_id, "failed", message).await;
}
