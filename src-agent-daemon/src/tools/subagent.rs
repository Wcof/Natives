//! Subagent / task execution for the gated tool runtime.

use agent_core::{
    default_subagent_tool_allowlist, ChildFailureEffect, EventSequencer, FailurePolicy, HookEvent,
    HookRegistry, HookRequest, SubAgentManager, SubAgentStatus, ToolExecutionResult,
    ToolProgressSink, ToolProgressUpdate,
};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::plan_mode;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex};

use crate::production::TaskRecord;

use super::gated::PermissionGatedTools;

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
                cursor = cursor.max(event.effective_run_sequence());
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

    pub(crate) async fn execute_task(&self, input: Value) -> ToolExecutionResult {
        let prompt = input
            .get("prompt")
            .or_else(|| input.get("task"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if prompt.trim().is_empty() {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "prompt required for task"}),
                is_error: true,
                duration_ms: 0,
            };
        }
        // Credential fields from the model are intentionally ignored (route policy assigns).
        let _ignored_provider = input.get("provider_id");
        let _ignored_key = input.get("key_id");
        let _ignored_model = input.get("model_id");

        // Expert team delegation (ADR-0016): `agent` must name a roster member;
        // outside the roster (or without a team) it is rejected fail-closed.
        let agent_param = input
            .get("agent")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let member_profile = match (&self.team, agent_param) {
            (Some(team), Some(agent)) => {
                if !team.members.iter().any(|m| m.expert_id == agent) {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!(
                                "agent '{agent}' is not a member of team '{}'",
                                team.team_id
                            ),
                            "code": "TEAM_MEMBER_INVALID",
                            "members": team.members.iter().map(|m| m.expert_id.clone()).collect::<Vec<_>>(),
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
                match crate::capability_resolution::load_profile(
                    agent,
                    self.gateway
                        .project_root
                        .as_deref()
                        .map(std::path::Path::new),
                ) {
                    Some(profile) => Some((agent.to_string(), profile)),
                    None => {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error": format!("team member profile not loadable: {agent}"),
                                "code": "TEAM_MEMBER_INVALID",
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                }
            }
            (None, Some(agent)) => {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!(
                            "'agent' parameter ('{agent}') requires an active expert team selection"
                        ),
                        "code": "TEAM_NOT_ACTIVE",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
            (_, None) => None,
        };
        let member_profile_id = member_profile.as_ref().map(|(id, _)| id.clone());

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // ── Persona: an on-disk profile the parent picked, plus a prompt it wrote ──
        //
        // `subagent_type` is the Claude-Code-compatible name; the daemon's own
        // field name is accepted too. A profile the parent names but that does
        // not exist is an error, never a silent fallback to "no persona".
        let requested_profile_id = input
            .get("subagent_type")
            .or_else(|| input.get("agent_profile_id"))
            .or_else(|| input.get("agent_type"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let project_root_path = self
            .gateway
            .project_root
            .as_deref()
            .map(std::path::Path::new);
        let profile = match &requested_profile_id {
            Some(id) => match agent_core::load_agent_profile(id, project_root_path) {
                Some(p) => Some(p),
                None => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!("agent profile `{id}` not found in the project or user profile directories"),
                            "code": "agent_profile_not_found",
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            },
            None => None,
        };

        // Parent-authored system prompt for this child. Prompt text only — it is
        // never consulted when resolving permissions or the tool surface.
        let child_directive = match input
            .get("system_prompt")
            .or_else(|| input.get("agent_prompt"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(sp) if sp.len() > MAX_CHILD_SYSTEM_PROMPT_BYTES => {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!(
                            "system_prompt is {} bytes; the limit is {MAX_CHILD_SYSTEM_PROMPT_BYTES}. Put task detail in `prompt`, not the persona.",
                            sp.len()
                        ),
                        "code": "system_prompt_too_long",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
            Some(sp) => Some(sp.to_string()),
            None => None,
        };

        // ── Permission and tool surface ──
        //
        // Both resolutions live in `agent_core::subagents`: the profile can only
        // tighten what was requested, and the parent is a hard ceiling. Neither a
        // parent-authored prompt nor a self-selected profile can widen either one,
        // so a prompt-injected parent gains nothing by writing a hostile persona.
        // The profile mode fed to the valve is the persona the parent named, or
        // — when the child is a roster member instead — the member's own profile
        // (ADR-0016). Either way it can only tighten: `resolve_child_permission`
        // caps the request by the profile and then by the parent.
        let child_perm = agent_core::resolve_child_permission(
            &self.permission_profile,
            input.get("permission_profile").and_then(|v| v.as_str()),
            profile
                .as_ref()
                .and_then(|p| p.permission_mode.as_deref())
                .or_else(|| {
                    member_profile
                        .as_ref()
                        .and_then(|(_, m)| m.permission_mode.as_deref())
                }),
        );
        // A present-but-malformed `tool_allowlist` falls back to the readonly
        // default rather than to "inherit the parent surface".
        let requested_allowlist: Option<Vec<String>> = input.get("tool_allowlist").map(|value| {
            value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(default_subagent_tool_allowlist)
        });
        let mut child_allowlist = agent_core::resolve_child_tool_allowlist(
            self.tool_allowlist.as_deref(),
            requested_allowlist.as_deref(),
            profile.as_ref().and_then(|p| p.tools.as_deref()),
            profile.as_ref().and_then(|p| p.disallowed_tools.as_deref()),
        );
        // Member persona narrows the surface further (ADR-0016): profile.tools
        // intersects what survived the parent ceiling, minus disallowed. Both
        // operations are `retain`, so a member profile can only remove tools —
        // naming a roster member never buys reach the parent lacked.
        if let Some((_, member)) = &member_profile {
            if let Some(tools) = member.tools.as_ref().filter(|t| !t.is_empty()) {
                child_allowlist.retain(|tool| tools.iter().any(|t| t == tool));
            }
            if let Some(disallowed) = member.disallowed_tools.as_ref() {
                child_allowlist.retain(|tool| !disallowed.iter().any(|d| d == tool));
            }
        }
        let child_allowlist = child_allowlist;

        // ── Step budget: request, else the profile's, else the daemon default ──
        let child_max_steps = input
            .get("max_steps")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(u32::MAX as u64) as u32)
            .or_else(|| profile.as_ref().and_then(|p| p.max_steps))
            .or_else(|| member_profile.as_ref().and_then(|(_, m)| m.max_steps))
            .unwrap_or(DEFAULT_CHILD_MAX_STEPS)
            .clamp(1, MAX_CHILD_MAX_STEPS);

        // ── T05: failure policy + budget (persisted on the reservation) ──
        //
        // The policy decides what a terminal child failure does to the parent:
        // Isolate (default) keeps the parent running, FailFast fails it and
        // cancels siblings, RequireAll aggregates a batch failure, Retry
        // re-queues transient provider failures up to `max_retries`. Budgets
        // are capped by the daemon config so a parent can never buy a child
        // bigger than the machine-wide ceiling.
        let failure_policy = input
            .get("failure_policy")
            .and_then(|v| v.as_str())
            .map(FailurePolicy::parse)
            .unwrap_or(self.subagents.config().failure_policy);
        let max_retries = input
            .get("max_retries")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(MAX_SUBAGENT_RETRIES as u64) as u32)
            .unwrap_or(0);
        let child_max_tokens = input
            .get("max_tokens")
            .or_else(|| input.get("max_budget_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v.min(self.subagents.config().max_tokens_per_child))
            .unwrap_or(self.subagents.config().max_tokens_per_child);
        let child_max_cost = input.get("max_cost_usd").and_then(|v| v.as_f64());

        // Persisted on the child run row and reported to every observer of this
        // spawn. A roster member wins over a parent-named persona because child
        // capability resolve loads the member's skills from this id (ADR-0016);
        // without a team it is the persona `ProductionRuntime::start_run`
        // reloads for the system prompt, tools and token budget.
        let child_profile_id = member_profile_id
            .clone()
            .or_else(|| requested_profile_id.clone());

        // Prefer binding injected by execute_task_batch; else resolve (single-task path).
        let binding = if let Some(b) = input
            .get("_resolved_binding")
            .cloned()
            .and_then(|v| serde_json::from_value::<crate::subagent_store::RouteBinding>(v).ok())
        {
            b
        } else {
            match self.resolve_task_binding(&input).await {
                Ok(b) => b,
                Err(e) => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": e,
                            "code": "subagent_assignment_failed",
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            }
        };
        let child_provider = binding.provider_id.clone();
        let child_key = binding.key_id.clone();
        let child_model = binding.model_id.clone();

        // Persist hidden child conversation + subagent_session (real IDs).
        let _ = crate::conversation_store::ensure_conversation_stub(
            &self.conversation_id,
            &self.provider_id,
            &self.model_id,
            Some(&self.permission_profile),
            None,
        );
        let task_call_id = input
            .get("task_call_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let (session_id, child_conversation_id) =
            match crate::subagent_store::create_hidden_child_session(
                &self.conversation_id,
                Some(&self.parent_run_id),
                task_call_id,
                &name,
                &prompt,
                &binding,
                Some(&child_perm),
                self.gateway.project_root.as_deref(),
            ) {
                Ok(v) => v,
                Err(e) => {
                    if use_fixture_flag(&input) {
                        let sid = uuid::Uuid::new_v4().to_string();
                        let cid = uuid::Uuid::new_v4().to_string();
                        (sid, cid)
                    } else {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error": format!("create child session failed: {e}"),
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                }
            };
        // Persist the child scope so a route restart restores it exactly
        // (migration 029). N05: bind the child to the parent's REAL project
        // identity (id + version) — never write the project path into
        // project_id. A persist failure fails closed BEFORE any child run is
        // created.
        //
        // Fixture mode (offline tests) has no durable store: the session is a
        // fake id and there is nothing to persist, so the DB steps are skipped
        // to keep the fixture path hermetic.
        let identity = self.verified_project_identity().await;
        let fixture_mode = use_fixture_flag(&input);
        let project_id_for_scope = identity
            .as_ref()
            .map(|i| i.project_id.clone())
            .or_else(|| self.gateway.project_root.clone());
        if !fixture_mode {
            if let Err(error) = crate::subagent_store::persist_subagent_scope(
                &session_id,
                &crate::subagent_store::SubagentScope {
                    project_path: self.gateway.project_root.clone(),
                    project_id: project_id_for_scope.clone(),
                    project_identity_version: identity.as_ref().map(|i| i.identity_version as i64),
                    permission_profile: Some(child_perm.clone()),
                    agent_profile_id: child_profile_id.clone(),
                    max_steps: Some(child_max_steps as i64),
                    tool_allowlist: child_allowlist.clone(),
                },
            ) {
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&error),
                );
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!("persist child scope failed: {error}"),
                        "code": "PERSISTENCE_FAILED",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        // Standard RunManager path: create_run + start_detached (no embedded Engine).
        let project_path = self.gateway.project_root.clone();

        // SubagentStart runs before the child exists, so a hook can refuse the
        // spawn while refusing is still free. Denial is honoured rather than
        // logged: this event is the only place a policy can stop a run from
        // fanning out, and a hook that says no must not be overruled by the
        // model having asked nicely.
        let subagent_hooks = crate::production_hooks::build_production_hooks_for_project(
            project_path.as_deref().map(std::path::Path::new),
        )
        .with_events(self.events.clone());
        let start_responses = subagent_hooks
            .dispatch(HookRequest {
                event: HookEvent::SubagentStart,
                run_id: self.parent_run_id.clone(),
                tool_name: Some("task".into()),
                input: serde_json::json!({
                    "prompt": prompt.clone(),
                    "name": name.clone(),
                    "agent_profile_id": child_profile_id.clone(),
                    "permission_profile": child_perm.clone(),
                    "tool_allowlist": child_allowlist.clone(),
                    "model_id": child_model.clone(),
                }),
            })
            .await;
        if let Err(reason) = HookRegistry::aggregate_allow(&start_responses) {
            // E04: the hidden session was created before the hook ran — close
            // it so a denied spawn leaves no persistent session orphan.
            let _ =
                crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&reason));
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": format!("subagent hook denied: {reason}"),
                    "code": "subagent_denied_by_hook",
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        // T05 saga phase 1 — durable reservation BEFORE the child run exists.
        // If anything after this fails, the compensation releases the slot and
        // closes the session in reverse order, idempotently. Fixture mode has
        // no durable store and skips the reservation (slot lives in-memory).
        let depth = self.subagents.depth_for_child(&self.parent_run_id).await;
        if !fixture_mode {
            let scope_snapshot = serde_json::json!({
                "project_path": self.gateway.project_root.clone(),
                "project_id": project_id_for_scope.clone(),
                "project_identity_version": identity.as_ref().map(|i| i.identity_version as i64),
                "permission_profile": child_perm.clone(),
                "agent_profile_id": child_profile_id.clone(),
                "max_steps": child_max_steps,
                "tool_allowlist": child_allowlist.clone(),
                "failure_policy": failure_policy.as_str(),
                "max_tokens": child_max_tokens,
            });
            if let Err(error) = crate::subagent_store::reserve_subagent_slot(
                &crate::subagent_store::SubagentReservation {
                    session_id: session_id.clone(),
                    parent_run_id: self.parent_run_id.clone(),
                    tree_root_run_id: self.parent_run_id.clone(),
                    depth,
                    max_tokens: Some(child_max_tokens),
                    max_cost_usd: child_max_cost,
                    failure_policy: failure_policy.as_str().to_string(),
                    max_retries,
                    scope_snapshot,
                },
            ) {
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some(&error),
                );
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": format!("subagent reservation failed: {error}"),
                        "code": "SUBAGENT_RESERVE_FAILED",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        let created = match crate::global_run_manager().create_run(
            assistant_protocol::v2::CreateRunRequest {
                // Child runs never inherit the parent conversation's selection;
                // member skills come from the member profile at child resolve.
                capability_selection: None,
                disabled_tools: None,
                conversation_id: child_conversation_id.clone(),
                provider_id: child_provider.clone(),
                model_id: child_model.clone(),
                key_id: Some(child_key.clone()),
                // Persisted on the run row; `ProductionRuntime::start_run` reloads
                // the profile from it (system prompt, tools, token budget).
                agent_profile_id: child_profile_id.clone(),
                permission_profile: Some(child_perm.clone()),
                content: Some(prompt.clone()),
                attachments: None,
                max_steps: Some(child_max_steps),
                parent_run_id: Some(self.parent_run_id.clone()),
                project_path: project_path.clone(),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            },
        ) {
            Ok(r) => r,
            Err(e) => {
                // Saga compensation: release the reserved slot, close session.
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&e));
                let _ =
                    crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
                return ToolExecutionResult {
                    output: serde_json::json!({"error": format!("create child run failed: {e}")}),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        let child_run_id = created.id.clone();

        // Metadata shares real run_id + persistent session id as task_id.
        // Depth from parent chain — never hardcode 1 (task-11).
        let depth = self.subagents.depth_for_child(&self.parent_run_id).await;
        let child = match self
            .subagents
            .register(
                session_id.clone(),
                child_run_id.clone(),
                &self.parent_run_id,
                prompt.clone(),
                depth,
                child_provider.clone(),
                child_key.clone(),
                child_model.clone(),
                child_perm.clone(),
                child_allowlist.clone(),
                child_profile_id.clone(),
                Some("none".into()),
                project_path.clone(),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                // Saga compensation (reverse order, idempotent): release the
                // durable slot, fail the queued child run, close the session.
                let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&e));
                let _ =
                    crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
                // E04: the child run was already created (queued) before the
                // reservation — settle it so a failed registration leaves no
                // queued orphan run behind.
                crate::global_run_manager().fail_run_if_active(
                    &child_run_id,
                    format!("subagent reservation failed: {e}"),
                    "SUBAGENT_REGISTER_FAILED",
                );
                return ToolExecutionResult {
                    output: serde_json::json!({"error": e}),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        let _ = self
            .subagents
            .update_status(&child.id, SubAgentStatus::Running)
            .await;
        let _ = crate::subagent_store::update_subagent_session_status(&session_id, "running", None);

        // Apply child tool surface + parent-authored system prompt before
        // RunManager starts the engine. Both are keyed by the child run id and
        // consumed once by `ProductionRuntime::start_run`.
        crate::global_run_manager()
            .runtime
            .set_run_tool_allowlist(&child_run_id, child_allowlist.clone())
            .await;
        if let Some(directive) = child_directive.clone() {
            crate::global_run_manager()
                .runtime
                .set_run_agent_directive(&child_run_id, directive)
                .await;
        }

        if let Err(error) = self.events.append_checked(
            &self.parent_run_id,
            RunEventKind::SubagentCreated {
                sub_run_id: child_run_id.clone(),
                agent_profile_id: child_profile_id.clone(),
                task: prompt.clone(),
            },
        ) {
            // Saga compensation: cancel the child, release both slot ledgers
            // (in-memory via a terminal status, durable via release), close.
            let _ = crate::global_run_manager()
                .cancel(assistant_protocol::v2::CancelRunRequest {
                    run_id: child_run_id.clone(),
                })
                .await;
            let _ = self
                .subagents
                .update_status(&session_id, SubAgentStatus::Failed(error.clone()))
                .await;
            let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&error));
            let _ =
                crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&error));
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": format!("subagent creation event could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        let task_id = session_id.clone();
        self.task_outputs.lock().await.insert(
            task_id.clone(),
            TaskRecord {
                run_id: child_run_id.clone(),
                status: "running".into(),
                output: None,
            },
        );

        let start_result = crate::run_manager::RunManager::start_detached_global(
            assistant_protocol::v2::StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(child_run_id.clone()),
                conversation_id: Some(child_conversation_id.clone()),
                provider_id: Some(child_provider.clone()),
                model_id: Some(child_model.clone()),
                key_id: Some(child_key.clone()),
                content: Some(prompt.clone()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some(child_perm.clone()),
                max_steps: Some(child_max_steps),
                project_path: project_path.clone(),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            },
        );
        if let Err(e) = start_result {
            // The child never started, so nothing will consume its directive.
            let _ = crate::global_run_manager()
                .runtime
                .take_run_agent_directive(&child_run_id)
                .await;
            // Saga compensation (T05): a failed start must not leave a
            // registered child holding its in-memory slot nor a durable
            // reservation with no live run behind it.
            let _ = self
                .subagents
                .update_status(&session_id, SubAgentStatus::Failed(e.clone()))
                .await;
            crate::global_run_manager().fail_run_if_active(
                &child_run_id,
                format!("start child run failed: {e}"),
                "SUBAGENT_START_FAILED",
            );
            let _ = crate::subagent_store::release_subagent_slot(&session_id, Some(&e));
            let _ = crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&e));
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&task_id) {
                rec.status = "failed".into();
                rec.output = Some(e.clone());
            }
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("start child run failed: {e}")}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Background watcher: when RunManager marks the run terminal, update
        // session/task, settle usage against the durable budget, apply the
        // failure policy, and release the slot exactly once.
        spawn_subagent_watcher(
            self.events.clone(),
            self.subagents.clone(),
            self.task_outputs.clone(),
            self.parent_run_id.clone(),
            session_id.clone(),
            session_id.clone(),
            child_conversation_id.clone(),
            child.id.clone(),
            child_run_id.clone(),
            project_path.clone(),
            self.subagents.config().child_timeout_ms.max(1),
            self.parent_run_id.clone(),
            self.subagents.config().max_tokens_per_tree,
            child_max_tokens,
            binding,
            child_perm.clone(),
            child_allowlist.clone(),
            child_profile_id.clone(),
            child_directive.clone(),
            child_max_steps,
            failure_policy,
            max_retries,
        );

        ToolExecutionResult {
            output: serde_json::json!({
                "task_id": task_id,
                "run_id": child_run_id,
                "conversation_id": child_conversation_id,
                "session_id": session_id,
                "status": "running",
                "provider_id": child_provider,
                "key_id": child_key,
                "model_id": child_model,
                "permission_profile": child_perm,
                "agent_profile_id": child_profile_id,
                "tool_allowlist": child_allowlist,
                "max_steps": child_max_steps,
                "system_prompt_authored": child_directive.is_some(),
            }),
            is_error: false,
            duration_ms: 0,
        }
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
    async fn resolve_task_binding(
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

/// Launch the background watcher for one child run (T05).
///
/// It polls the child to a terminal status, settles provider usage against the
/// *durable* budget incrementally (cancelling the child the moment the budget
/// is exceeded), consumes the persisted failure policy, releases the slot
/// exactly once, and re-enters itself when `Retry` re-queues the child.
#[allow(clippy::too_many_arguments)] // watcher re-entry needs the full child scope
fn spawn_subagent_watcher(
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
fn spawn_retry_watcher(
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
                    cursor = cursor.max(e.effective_run_sequence());
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

/// Sum of input+output tokens in a batch of replayed events (provider deltas).
fn usage_delta_from_events(events: &[assistant_protocol::v2::RunEventV2]) -> u64 {
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
async fn child_completed_terminal(
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
async fn child_failed_terminal(
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

/// Create a fresh child run for a Retry re-queue and bump the retry counter.
#[allow(clippy::too_many_arguments)] // re-queue needs the exact child scope
async fn requeue_child_run(
    session_id: &str,
    child_conversation_id: &str,
    parent_run_id: &str,
    binding: &crate::subagent_store::RouteBinding,
    retry_number: &u32,
    reason: &str,
    child_perm: &str,
    child_allowlist: &[String],
    child_profile_id: &Option<String>,
    child_directive: &Option<String>,
    child_max_steps: u32,
) -> Result<String, String> {
    let session = crate::subagent_store::get_subagent_session(session_id)?
        .ok_or_else(|| format!("subagent session not found: {session_id}"))?;
    let project_path = session.project_path.clone();
    let prompt = if session.task.trim().is_empty() {
        format!("Retry (attempt {retry_number}) after: {reason}")
    } else {
        session.task.to_string()
    };
    let created =
        crate::global_run_manager().create_run(assistant_protocol::v2::CreateRunRequest {
            capability_selection: None,
            disabled_tools: None,
            conversation_id: child_conversation_id.to_string(),
            provider_id: binding.provider_id.clone(),
            model_id: binding.model_id.clone(),
            key_id: Some(binding.key_id.clone()),
            agent_profile_id: child_profile_id.clone(),
            permission_profile: Some(child_perm.to_string()),
            content: Some(prompt.clone()),
            attachments: None,
            max_steps: Some(child_max_steps),
            parent_run_id: Some(parent_run_id.to_string()),
            project_path,
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".into()),
        })?;
    crate::global_run_manager()
        .runtime
        .set_run_tool_allowlist(&created.id, child_allowlist.to_vec())
        .await;
    if let Some(directive) = child_directive.as_deref().filter(|s| !s.is_empty()) {
        crate::global_run_manager()
            .runtime
            .set_run_agent_directive(&created.id, directive.to_string())
            .await;
    }
    crate::run_manager::RunManager::start_detached_global(
        assistant_protocol::v2::StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
            run_id: Some(created.id.clone()),
            conversation_id: Some(child_conversation_id.to_string()),
            provider_id: Some(binding.provider_id.clone()),
            model_id: Some(binding.model_id.clone()),
            key_id: Some(binding.key_id.clone()),
            content: Some(prompt),
            attachments: None,
            trigger_message_id: None,
            permission_profile: Some(child_perm.to_string()),
            max_steps: Some(child_max_steps),
            project_path: session.project_path.clone(),
            idempotency_key: None,
            effort: None,
            runtime_id: Some("native".into()),
        },
    )?;
    let _ = crate::subagent_store::bump_subagent_retry(session_id);
    Ok(created.id)
}

/// (all_other_siblings_terminal, any_other_sibling_failed) for the parent's
/// task ledger. Used by RequireAll to detect the aggregate outcome.
async fn sibling_settled_state(
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    exclude_task_id: &str,
) -> (bool, bool) {
    let map = task_outputs.lock().await;
    let mut any_running = false;
    let mut any_failed = false;
    for (tid, rec) in map.iter() {
        if tid == exclude_task_id {
            continue;
        }
        if rec.status == "running" {
            any_running = true;
        }
        if rec.status == "failed" {
            any_failed = true;
        }
    }
    (!any_running, any_failed)
}

/// FailFast / RequireAll aggregate action: cancel every sibling child's engine
/// and metadata, then fail the parent run (idempotent).
pub(crate) async fn fail_parent_and_cancel_siblings(
    subagents: &Arc<SubAgentManager>,
    parent_run_id: &str,
    reason: &str,
) {
    for sibling in subagents.get_children(parent_run_id).await {
        let _ = subagents
            .update_status(&sibling.id, SubAgentStatus::Cancelled)
            .await;
        crate::global_run_manager()
            .runtime
            .cancel_run(&sibling.run_id)
            .await;
    }
    let _ = crate::global_run_manager()
        .runtime
        .cancel_run(parent_run_id)
        .await;
    crate::global_run_manager().fail_run_if_active(
        parent_run_id,
        reason.to_string(),
        "SUBAGENT_FAILFAST",
    );
}

/// SubagentStop hook fired for every terminal outcome (matches prior behavior).
async fn fire_subagent_stop(
    project_path: &Option<String>,
    parent_run_id: &str,
    child_run_id: &str,
    status: &str,
    output: &str,
) {
    let _ = crate::production_hooks::build_production_hooks_for_project(
        project_path.as_deref().map(std::path::Path::new),
    )
    .dispatch(HookRequest {
        event: HookEvent::SubagentStop,
        run_id: parent_run_id.to_string(),
        tool_name: Some("task".into()),
        input: serde_json::json!({
            "sub_run_id": child_run_id,
            "status": status,
            "output": output,
        }),
    })
    .await;
}

fn use_fixture_flag(input: &Value) -> bool {
    input
        .get("fixture")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::SubAgentConfig;

    /// T05 FailFast action: one child failure cancels every sibling and fails
    /// the parent run — the parent-side outcome the watcher applies.
    #[tokio::test]
    async fn fail_fast_action_cancels_siblings_and_fails_parent() {
        let _env = crate::storage::DataStore::env_test_lock();
        // Restore env on drop so `NATIVES_RUN_MANAGER_MEMORY` cannot leak into
        // later tests in the same process.
        let _env_restore = crate::storage::EnvRestore::capture();
        // Memory-only global RunManager (hermetic; the durable budget side is
        // covered by the subagent_store restart/recovery tests).
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        let rm = crate::run_manager::RunManager::new();
        let make_run = |conversation_id: &str, parent: Option<&str>| {
            rm.create_run(assistant_protocol::v2::CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: conversation_id.to_string(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("x".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: parent.map(|p| p.to_string()),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap()
        };
        let probe_parent = make_run("c-ff", None);
        let parent = make_run("c-ff", None);
        let child_a = make_run("c-ff", Some(&parent.id));
        let child_b = make_run("c-ff", Some(&parent.id));

        let subagents = Arc::new(SubAgentManager::new(SubAgentConfig::default()));
        subagents.register_root_depth(&parent.id).await;
        let a = subagents
            .register(
                "session-a".into(),
                child_a.id.clone(),
                &parent.id,
                "a".into(),
                1,
                "openai".into(),
                "k".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let b = subagents
            .register(
                "session-b".into(),
                child_b.id.clone(),
                &parent.id,
                "b".into(),
                1,
                "openai".into(),
                "k".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();

        crate::run_manager::install_global_for_test(rm);

        // Probe: a queued run must be able to fail (FailFast depends on it).
        let commit_result = crate::global_run_manager().commit_status(
            &probe_parent.id,
            assistant_protocol::v2::RunStatusV2::Failed,
            agent_core::TransitionMetadata::empty()
                .with_error_code("PROBE")
                .with_lifecycle_hint("failed"),
        );
        match &commit_result {
            Ok(run) => assert_eq!(
                run.status.as_str(),
                "failed",
                "commit_status Ok must report the failed run: {run:?}"
            ),
            Err(e) => panic!("queued → failed must be legal for FailFast: {e}"),
        }

        crate::tools::subagent::fail_parent_and_cancel_siblings(
            &subagents,
            &parent.id,
            "injected child failure",
        )
        .await;

        assert_eq!(
            subagents.get(&a.id).await.unwrap().status,
            SubAgentStatus::Cancelled,
            "FailFast must cancel sibling A"
        );
        assert_eq!(
            subagents.get(&b.id).await.unwrap().status,
            SubAgentStatus::Cancelled,
            "FailFast must cancel sibling B"
        );
        let parent_run = crate::global_run_manager().get_run(&parent.id).unwrap();
        assert_eq!(
            parent_run.status.as_str(),
            "failed",
            "FailFast must fail the parent run"
        );
        crate::run_manager::install_memory_global_for_test();
    }
}
