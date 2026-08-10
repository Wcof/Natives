//! `task` tool execution (split from `subagent.rs` by responsibility, ARCH-002):
//! resolves permission, persona/directive, allowlist, step/token budget, failure
//! policy and route binding, then spawns the child run under RunManager and
//! hands the child over to the background watcher (`subagent_watcher`).

use agent_core::{
    default_subagent_tool_allowlist, FailurePolicy, HookEvent, HookRegistry, HookRequest,
    SubAgentStatus, ToolExecutionResult,
};
use assistant_protocol::v2::RunEventKind;
use serde_json::Value;

use crate::production::TaskRecord;

use super::gated::PermissionGatedTools;
use super::subagent::{
    DEFAULT_CHILD_MAX_STEPS, MAX_CHILD_MAX_STEPS, MAX_CHILD_SYSTEM_PROMPT_BYTES,
    MAX_SUBAGENT_RETRIES,
};
use super::subagent_requeue::use_fixture_flag;
use super::subagent_watcher::spawn_subagent_watcher;

impl PermissionGatedTools {
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
            // 19.3-②: the SAME authoritative loader as DB Experts and file
            // profiles — so a `subagent_type` naming a DB Expert works exactly
            // like one naming a `.claude/agents/*.md` file (DB wins, file
            // fallback, fail-closed on neither).
            Some(id) => match crate::capability::experts::load_agent_profile(id, project_root_path)
            {
                Some(p) => Some(p),
                None => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!("agent profile `{id}` not found in the capability experts or project/user profile directories"),
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

        // NE-P0-08 / 19.3-①: a stable SHA-256 digest of the parent-authored
        // persona. It is persisted next to the directive in the protected
        // pending execution plan so restart/retry/continue restore the exact
        // same text and digest — never a re-rolled or truncated persona.
        let child_directive_digest = child_directive
            .as_ref()
            .map(|sp| crate::subagent_store::directive_sha256_hex(sp));

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

        // ── 19.3-④ team maxConcurrent contract ──
        //
        // When a leader delegates to a roster member under an active team, the
        // parent run may not exceed the team's configured concurrent-child
        // ceiling. The gate consults the durable reservation ledger BEFORE any
        // child is spawned (persist-first), so a busy team rejects the extra
        // delegation fail-closed instead of silently oversubscribing.
        if member_profile.is_some() {
            if let Some(team) = &self.team {
                // Fail closed on ledger errors too — a team whose concurrency
                // ledger cannot be read must not start unbounded delegations.
                let active =
                    match crate::subagent_store::parent_active_reservations(&self.parent_run_id) {
                        Ok(n) => n,
                        Err(e) => {
                            return ToolExecutionResult {
                                output: serde_json::json!({
                                    "error": format!("team concurrency gate failed: {e}"),
                                    "code": "TEAM_CONCURRENCY_GATE_FAILED",
                                }),
                                is_error: true,
                                duration_ms: 0,
                            };
                        }
                    };
                if active >= team.max_concurrent as usize {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!(
                                "team '{}' max_concurrent reached ({}/{}): wait for a member to finish or raise the team limit",
                                team.team_id, active, team.max_concurrent
                            ),
                            "code": "TEAM_MAX_CONCURRENT_REACHED",
                            "max_concurrent": team.max_concurrent,
                            "active": active,
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            }
        }

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
        // 19.3-④ team failurePolicy contract: without an explicit per-call
        // policy, a team delegation uses the team's configured failure policy
        // (same vocabulary the daemon enforces per child). A member spawned
        // without a team still falls back to the daemon default.
        let failure_policy = match input.get("failure_policy").and_then(|v| v.as_str()) {
            Some(raw) => FailurePolicy::parse(raw),
            None => match (&self.team, member_profile.as_ref()) {
                (Some(team), Some(_)) => FailurePolicy::parse(&team.failure_policy),
                _ => self.subagents.config().failure_policy,
            },
        };
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
                    // 19.3-⑤: field-level visibility — hooks see the persona's
                    // digest, never its text. The text lives only in the
                    // protected pending execution plan.
                    "system_prompt_digest": child_directive_digest.clone(),
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
            // NE-P0-08 / 19.3-①: the parent-authored directive is embedded in
            // the reservation scope snapshot BEFORE the child run exists — the
            // protected pending execution plan. A daemon crash between here and
            // child start no longer loses the persona; retry/restart/continue
            // read the exact same text + digest back.
            let scope_snapshot = crate::subagent_store::with_pending_directive(
                serde_json::json!({
                    "project_path": self.gateway.project_root.clone(),
                    "project_id": project_id_for_scope.clone(),
                    "project_identity_version": identity.as_ref().map(|i| i.identity_version as i64),
                    "permission_profile": child_perm.clone(),
                    "agent_profile_id": child_profile_id.clone(),
                    "max_steps": child_max_steps,
                    "tool_allowlist": child_allowlist.clone(),
                    "failure_policy": failure_policy.as_str(),
                    "max_tokens": child_max_tokens,
                }),
                child_directive
                    .as_deref()
                    .zip(child_directive_digest.as_deref()),
            );
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

        let created = match crate::child_run_orchestrator::create_child_run(
            crate::child_run_orchestrator::ChildRunSpec {
                // Child runs never inherit the parent conversation's selection;
                // member skills come from the member profile at child resolve.
                conversation_id: child_conversation_id.clone(),
                provider_id: child_provider.clone(),
                model_id: child_model.clone(),
                key_id: Some(child_key.clone()),
                // Persisted on the run row; `ProductionRuntime::start_run` reloads
                // the profile from it (system prompt, tools, token budget).
                agent_profile_id: child_profile_id.clone(),
                permission_profile: Some(child_perm.clone()),
                content: Some(prompt.clone()),
                max_steps: Some(child_max_steps),
                parent_run_id: Some(self.parent_run_id.clone()),
                project_path: project_path.clone(),
                runtime_id: Some("native".into()),
            },
        )
        .await
        {
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
        crate::child_run_orchestrator::apply_child_surface(
            &child_run_id,
            child_allowlist.clone(),
            child_directive.clone(),
        )
        .await;

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

        let start_result = crate::child_run_orchestrator::start_child_run(
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
        )
        .await;
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
                // 19.3-⑤: field-level visibility — the persona's digest, never
                // its text, is echoed back to the parent conversation.
                "system_prompt_digest": child_directive_digest,
            }),
            is_error: false,
            duration_ms: 0,
        }
    }
}
