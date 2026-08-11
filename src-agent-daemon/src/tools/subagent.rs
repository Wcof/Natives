//! Subagent / task execution for the gated tool runtime.
//!
//! Aggregate module (ARCH-002 split): the constants, the child-spawn progress
//! helper, and the `PermissionGatedTools` tree/budget + route-assignment
//! methods stay here; the task-tool execution, background watcher, terminal
//! settlement and retry/requeue helpers live in the sibling modules
//! `subagent_execute`, `subagent_watcher`, `subagent_terminal` and
//! `subagent_requeue`. External paths (`crate::tools::subagent::*`) are
//! preserved via re-exports below.

use agent_core::{EventSequencer, SubAgentStatus, ToolProgressSink, ToolProgressUpdate};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::plan_mode;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

use super::gated::PermissionGatedTools;

// Helpers relocated to `subagent_requeue`; re-exported here so the external
// paths `crate::tools::subagent::redact_task_input_system_prompt` and
// `crate::tools::subagent::fail_parent_and_cancel_siblings` keep working.
pub(crate) use super::subagent_requeue::directive_for_requeue;
pub(crate) use super::subagent_requeue::fail_parent_and_cancel_siblings;
pub use super::subagent_requeue::redact_task_input_system_prompt;

// ── NE-P0-05 §19.5: Run-level frozen Hook Dispatcher for the subagent lifecycle ──
//
// `SubagentStart` and `SubagentStop` are subagent lifecycle hooks. They must
// dispatch through the Run's *frozen* Hook Dispatcher — the same read-only plan
// the permission gate and the notification hook already use — rather than
// re-scanning `hooks.json` on every spawn or terminal settlement. A mid-Run
// `hooks.json` edit only reaches the *next* Run.
//
// These helpers are the seam `subagent_execute` (SubagentStart) and
// `subagent_requeue` (SubagentStop) should call instead of
// `build_production_hooks_for_project`. They resolve the Run's frozen
// dispatcher once and delegate, so every lifecycle event emits
// HookInvocationStarted / Completed telemetry through the same
// [`EventSequencer`] the engine uses — a single durable source.

/// Resolve the Run's frozen Hook Dispatcher for the subagent lifecycle, shared
/// with the permission gate and the notification hook. Re-scanning
/// `hooks.json` happens at most once per Run (the lazy compile fallback), and
/// never on a lifecycle event once the Run is frozen.
pub(crate) fn frozen_subagent_dispatcher(
    parent_run_id: &str,
    events: agent_core::EventSequencer,
    project: Option<&std::path::Path>,
) -> crate::production_hooks::FrozenHookDispatcher {
    crate::production_hooks::resolve_frozen_dispatcher_for_subagent(parent_run_id, events, project)
}

/// Dispatch `SubagentStart` through the Run's frozen Hook Dispatcher.
///
/// `project_path` is the project root (used only for the lazy compile fallback
/// when the Run has not been frozen yet). Returns the hook responses so the
/// caller can aggregate them (`FrozenHookDispatcher::aggregate_allow`).
pub(crate) async fn fire_subagent_start_frozen(
    parent_run_id: &str,
    events: agent_core::EventSequencer,
    project_path: &Option<String>,
    input: &Value,
) -> Vec<agent_core::HookResponse> {
    let dispatcher = frozen_subagent_dispatcher(
        parent_run_id,
        events,
        project_path.as_deref().map(std::path::Path::new),
    );
    dispatcher
        .dispatch(agent_core::HookRequest {
            event: agent_core::HookEvent::SubagentStart,
            run_id: parent_run_id.to_string(),
            tool_name: Some("task".into()),
            input: input.clone(),
        })
        .await
}

/// Dispatch `SubagentStop` through the Run's frozen Hook Dispatcher.
///
/// `project_path` is the project root (used only for the lazy compile fallback
/// when the Run has not been frozen yet). The responses are returned for
/// telemetry; a `SubagentStop` is an observation event and the caller does not
/// gate on the verdict.
pub(crate) async fn fire_subagent_stop_frozen(
    parent_run_id: &str,
    events: agent_core::EventSequencer,
    project_path: &Option<String>,
    input: &Value,
) {
    let dispatcher = frozen_subagent_dispatcher(
        parent_run_id,
        events,
        project_path.as_deref().map(std::path::Path::new),
    );
    let _ = dispatcher
        .dispatch(agent_core::HookRequest {
            event: agent_core::HookEvent::SubagentStop,
            run_id: parent_run_id.to_string(),
            tool_name: Some("task".into()),
            input: input.clone(),
        })
        .await;
}

