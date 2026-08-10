//! Background watcher for one subagent child run (T05), split from `subagent.rs`:
//! polls the child to a terminal status, settles provider usage against the
//! durable budget incrementally, consumes the persisted failure policy, releases
//! the slot exactly once, and re-enters itself when `Retry` re-queues the child.

use agent_core::{EventSequencer, FailurePolicy, SubAgentManager, SubAgentStatus};
use assistant_protocol::v2::RunEventKind;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::production::TaskRecord;

use super::subagent_requeue::fail_parent_and_cancel_siblings;
use super::subagent_terminal::{
    child_completed_terminal, child_failed_terminal, usage_delta_from_events,
};

/// Launch the background watcher for one child run (T05).
///
/// It polls the child to a terminal status, settles provider usage against the
/// *durable* budget incrementally (cancelling the child the moment the budget
/// is exceeded), consumes the persisted failure policy, releases the slot
/// exactly once, and re-enters itself when `Retry` re-queues the child.
#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
pub(crate) fn spawn_subagent_watcher(
    events: EventSequencer,
    subagents: Arc<SubAgentManager>,
    task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: String,
    session_id: String,
    task_id: String,
    child_conversation_id: String,
    mem_task_id: String,
    child_run_id: String,
    project_path: Option<String>,
    child_timeout_ms: u64,
    tree_root_for_budget: String,
    max_tokens_per_tree: u64,
    child_max_tokens: u64,
    binding: crate::subagent_store::RouteBinding,
    child_perm: String,
    child_allowlist: Vec<String>,
    child_profile_id: Option<String>,
    child_directive: Option<String>,
    child_max_steps: u32,
    failure_policy: FailurePolicy,
    max_retries: u32,
) {
    tokio::spawn(async move {
        watch_subagent_run(
            events,
            subagents,
            task_outputs,
            parent_run_id,
            session_id,
            task_id,
            child_conversation_id,
            mem_task_id,
            project_path,
            child_timeout_ms,
            tree_root_for_budget,
            max_tokens_per_tree,
            child_max_tokens,
            binding,
            child_perm,
            child_allowlist,
            child_profile_id,
            child_directive,
            child_max_steps,
            failure_policy,
            max_retries,
            child_run_id,
            0,
            None,
        )
        .await;
    });
}

/// Re-enter the watcher for a Retry re-queue with a fresh child run id.
/// Same `async move` pattern as [`spawn_subagent_watcher`] so the spawned
/// future stays `Send` (EventSequencer is Send but not Sync, so a direct
/// `tokio::spawn(watch_subagent_run(...))` from inside an async fn is not).
#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
pub(crate) fn spawn_retry_watcher(
    events: EventSequencer,
    subagents: Arc<SubAgentManager>,
    task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: String,
    session_id: String,
    task_id: String,
    child_conversation_id: String,
    mem_task_id: String,
    project_path: Option<String>,
    tree_root_for_budget: String,
    child_max_tokens: u64,
    binding: crate::subagent_store::RouteBinding,
    child_perm: String,
    child_allowlist: Vec<String>,
    child_profile_id: Option<String>,
    child_directive: Option<String>,
    child_max_steps: u32,
    failure_policy: FailurePolicy,
    max_retries: u32,
    new_run_id: String,
) {
    tokio::spawn(async move {
        let timeout_ms = subagents.config().child_timeout_ms.max(1);
        let tree_cap = subagents.config().max_tokens_per_tree;
        watch_subagent_run(
            events,
            subagents,
            task_outputs,
            parent_run_id,
            session_id,
            task_id,
            child_conversation_id,
            mem_task_id,
            project_path,
            timeout_ms,
            tree_root_for_budget,
            tree_cap,
            child_max_tokens,
            binding,
            child_perm,
            child_allowlist,
            child_profile_id,
            child_directive,
            child_max_steps,
            failure_policy,
            max_retries,
            new_run_id,
            0,
            None,
        )
        .await;
    });
}