/// Step budget for a subagent turn loop when neither the caller nor the
/// selected agent profile asks for one.
pub const DEFAULT_CHILD_MAX_STEPS: u32 = 15;

/// Ceiling on a subagent step budget. A parent agent (or a prompt-injected one)
/// must not be able to buy an unbounded child loop by asking for a huge number;
/// the tool-call and token ledgers in `SubAgentManager` bound cost too, this
/// bounds wall-clock turns.
pub const MAX_CHILD_MAX_STEPS: u32 = 100;

/// Ceiling on `max_retries` for a Retry failure policy. Retries are bounded so
/// a failing child cannot re-queue itself (or be re-queued by a parent) forever.
pub const MAX_SUBAGENT_RETRIES: u32 = 5;

/// Ceiling on the parent-authored child system prompt, in UTF-8 bytes. Long
/// enough for a real persona brief, short enough that it cannot crowd out the
/// child's own context budget. Over the limit is an error, never a silent
/// truncation — a truncated persona is worse than a rejected one.
pub const MAX_CHILD_SYSTEM_PROMPT_BYTES: usize = 16_000;

pub(crate) fn spawn_subagent_progress(
    events: EventSequencer,
    parent_run_id: String,
    tool_call_id: String,
    child_run_id: String,
    progress: Arc<dyn ToolProgressSink>,
    turn_id: Option<String>,
    message_id: Option<String>,
) {
    tokio::spawn(async move {
        let mut cursor = 0u64;
        let mut pending = String::new();
        let mut last_emit = Instant::now() - Duration::from_millis(50);
        for _ in 0..3_600 {
            let child_events = match events.replay_after_checked(&child_run_id, cursor) {
                Ok(events) => events,
                Err(_) => break,
            };
            for event in child_events {
                cursor = cursor.max(event.run_sequence);
                match event.payload {
                    RunEventKind::TextDelta { text } | RunEventKind::ReasoningDelta { text } => {
                        pending.push_str(&text)
                    }
                    RunEventKind::Progress { message, .. } => {
                        pending.push_str(&message);
                    }
                    _ => {}
                }
            }
            if !pending.is_empty() && last_emit.elapsed() >= Duration::from_millis(50) {
                let text = std::mem::take(&mut pending);
                progress
                    .publish(ToolProgressUpdate {
                        run_id: parent_run_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool_name: "task".into(),
                        stream: "subagent".into(),
                        text,
                        final_update: false,
                        turn_id: turn_id.clone(),
                        message_id: message_id.clone(),
                        progress_sequence: 0,
                    })
                    .await;
                last_emit = Instant::now();
            }
            let Some(run) = crate::global_run_manager().get_run(&child_run_id) else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            if run.status.is_terminal() {
                if !pending.is_empty() {
                    progress
                        .publish(ToolProgressUpdate {
                            run_id: parent_run_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                            tool_name: "task".into(),
                            stream: "subagent".into(),
                            text: std::mem::take(&mut pending),
                            final_update: false,
                            turn_id: turn_id.clone(),
                            message_id: message_id.clone(),
                            progress_sequence: 0,
                        })
                        .await;
                }
                progress
                    .publish(ToolProgressUpdate {
                        run_id: parent_run_id,
                        tool_call_id: tool_call_id.clone(),
                        tool_name: "task".into(),
                        stream: "subagent".into(),
                        text: run.status.as_str().to_string(),
                        final_update: true,
                        turn_id,
                        message_id,
                        progress_sequence: 0,
                    })
                    .await;
                progress.mark_tool_call_settled(&tool_call_id).await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

impl PermissionGatedTools {
    /// If this tools instance is running as a subagent child, return (child_run_id, tree_root).
    pub(crate) async fn subagent_budget_ids(&self) -> Option<(String, String)> {
        // parent_run_id field is the current run for this tools instance.
        let child_run = self.parent_run_id.clone();
        let run = crate::run_manager::global_run_manager().get_run(&child_run)?;
        let parent = run.parent_run_id.clone()?;
        // tree root = walk parents until none
        let mut root = parent.clone();
        let mut guard = 0;
        while guard < 32 {
            guard += 1;
            let Some(r) = crate::run_manager::global_run_manager().get_run(&root) else {
                break;
            };
            match r.parent_run_id {
                Some(p) => root = p,
                None => break,
            }
        }
        Some((child_run, root))
    }

    /// Same tree-cancel rules as ProductionRuntime::cancel_run_tree, using
    /// the shared engines/subagents/events maps held by this tool runtime.
    async fn cancel_run_tree_local(&self, run_id: &str) {
        // A cancelled run has no plan to approve. Dropping the session here is
        // safe in a way that eviction is not: the run is gone, so there is no
        // latch left to reopen.
        plan_mode::clear(run_id);
        if let Some(rt) = &self.runtime {
            rt.cancel_run_tree(run_id).await;
            return;
        }
        let descendants = self.subagents.list_descendants(run_id).await;
        let mut run_ids: Vec<String> = vec![run_id.to_string()];
        for d in &descendants {
            if !run_ids.contains(&d.run_id) {
                run_ids.push(d.run_id.clone());
            }
            plan_mode::clear(&d.run_id);
        }
        {
            let engines = self.engines.lock().await;
            for rid in &run_ids {
                if let Some(engine) = engines.get(rid) {
                    engine.request_cancel();
                }
            }
        }
        for d in &descendants {
            let _ = self
                .subagents
                .update_status(&d.id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&d.id) {
                rec.status = "cancelled".into();
            }
        }
        let _ = self.subagents.cascade_cancel_metadata(run_id).await;
        // No terminal lifecycle events here — RunManager::cancel is the sole committer.
    }

    pub(crate) async fn kill_task_tree(&self, task_id: &str) -> bool {
        let child_run_id = self
            .task_outputs
            .lock()
            .await
            .get(task_id)
            .map(|r| r.run_id.clone());
        let Some(rid) = child_run_id else {
            return false;
        };
        self.cancel_run_tree_local(&rid).await;
        let _ = self
            .subagents
            .update_status(task_id, SubAgentStatus::Cancelled)
            .await;
        if let Some(rec) = self.task_outputs.lock().await.get_mut(task_id) {
            rec.status = "cancelled".into();
        }
        true
    }

    fn default_binding(&self) -> crate::subagent_store::RouteBinding {
        crate::subagent_store::RouteBinding {
            provider_id: self.provider_id.clone(),
            key_id: self
                .key_id
                .clone()
                .filter(|k| !k.trim().is_empty() && !k.eq_ignore_ascii_case("auto"))
                .unwrap_or_default(),
            model_id: self.model_id.clone(),
        }
    }

    /// One assignment interaction for the entire task batch (full tasks[] + default_binding).
    /// Returns call_id → binding map (default mode maps every call_id to default_binding).
    async fn resolve_batch_assignment(
        &self,
        tasks: &[(String, String, String)],
    ) -> Result<std::collections::HashMap<String, crate::subagent_store::RouteBinding>, String>
    {
        use std::collections::HashMap;

        let use_fixture = std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let default_binding = self.default_binding();

        // Existing policy: assign from pool without UI.
        if let Some(policy) = crate::subagent_store::get_route_policy(&self.conversation_id)
            .ok()
            .flatten()
        {
            // If the stored policy has no bindings (e.g. because the policy was
            // saved before a provider reset or after a user cancellation), fall
            // back to the main-session credential rather than hard-failing.
            if policy.bindings.is_empty() {
                if !default_binding.key_id.trim().is_empty() {
                    // Repair the stale policy so next subagent benefits too.
                    let _ = crate::subagent_store::upsert_route_policy(
                        &self.conversation_id,
                        "default",
                        std::slice::from_ref(&default_binding),
                    );
                    let mut map = HashMap::new();
                    for (call_id, _, _) in tasks {
                        map.insert(call_id.clone(), default_binding.clone());
                    }
                    return Ok(map);
                }
                // No usable default either — drop the broken policy row so the
                // assignment interaction is shown to the user on the next call.
                let _ = crate::subagent_store::delete_route_policy(&self.conversation_id);
                // Fall through to assignment interaction below.
            } else {
                let mut map = HashMap::new();
                let mut attempted = Vec::new();
                for (call_id, _, _) in tasks {
                    let b = crate::subagent_store::pick_binding(&policy, &attempted)?;
                    attempted.push(b.clone());
                    // Prefer not repeating until pool exhausted (pick_binding already cycles).
                    map.insert(call_id.clone(), b);
                }
                return Ok(map);
            }
        }

        if use_fixture {
            // Offline unit tests: still ignore model-supplied credentials unless
            // NATIVES_DAEMON_FIXTURE_HONOR_TASK_CREDS=1 (legacy identity tests).
            let honor = std::env::var("NATIVES_DAEMON_FIXTURE_HONOR_TASK_CREDS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let mut map = HashMap::new();
            for (call_id, _, _) in tasks {
                let b = if honor {
                    // Caller may pass creds via ambient: use default_binding only;
                    // honor path is for resolve_task_binding single-task tests that
                    // still set fixture env — fall through to default.
                    default_binding.clone()
                } else {
                    crate::subagent_store::RouteBinding {
                        provider_id: if default_binding.provider_id.is_empty() {
                            "fixture".into()
                        } else {
                            default_binding.provider_id.clone()
                        },
                        key_id: if default_binding.key_id.is_empty() {
                            "fixture-key".into()
                        } else {
                            default_binding.key_id.clone()
                        },
                        model_id: if default_binding.model_id.is_empty() {
                            "fixture-model".into()
                        } else {
                            default_binding.model_id.clone()
                        },
                    }
                };
                map.insert(call_id.clone(), b);
            }
            return Ok(map);
        }

        let Some(rt) = self.runtime.clone() else {
            return Err(
                "no route policy and no runtime for subagent_assignment interaction".into(),
            );
        };

        // Only one assignment interaction per parent conversation at a time.
        let interaction_id = {
            let mut inflight = rt
                .assignment_inflight
                .lock()
                .map_err(|e| format!("assignment_inflight lock: {e}"))?;
            if let Some(existing) = inflight.get(&self.conversation_id) {
                // Another batch already waiting — still wait on same interaction.
                existing.clone()
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                inflight.insert(self.conversation_id.clone(), id.clone());
                id
            }
        };

        let (tx, rx) = oneshot::channel::<Value>();
        let mut installed = false;
        {
            let mut waiters = rt
                .assignment_waiters
                .lock()
                .map_err(|e| format!("assignment_waiters lock: {e}"))?;
            if !waiters.contains_key(&interaction_id) {
                waiters.insert(interaction_id.clone(), tx);
                installed = true;
                let batch_id = uuid::Uuid::new_v4().to_string();
                let tasks_payload: Vec<Value> = tasks
                    .iter()
                    .map(|(call_id, name, prompt)| {
                        serde_json::json!({
                            "call_id": call_id,
                            "name": name,
                            "prompt": prompt,
                        })
                    })
                    .collect();
                let payload = serde_json::json!({
                    "kind": "subagent_assignment",
                    "batch_id": batch_id,
                    "parent_conversation_id": self.conversation_id,
                    "parent_run_id": self.parent_run_id,
                    "conversation_id": self.conversation_id,
                    "run_id": self.parent_run_id,
                    "default_binding": {
                        "provider_id": default_binding.provider_id,
                        "key_id": default_binding.key_id,
                        "model_id": default_binding.model_id,
                    },
                    "tasks": tasks_payload,
                    "reason": "Assign provider/key/model for subagent tasks in this conversation",
                });
                let _ = crate::interaction_store::insert_pending(
                    &interaction_id,
                    Some(&self.parent_run_id),
                    Some(&self.conversation_id),
                    "subagent_assignment",
                    payload.clone(),
                );
                self.events.append(
                    &self.parent_run_id,
                    RunEventKind::InteractionRequested {
                        interaction_id: interaction_id.clone(),
                        kind: "subagent_assignment".into(),
                        payload,
                    },
                );
            } else {
                drop(tx);
            }
        }

        let response = if installed {
            match tokio::time::timeout(Duration::from_secs(120), rx).await {
                Ok(Ok(v)) => v,
                Ok(Err(_)) => {
                    if let Ok(mut i) = rt.assignment_inflight.lock() {
                        i.remove(&self.conversation_id);
                    }
                    return Err("subagent assignment cancelled".into());
                }
                Err(_) => {
                    if let Ok(mut w) = rt.assignment_waiters.lock() {
                        w.remove(&interaction_id);
                    }
                    if let Ok(mut i) = rt.assignment_inflight.lock() {
                        i.remove(&self.conversation_id);
                    }
                    return Err("subagent assignment timed out".into());
                }
            }
        } else {
            // Wait for policy written by the owner of the oneshot.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            loop {
                if let Some(policy) = crate::subagent_store::get_route_policy(&self.conversation_id)
                    .ok()
                    .flatten()
                {
                    let mut map = HashMap::new();
                    let mut attempted = Vec::new();
                    for (call_id, _, _) in tasks {
                        let b = crate::subagent_store::pick_binding(&policy, &attempted)?;
                        attempted.push(b.clone());
                        map.insert(call_id.clone(), b);
                    }
                    return Ok(map);
                }
                let still = rt
                    .assignment_inflight
                    .lock()
                    .map(|g| g.contains_key(&self.conversation_id))
                    .unwrap_or(false);
                if !still {
                    return Err("subagent assignment cancelled".into());
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err("subagent assignment timed out".into());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };

        // Cancel / deny → fail the batch (no silent default key).
        if response
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || response.get("approved").and_then(|v| v.as_bool()) == Some(false)
        {
            if let Ok(mut i) = rt.assignment_inflight.lock() {
                i.remove(&self.conversation_id);
            }
            return Err("subagent assignment cancelled".into());
        }

        let mode = response
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("default");

        // Prefer per-call assignments; fall back to pool/bindings.
        let assignments: Vec<Value> = response
            .get("assignments")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut map = HashMap::new();
        // A caller can explicitly confirm a usable main-session route when
        // an older parent run did not persist default_binding.key_id. Use that
        // confirmed route before falling back to the legacy default binding.
        if !assignments.is_empty() {
            for a in &assignments {
                let call_id = a
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let b = crate::subagent_store::RouteBinding {
                    provider_id: a
                        .get("provider_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    key_id: a
                        .get("key_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    model_id: a
                        .get("model_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                };
                crate::production::validate_route_binding(&b)?;
                if !call_id.is_empty() {
                    map.insert(call_id, b);
                }
            }
        } else if mode == "default" {
            if default_binding.key_id.trim().is_empty() {
                if let Ok(mut i) = rt.assignment_inflight.lock() {
                    i.remove(&self.conversation_id);
                }
                return Err(
                    "default_binding.key_id missing on parent run; cannot confirm default mode"
                        .into(),
                );
            }
            crate::production::validate_route_binding(&default_binding)?;
            for (call_id, _, _) in tasks {
                map.insert(call_id.clone(), default_binding.clone());
            }
        } else {
            let bindings: Vec<crate::subagent_store::RouteBinding> = response
                .get("pool")
                .or_else(|| response.get("bindings"))
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            if bindings.is_empty() {
                if let Ok(mut i) = rt.assignment_inflight.lock() {
                    i.remove(&self.conversation_id);
                }
                return Err("subagent assignment response missing bindings".into());
            }
            for b in &bindings {
                crate::production::validate_route_binding(b)?;
            }
            for (pi, (call_id, _, _)) in tasks.iter().enumerate() {
                map.insert(call_id.clone(), bindings[pi % bindings.len()].clone());
            }
        }

        // Partial custom assignments may use the confirmed pool for remaining tasks.
        if !assignments.is_empty() && map.len() < tasks.len() {
            let pool: Vec<crate::subagent_store::RouteBinding> = response
                .get("pool")
                .or_else(|| response.get("bindings"))
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let mut pi = 0usize;
            #[allow(clippy::explicit_counter_loop)]
            // pi counts only processed items (continue skips)
            for (call_id, _, _) in tasks {
                if map.contains_key(call_id) || pool.is_empty() {
                    continue;
                }
                let b = pool[pi % pool.len()].clone();
                crate::production::validate_route_binding(&b)?;
                map.insert(call_id.clone(), b);
                pi += 1;
            }
        }

        if map.is_empty() {
            if let Ok(mut i) = rt.assignment_inflight.lock() {
                i.remove(&self.conversation_id);
            }
            return Err("subagent assignment produced empty binding map".into());
        }

        // Persist pool for subsequent subagents (random/custom pool or default singleton).
        let pool_for_policy: Vec<crate::subagent_store::RouteBinding> = response
            .get("pool")
            .or_else(|| response.get("bindings"))
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| {
                map.values().cloned().fold(Vec::new(), |mut acc, b| {
                    if !acc.iter().any(|x| x == &b) {
                        acc.push(b);
                    }
                    acc
                })
            });
        if !pool_for_policy.is_empty() {
            let _ = crate::subagent_store::upsert_route_policy(
                &self.conversation_id,
                mode,
                &pool_for_policy,
            );
        }

        if let Ok(mut i) = rt.assignment_inflight.lock() {
            i.remove(&self.conversation_id);
        }
        Ok(map)
    }

    /// Single-task path: resolve one binding (uses batch assignment with one task).
    pub(crate) async fn resolve_task_binding(
        &self,
        _input: &Value,
    ) -> Result<crate::subagent_store::RouteBinding, String> {
        let call_id = _input
            .get("task_call_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let name = _input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let prompt = _input
            .get("prompt")
            .or_else(|| _input.get("task"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let map = self
            .resolve_batch_assignment(&[(call_id.clone(), name, prompt)])
            .await?;
        map.get(&call_id)
            .cloned()
            .or_else(|| map.into_values().next())
            .ok_or_else(|| "subagent assignment produced no binding".into())
    }
}