#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
async fn watch_subagent_run(
    events: EventSequencer,
    subagents: Arc<SubAgentManager>,
    task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    parent_run_id: String,
    session_id: String,
    task_id: String,
    child_conversation_id: String,
    mem_task_id: String,
    project_path: Option<String>,
    child_timeout_ms: u64,
    tree_root_for_budget: String,
    max_tokens_per_tree: u64,
    // Child max tokens is enforced by the durable `subagent_session` budget;
    // the in-memory ledger is kept for the tool-call hook only.
    _child_max_tokens: u64,
    binding: crate::subagent_store::RouteBinding,
    child_perm: String,
    child_allowlist: Vec<String>,
    child_profile_id: Option<String>,
    child_directive: Option<String>,
    child_max_steps: u32,
    failure_policy: FailurePolicy,
    max_retries: u32,
    child_run_id: String,
    mut cursor: u64,
    mut budget_exceeded: Option<String>,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(child_timeout_ms.max(1));
    loop {
        if tokio::time::Instant::now() >= deadline {
            // Timeout → unified cancel tree for the child, then fail it.
            crate::global_run_manager()
                .runtime
                .cancel_run(&child_run_id)
                .await;
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest {
                    run_id: child_run_id.clone(),
                })
                .await;
            let _ = crate::global_run_manager()
                .runtime
                .take_run_agent_directive(&child_run_id)
                .await;
            let message = format!("subagent timed out after {}ms", child_timeout_ms.max(1));
            let _ = crate::subagent_store::settle_subagent_usage(&session_id, 0, None);
            child_failed_terminal(
                events.clone(),
                &subagents,
                &task_outputs,
                &parent_run_id,
                &session_id,
                &task_id,
                &mem_task_id,
                &child_run_id,
                &project_path,
                &message,
                failure_policy,
                max_retries,
                &binding,
                &child_conversation_id,
                &child_perm,
                &child_allowlist,
                &child_profile_id,
                &child_directive,
                child_max_steps,
            )
            .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let Some(run) = crate::global_run_manager().get_run(&child_run_id) else {
            continue;
        };
        let status = run.status.as_str().to_string();

        // Incremental usage settle: replay events after the cursor, feed every
        // new UsageUpdated delta into the durable budget, and cancel the child
        // the moment the budget is exceeded — not only at terminal.
        match events.replay_after_checked(&child_run_id, cursor) {
            Ok(replayed) => {
                for e in &replayed {
                    cursor = cursor.max(e.run_sequence);
                }
                let delta = usage_delta_from_events(&replayed);
                if delta > 0 && budget_exceeded.is_none() {
                    // Keep the in-memory ledger aligned for tool-call hooks.
                    let _ = subagents
                        .settle_tokens(&child_run_id, &tree_root_for_budget, delta)
                        .await;
                    match crate::subagent_store::settle_subagent_usage(&session_id, delta, None) {
                        Err(e) => {
                            budget_exceeded = Some(e);
                        }
                        Ok(()) => {
                            if let Ok(tree_used) = crate::subagent_store::subagent_tree_tokens_used(
                                &tree_root_for_budget,
                            ) {
                                if tree_used > max_tokens_per_tree {
                                    budget_exceeded = Some(format!(
                                        "subagent tree token budget exceeded ({tree_used}/{max_tokens_per_tree})"
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                let message = format!("child event replay failed: {error}");
                let _ = subagents
                    .update_status(&mem_task_id, SubAgentStatus::Failed(message.clone()))
                    .await;
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&message));
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&message),
                );
                let _ = events.append_checked(
                    &parent_run_id,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: message.clone(),
                    },
                );
                task_outputs.lock().await.insert(
                    task_id.clone(),
                    TaskRecord {
                        run_id: child_run_id.clone(),
                        status: "failed".into(),
                        output: Some(message),
                    },
                );
                return;
            }
        }

        if budget_exceeded.is_some() {
            // Budget reached → the child cannot be Completed. Cancel any live
            // run (idempotent when already terminal) and fail it.
            crate::global_run_manager()
                .runtime
                .cancel_run(&child_run_id)
                .await;
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest {
                    run_id: child_run_id.clone(),
                })
                .await;
            let message = budget_exceeded.clone().unwrap_or_default();
            child_failed_terminal(
                events.clone(),
                &subagents,
                &task_outputs,
                &parent_run_id,
                &session_id,
                &task_id,
                &mem_task_id,
                &child_run_id,
                &project_path,
                &message,
                failure_policy,
                0,
                &binding,
                &child_conversation_id,
                &child_perm,
                &child_allowlist,
                &child_profile_id,
                &child_directive,
                child_max_steps,
            )
            .await;
            return;
        }

        if !run.status.is_terminal() {
            continue;
        }

        // ── Terminal handling ──
        let child_events = match events.replay_after_checked(&child_run_id, cursor) {
            Ok(events) => events,
            Err(error) => {
                let message = format!("child event replay failed: {error}");
                let _ = subagents
                    .update_status(&mem_task_id, SubAgentStatus::Failed(message.clone()))
                    .await;
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&message));
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&message),
                );
                let _ = events.append_checked(
                    &parent_run_id,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: message.clone(),
                    },
                );
                task_outputs.lock().await.insert(
                    task_id.clone(),
                    TaskRecord {
                        run_id: child_run_id.clone(),
                        status: "failed".into(),
                        output: Some(message),
                    },
                );
                return;
            }
        };
        let final_delta = usage_delta_from_events(&child_events);
        if final_delta > 0 && budget_exceeded.is_none() {
            let _ = subagents
                .settle_tokens(&child_run_id, &tree_root_for_budget, final_delta)
                .await;
            if let Err(e) =
                crate::subagent_store::settle_subagent_usage(&session_id, final_delta, None)
            {
                budget_exceeded = Some(e);
            }
        }
        if let Some(message) = budget_exceeded {
            let _ = subagents
                .update_status(&mem_task_id, SubAgentStatus::Failed(message.clone()))
                .await;
            let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&message));
            let _ = crate::subagent_store::close_subagent_session(
                &session_id,
                "failed",
                Some(&message),
            );
            let _ = events.append_checked(
                &parent_run_id,
                RunEventKind::SubagentFailed {
                    sub_run_id: child_run_id.clone(),
                    error: message.clone(),
                },
            );
            task_outputs.lock().await.insert(
                task_id.clone(),
                TaskRecord {
                    run_id: child_run_id.clone(),
                    status: "failed".into(),
                    output: Some(message.clone()),
                },
            );
            fail_parent_and_cancel_siblings(
                &subagents,
                &parent_run_id,
                &format!("subagent budget exceeded: {message}"),
            )
            .await;
            return;
        }

        let text = child_events
            .iter()
            .filter_map(|e| match &e.payload {
                RunEventKind::TextDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<String>();
        if status == "completed" {
            child_completed_terminal(
                events.clone(),
                &subagents,
                &task_outputs,
                &parent_run_id,
                &session_id,
                &task_id,
                &mem_task_id,
                &child_run_id,
                &project_path,
                &text,
                failure_policy,
            )
            .await;
            return;
        }

        let err_msg = run.error_code.clone().unwrap_or_else(|| status.clone());
        let message = if text.trim().is_empty() {
            err_msg
        } else {
            format!("{err_msg}: {text}")
        };
        child_failed_terminal(
            events.clone(),
            &subagents,
            &task_outputs,
            &parent_run_id,
            &session_id,
            &task_id,
            &mem_task_id,
            &child_run_id,
            &project_path,
            &message,
            failure_policy,
            max_retries,
            &binding,
            &child_conversation_id,
            &child_perm,
            &child_allowlist,
            &child_profile_id,
            &child_directive,
            child_max_steps,
        )
        .await;
        return;
    }
}
