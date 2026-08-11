//! Tool execution + permission gating for `PermissionGatedTools`.
//!
//! The `execute_*` surface is the `EngineToolRuntime` execution entry points;
//! `execute_tool_with_call_id_and_progress` is the single gated dispatch path
//! (allowlist, plan mode, project identity, subagent budget, schema validation,
//! permission gate, checkpoint/ledger/lease, Gateway handler). Split out of
//! `gated.rs` (ARCH-002).

use super::*;

#[async_trait::async_trait]
impl EngineToolRuntime for PermissionGatedTools {
    async fn execute_tool_with_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        progress: &dyn ToolProgressSink,
    ) -> ToolExecutionResult {
        progress
            .publish(ToolProgressUpdate {
                run_id: self.parent_run_id.clone(),
                tool_call_id: name.to_string(),
                tool_name: name.to_string(),
                stream: "status".into(),
                text: "started".into(),
                final_update: false,
                turn_id: None,
                message_id: None,
                progress_sequence: 0,
            })
            .await;
        let result = self
            .execute_tool_with_call_id(name, input, cancel, None)
            .await;
        progress
            .publish(ToolProgressUpdate {
                run_id: self.parent_run_id.clone(),
                tool_call_id: name.to_string(),
                tool_name: name.to_string(),
                stream: "status".into(),
                text: if result.is_error {
                    "failed"
                } else {
                    "completed"
                }
                .into(),
                final_update: true,
                turn_id: None,
                message_id: None,
                progress_sequence: 0,
            })
            .await;
        result
    }

    async fn execute_tool_with_progress_for_call(
        &self,
        call_id: &str,
        turn_id: Option<&str>,
        message_id: Option<&str>,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        progress
            .publish(ToolProgressUpdate {
                run_id: self.parent_run_id.clone(),
                tool_call_id: call_id.to_string(),
                tool_name: name.to_string(),
                stream: "status".into(),
                text: "started".into(),
                final_update: false,
                turn_id: turn_id.map(str::to_string),
                message_id: message_id.map(str::to_string),
                progress_sequence: 0,
            })
            .await;
        let result = self
            .execute_tool_with_call_id_and_progress(
                name,
                input,
                cancel,
                Some(call_id),
                turn_id,
                message_id,
                progress.clone(),
            )
            .await;
        let child_run_id = result
            .output
            .get("run_id")
            .and_then(Value::as_str)
            .filter(|_| result.output.get("status").and_then(Value::as_str) == Some("running"))
            .map(str::to_string);
        if let Some(child_run_id) = child_run_id {
            crate::tools::subagent::spawn_subagent_progress(
                self.events.clone(),
                self.parent_run_id.clone(),
                call_id.to_string(),
                child_run_id,
                progress.clone(),
                turn_id.map(str::to_string),
                message_id.map(str::to_string),
            );
        } else {
            progress
                .publish(ToolProgressUpdate {
                    run_id: self.parent_run_id.clone(),
                    tool_call_id: call_id.to_string(),
                    tool_name: name.to_string(),
                    stream: "status".into(),
                    text: if result.is_error {
                        "failed"
                    } else {
                        "completed"
                    }
                    .into(),
                    final_update: true,
                    turn_id: turn_id.map(str::to_string),
                    message_id: message_id.map(str::to_string),
                    progress_sequence: 0,
                })
                .await;
        }
        result
    }

    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        self.execute_tool_with_call_id(name, input, cancel, None)
            .await
    }

    async fn execute_tool_with_call_id(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        call_id: Option<&str>,
    ) -> ToolExecutionResult {
        self.execute_tool_with_call_id_and_progress(
            name,
            input,
            cancel,
            call_id,
            None,
            None,
            Arc::new(NoopToolProgressSink),
        )
        .await
    }

    async fn execute_tool_with_call_id_and_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        call_id: Option<&str>,
        turn_id: Option<&str>,
        message_id: Option<&str>,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        if cancel.is_cancelled() {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "cancelled"}),
                is_error: true,
                duration_ms: 0,
            };
        }
        let stream_tool_call_id = call_id
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Hard allowlist gate before permission / orchestration (Phase 0).
        if !self.tool_allowed(name) {
            return Self::deny_not_allowlisted(name);
        }
        // Generic mcp_call carries the server in its input — the ADR-0016
        // server whitelist must gate it too, not just namespaced names.
        if name == "mcp_call" {
            if let Some(selected) = &self.selected_mcp_servers {
                let server = input
                    .get("server")
                    .or_else(|| input.get("server_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !selected.contains(server) {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!(
                                "mcp server '{server}' is not part of this run's capability selection"
                            ),
                            "denied": true,
                            "code": "MCP_SERVER_NOT_SELECTED",
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            }
        }

        // Plan Mode latch, ahead of everything else.
        //
        // Ahead of the identity check because the exit path must stay reachable
        // even when the project binding is unhappy — a run that cannot submit a
        // plan and cannot write is a run with no way out. Ahead of the subagent
        // budget because a refused call is not work and must not cost a slot.
        self.ensure_plan_latch();
        let planning = plan_mode::is_active(&self.parent_run_id);
        if name == plan_mode::EXIT_PLAN_MODE_TOOL {
            return self.handle_exit_plan_mode(input, planning).await;
        }
        if name == plan_mode::ENTER_PLAN_MODE_TOOL {
            return self.handle_enter_plan_mode(input).await;
        }
        if planning && self.plan_decision_for(name) == PlanDecision::Deny {
            // Deliberately not a permission prompt. Plan Mode exists so the user
            // reviews one plan instead of a stream of approval cards; turning a
            // blocked write into a card would rebuild the thing it replaces.
            let err = plan_mode::blocked_error(name);
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": err.message,
                    "code": err.code,
                    "denied": true,
                    "plan_mode": true,
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        // ProjectIdentity fail-closed for mutating/process/network/MCP tools.
        if let Err(err) = self.ensure_verified_project_for_tool(name).await {
            return ToolExecutionResult {
                output: serde_json::json!({"error": err, "denied": true, "code": "PROJECT_IDENTITY_REQUIRED"}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Subagent tool-call budget (production hook).
        if let Some((child_id, tree_root)) = self.subagent_budget_ids().await {
            if let Err(e) = self
                .subagents
                .consume_tool_call(&child_id, &tree_root)
                .await
            {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": e,
                        "denied": true,
                        "code": "subagent_tool_budget_exhausted",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        let tool = self.gateway.get_tool(name);
        let is_mcp = name == "mcp_call" || name.starts_with("mcp__");
        if tool.is_none()
            && !matches!(name, "task" | "task_output" | "kill_task" | "mcp_call")
            && !name.starts_with("mcp__")
        {
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("unknown tool: {name}")}),
                is_error: true,
                duration_ms: 0,
            };
        }
        if name.starts_with("mcp__")
            && !self
                .mcp_tool_schemas
                .iter()
                .any(|schema| schema.name == name)
        {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "UNKNOWN_TOOL",
                    "error": format!("unknown advertised MCP tool: {name}"),
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        if let Some(tool) = tool {
            if let Err(error) = CapabilityGateway::validate_external_input(&tool.schema, &input) {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "SCHEMA_INVALID",
                        "error": error.message,
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        // Dynamic MCP tools are not local Gateway handlers, but their
        // advertised schemas are still an execution trust boundary. Validate
        // the actual tool arguments before permission or transport dispatch;
        // generic `mcp_call` unwraps its nested `arguments` object first.
        if is_mcp {
            let schema_name = if name == "mcp_call" {
                let server = input
                    .get("server")
                    .or_else(|| input.get("server_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let tool_name = input
                    .get("tool")
                    .or_else(|| input.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                (!server.is_empty() && !tool_name.is_empty())
                    .then(|| format!("mcp__{server}__{tool_name}"))
            } else {
                Some(name.to_string())
            };
            if let Some(schema_name) = schema_name {
                if let Some(schema) = self
                    .mcp_tool_schemas
                    .iter()
                    .find(|schema| schema.name == schema_name)
                {
                    let schema_input = if name == "mcp_call" {
                        input
                            .get("arguments")
                            .or_else(|| input.get("input"))
                            .unwrap_or(&Value::Null)
                    } else {
                        &input
                    };
                    if let Err(error) = CapabilityGateway::validate_external_input(
                        &schema.input_schema,
                        schema_input,
                    ) {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error_code": "SCHEMA_INVALID",
                                "error": error.message,
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                }
            }
        }

        // Permission gate: PermissionClass + SideEffect (G5).
        // Orchestration tools always go through this gate (never early-return around it).
        // Read the gear through the Plan Mode latch, never the raw declared
        // profile: while planning it reads `plan`, and after approval it reads
        // the profile captured at entry (which can only be what the run already
        // had).
        let effective_profile = self.effective_permission_profile();
        let profile_str = match effective_profile.as_str() {
            "full_access" | "autonomous" | "full" => "autonomous",
            "readonly" => "readonly",
            p if p == plan_mode::PLAN_PROFILE => plan_mode::PLAN_PROFILE,
            _ => "ask",
        };
        let (perm_class, side_effect) = if let Some(tool) = tool {
            (tool.permission_class, tool.side_effect)
        } else if is_mcp {
            (
                capability_gateway::PermissionClass::ExternalWrite,
                SideEffect::Network,
            )
        } else {
            // Fail-closed defaults if somehow unregistered.
            match name {
                "task_output" => (
                    capability_gateway::PermissionClass::AlwaysAllowed,
                    SideEffect::ReadOnly,
                ),
                "task" | "kill_task" => (
                    capability_gateway::PermissionClass::ProjectWrite,
                    SideEffect::Process,
                ),
                _ => (
                    capability_gateway::PermissionClass::Elevation,
                    SideEffect::Destructive,
                ),
            }
        };
        let class_result = capability_gateway::policy::check_permission(perm_class, profile_str);
        let needs_ask = match class_result {
            capability_gateway::policy::PolicyResult::Allowed => false,
            capability_gateway::policy::PolicyResult::NeedsApproval(_)
            | capability_gateway::policy::PolicyResult::Denied(_) => true,
        };

        if profile_str == "readonly" && !matches!(side_effect, SideEffect::ReadOnly) {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "readonly profile denies side effects", "denied": true}),
                is_error: true,
                duration_ms: 0,
            };
        }

        if needs_ask {
            // The ceiling a PermissionRequest hook may not raise. A hook's
            // `permissionDecision: "allow"` skips a *confirmation*; it can never
            // buy a capability the profile itself withholds. So auto-approval is
            // offered only where the policy genuinely says "ask a human":
            //   - `Denied(_)` is a policy refusal, not a question;
            //   - `readonly` reaches here only for read-only side effects, and
            //     `request_permission_for_profile` refuses that profile anyway.
            let auto_approve_allowed = matches!(
                class_result,
                capability_gateway::policy::PolicyResult::NeedsApproval(_)
            ) && profile_str != "readonly";
            if let Some(denied) = self
                .await_tool_permission(&stream_tool_call_id, name, &input, auto_approve_allowed)
                .await
            {
                return denied;
            }
        }

        // This is the first event that authorizes handler execution. It is
        // emitted only after allowlist, project identity and permission gates;
        // rejected calls never receive a started fact.
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::ToolCallStarted {
                    id: stream_tool_call_id.clone(),
                    name: name.to_string(),
                },
            )
            .is_err()
        {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": "tool start event could not be persisted"
                }),
                is_error: true,
                duration_ms: 0,
            };
        }
        if name == "skill" {
            let skill_name = input.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(project_root) = self.gateway.project_root.as_deref() else {
                return ToolExecutionResult {
                    output: serde_json::json!({"error": "project root required to load skill"}),
                    is_error: true,
                    duration_ms: 0,
                };
            };
            return match crate::skill_store::load_skill_for_project(
                std::path::Path::new(project_root),
                skill_name,
            ) {
                Ok(output) => ToolExecutionResult {
                    output,
                    is_error: false,
                    duration_ms: 0,
                },
                Err(error) => ToolExecutionResult {
                    output: serde_json::json!({"error": error}),
                    is_error: true,
                    duration_ms: 0,
                },
            };
        }

        // Orchestration tools after permission.
        if name == "task" {
            return self.execute_task(input).await;
        }
        if name == "task_output" {
            let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let rec = self.task_outputs.lock().await.get(id).cloned();
            return ToolExecutionResult {
                output: serde_json::to_value(rec)
                    .unwrap_or(serde_json::json!({"status":"unknown"})),
                is_error: false,
                duration_ms: 0,
            };
        }
        if name == "kill_task" {
            let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let ok = self.kill_task_tree(id).await;
            return ToolExecutionResult {
                output: serde_json::json!({"cancelled": ok, "task_id": id}),
                is_error: !ok,
                duration_ms: 0,
            };
        }
        // MCP tools: always after permission gate (ExternalWrite / Network).
        if name == "mcp_call" || name.starts_with("mcp__") {
            return self
                .execute_mcp_call(
                    name,
                    input,
                    Some(&stream_tool_call_id),
                    turn_id,
                    message_id,
                    cancel,
                    progress.clone(),
                )
                .await;
        }

        let Some(_tool) = tool else {
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("unknown tool: {name}")}),
                is_error: true,
                duration_ms: 0,
            };
        };

        // Inject project cwd for terminal when missing (sandbox).
        let mut input = input;
        if name == "run_terminal" && input.get("cwd").and_then(|v| v.as_str()).is_none() {
            if let Some(root) = &self.gateway.project_root {
                if let Some(obj) = input.as_object_mut() {
                    obj.insert("cwd".into(), Value::String(root.clone()));
                }
            }
        }

        let started = Instant::now();
        // Create tool call context FIRST so Gateway path preflight can
        // authorize write paths before any checkpoint I/O (N01).
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(|| cancel.clone())
        } else {
            cancel.clone()
        };
        let live_settled = Arc::new(AtomicBool::new(false));
        let live_dropped_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let live_forwarder = if name == "run_terminal" {
            // H03: bounded live-output channel. Overflow drops at the producer
            // and is counted in `live_dropped_bytes`; a slow consumer can never
            // grow memory unboundedly.
            let (tx, mut rx) = tokio::sync::mpsc::channel::<capability_gateway::ToolProgressChunk>(
                capability_gateway::TERMINAL_PROGRESS_CAPACITY,
            );
            let run_id = self.parent_run_id.clone();
            let call_id = stream_tool_call_id.clone();
            let turn_id = turn_id.map(str::to_string);
            let message_id = message_id.map(str::to_string);
            let settled = live_settled.clone();
            // Terminal live output goes through the SAME batched progress sink
            // as every other tool (8KiB/250ms) — there is no second direct
            // EventLog path for run_terminal. The handler drains supervisor
            // chunks into this channel; the sink batches and late-drops after
            // the call settles, just like MCP/Subagent progress.
            let progress = progress.clone();
            let forwarder = tokio::spawn(async move {
                while let Some(chunk) = rx.recv().await {
                    if settled.load(AtomicOrdering::Acquire) {
                        break;
                    }
                    progress
                        .publish(agent_core::ToolProgressUpdate {
                            run_id: run_id.clone(),
                            tool_call_id: call_id.clone(),
                            tool_name: "run_terminal".into(),
                            stream: chunk.stream,
                            text: chunk.text,
                            final_update: false,
                            turn_id: turn_id.clone(),
                            message_id: message_id.clone(),
                            progress_sequence: 0,
                        })
                        .await;
                }
            });
            Some((tx, forwarder))
        } else {
            None
        };
        let progress_tx = live_forwarder.as_ref().map(|(tx, _)| tx.clone());
        let tool_context = self
            .build_tool_call_context(
                stream_tool_call_id.clone(),
                cancel.clone(),
                progress_tx,
                live_dropped_bytes.clone(),
                turn_id.map(str::to_string),
                message_id.map(str::to_string),
            )
            .await;

        // A4 — ReadOnly Fast Path.
        //
        // Classification source is the Gateway's SideEffect, never the legacy
        // name-default `external`. Genuinely read-only tools (read_file /
        // list_dir / grep / search / memory reads) skip checkpoint before/after,
        // the side-effect ledger, and the conflict lease entirely — they have no
        // durable side effect to record or resume. ToolCallStarted (above) and
        // ToolCallCompleted (emitted by the engine loop after this returns)
        // remain durable facts, so the read-only invocation is still
        // observable/replayable. Mutating/process/network/MCP tools keep the
        // full strict path below.
        if matches!(side_effect, SideEffect::ReadOnly) {
            let started = Instant::now();
            let result = match self
                .gateway
                .execute(name, input.clone(), &tool_context)
                .await
            {
                Ok(out) => {
                    let mut output = out.result;
                    attach_tool_output_artifact(
                        &self.parent_run_id,
                        &stream_tool_call_id,
                        &mut output,
                    );
                    let is_error = output.get("error_code").and_then(Value::as_str).is_some()
                        || output.get("error").is_some();
                    ToolExecutionResult {
                        output,
                        is_error,
                        duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
                    }
                }
                Err(error) => ToolExecutionResult {
                    output: serde_json::json!({ "error": error, "code": "readonly_tool_failed" }),
                    is_error: true,
                    duration_ms: started.elapsed().as_millis() as u64,
                },
            };
            return result;
        }

        // Phase 3: Gateway path preflight MUST precede any checkpoint I/O (N01).
        // A rejected path returns before the ledger and the handler, so it
        // causes zero I/O. Checkpoint only ever receives Gateway-authorized
        // canonical paths (`TrustedPath`); it no longer accepts raw strings.
        let trusted_paths = match self
            .gateway
            .preflight_write_paths(name, &input, &tool_context)
        {
            Ok(paths) => paths,
            Err(e) => {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": e.code,
                        "error": e.message,
                    }),
                    is_error: true,
                    duration_ms: 0,
                }
            }
        };
        for trusted in &trusted_paths {
            if let Err(error) = self
                .checkpoint_manager()
                .capture_before_async(&self.parent_run_id, trusted)
                .await
            {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "PERSISTENCE_FAILED",
                        "error": format!("checkpoint before-image could not be persisted: {error}"),
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        }

        if let Err(error) = crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            &stream_tool_call_id,
            name,
            crate::side_effect_ledger::category_for_tool(name),
            "started",
            false,
            turn_id,
            &input,
        ) {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": format!("tool side-effect ledger could not be started: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        // D02: hold the cross-run conflict lease for the duration of the
        // handler. The RAII guard releases it on every exit path, so a cancel
        // or failure never leaves the conflict key leased.
        let conflict_key = self.gateway.conflict_key_for(name);
        let _conflict_lease = match conflict_key {
            Some(key) => match crate::runtime::conflict_lease::global_conflict_leases()
                .acquire_guard(&key, &self.parent_run_id)
            {
                Ok(guard) => Some(guard),
                Err(error) => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "CONFLICT_LEASE_DENIED",
                            "error": error,
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            },
            None => None,
        };

        let result = match self
            .gateway
            .execute(name, input.clone(), &tool_context)
            .await
        {
            Ok(out) => {
                let mut output = out.result;
                attach_tool_output_artifact(&self.parent_run_id, &stream_tool_call_id, &mut output);
                let mut checkpoint_error = None;
                for trusted in &trusted_paths {
                    if let Err(error) = self
                        .checkpoint_manager()
                        .capture_after_async(&self.parent_run_id, trusted)
                        .await
                    {
                        checkpoint_error = Some(error);
                        break;
                    }
                }
                if let Some(error) = checkpoint_error.as_deref() {
                    // The handler already ran but the after-image could not be
                    // persisted, so the effect outcome is not fully known:
                    // surface a durable `uncertain` mark. If even that write
                    // fails, the intent stays `started`, which the resume gate
                    // treats as unresolved — never replay-safe (D04).
                    let _ = crate::side_effect_ledger::record_tool_effect_state(
                        &self.parent_run_id,
                        &stream_tool_call_id,
                        name,
                        crate::side_effect_ledger::category_for_tool(name),
                        "uncertain",
                        false,
                        turn_id,
                        &input,
                    );
                    output = serde_json::json!({
                        "error_code": "PERSISTENCE_FAILED",
                        "error": format!("checkpoint after-image could not be persisted: {error}"),
                    });
                }
                // On success the ledger intent stays `started`: the daemon
                // event log settles it to a terminal status in the same
                // transaction that appends the ToolCallCompleted fact (D04).
                // A crash between the handler returning and that append leaves
                // the effect unresolved, so resume blocks instead of re-running
                // it.
                if name == "run_terminal" && live_forwarder.is_none() {
                    // Terminal output deltas are live-lane only; the shared
                    // LiveBus lives on the runtime.
                    if let Some(runtime) = self.runtime.as_deref() {
                        let live = runtime.live_events();
                        crate::tools::progress::emit_terminal_output_deltas(
                            &live,
                            &self.parent_run_id,
                            &stream_tool_call_id,
                            &output,
                        );
                    }
                    // Background shell tasks: surface on Activity task list.
                    if output
                        .get("background")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        let task_id = output
                            .get("task_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&stream_tool_call_id)
                            .to_string();
                        let label = output
                            .get("display_command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("terminal")
                            .to_string();
                        self.events.append(
                            &self.parent_run_id,
                            RunEventKind::TaskStarted {
                                task_id: task_id.clone(),
                                run_id: self.parent_run_id.clone(),
                                label,
                            },
                        );
                        if let Some(rt) = &self.runtime {
                            rt.task_outputs.lock().await.insert(
                                task_id,
                                TaskRecord {
                                    run_id: self.parent_run_id.clone(),
                                    status: "running".into(),
                                    output: output
                                        .get("output")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                },
                            );
                        }
                    }
                }
                if checkpoint_error.is_none() {
                    for trusted in &trusted_paths {
                        let rel = trusted.project_relative.to_string_lossy().into_owned();
                        // Best-effort FileChanged with before/after from checkpoint live map.
                        if let Ok(preview) = self
                            .checkpoint_manager()
                            .checkpoint_for_run_public(&self.parent_run_id)
                        {
                            if let Some(snap) = preview.files.iter().find(|f| f.path == rel) {
                                self.events.append(
                                    &self.parent_run_id,
                                    RunEventKind::FileChanged {
                                        path: rel.clone(),
                                        change_type: if !snap.existed_before {
                                            "created".into()
                                        } else {
                                            "modified".into()
                                        },
                                        before: snap.before_content.clone(),
                                        after: snap.after_content.clone(),
                                        before_hash: snap.before_hash.clone(),
                                        after_hash: snap.after_hash.clone(),
                                        diff_artifact_id: None,
                                    },
                                );
                            }
                        }
                    }
                }
                if name == "notification" {
                    // NE-P0-05: Notification dispatches through the Run's frozen
                    // Hook Dispatcher (same plan hash as the engine's registry)
                    // rather than re-scanning hooks.json per tool call. Mid-Run
                    // edits only affect a new Run.
                    let hooks = crate::production_hooks::resolve_frozen_dispatcher(
                        &self.parent_run_id,
                        self.events.clone(),
                        self.gateway
                            .project_root
                            .as_deref()
                            .map(std::path::Path::new),
                    );
                    let _ = hooks
                        .dispatch(HookRequest {
                            event: HookEvent::Notification,
                            run_id: self.parent_run_id.clone(),
                            tool_name: Some(name.to_string()),
                            input: serde_json::json!({
                                "input": input,
                                "output": output.clone(),
                            }),
                        })
                        .await;
                }
                // T06: convert a successful `creative_proposal` tool result into
                // a durable typed proposal fact so the Host can pull it over UDS
                // and surface a pending approval inbox. The tool result itself
                // stays a normal ToolOutput — the fact is the durable side
                // record. A persistence failure must not fail the tool call
                // (the model still completed its work); it only loses the
                // bridge, which is logged for observability.
                if name == crate::proposal_fact::PROPOSAL_TOOL
                    && output.get("ok").and_then(Value::as_bool) == Some(true)
                {
                    if let Some(payload) =
                        crate::proposal_fact::proposal_from_tool_output(name, &output)
                    {
                        if let Some(store) =
                            crate::run_manager::global_run_manager().data_store_ref()
                        {
                            if let Err(e) = crate::proposal_fact::record_proposal_fact(
                                &store,
                                &self.parent_run_id,
                                turn_id,
                                &stream_tool_call_id,
                                &payload,
                            ) {
                                eprintln!(
                                    "[proposal_fact] failed to record creative proposal fact: {e}"
                                );
                            }
                        }
                    }
                }
                let is_error =
                    output.get("error_code").and_then(Value::as_str) == Some("PERSISTENCE_FAILED");
                ToolExecutionResult {
                    output,
                    is_error,
                    duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
                }
            }
            Err(err) => {
                let status = if err.code == "cancelled" {
                    "cancelled"
                } else if err.code == "timeout" {
                    "uncertain"
                } else {
                    "failed"
                };
                let ledger_result = crate::side_effect_ledger::record_tool_effect_state(
                    &self.parent_run_id,
                    &stream_tool_call_id,
                    name,
                    crate::side_effect_ledger::category_for_tool(name),
                    status,
                    false,
                    turn_id,
                    &input,
                );
                let output = match ledger_result {
                    Ok(()) => serde_json::json!({"error": err.message, "code": err.code}),
                    Err(ledger_error) => serde_json::json!({
                        "error_code": "PERSISTENCE_FAILED",
                        "error": format!("tool side-effect ledger could not be settled: {ledger_error}"),
                    }),
                };
                ToolExecutionResult {
                    output,
                    is_error: true,
                    duration_ms: started.elapsed().as_millis() as u64,
                }
            }
        };
        live_settled.store(true, AtomicOrdering::Release);
        drop(tool_context);
        if let Some((tx, forwarder)) = live_forwarder {
            drop(tx);
            forwarder.abort();
        }
        result
    }

    async fn mark_tool_call_uncertain(
        &self,
        call_id: &str,
        name: &str,
        turn_id: Option<&str>,
        input: &Value,
    ) -> Result<(), String> {
        crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            call_id,
            name,
            crate::side_effect_ledger::category_for_tool(name),
            "uncertain",
            false,
            turn_id,
            input,
        )
    }

    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        self.ensure_plan_latch();
        let planning = plan_mode::is_active(&self.parent_run_id);
        model_visible_tool_schemas(
            &self.gateway,
            self.tool_allowlist.as_deref(),
            &self.mcp_tool_schemas,
            self.selected_mcp_servers.as_ref(),
            planning,
        )
    }

    async fn list_tool_capabilities(&self) -> Vec<agent_core::ToolCapability> {
        self.ensure_plan_latch();
        let gateway_capabilities = self.gateway.list_capabilities();
        model_visible_tool_schemas(
            &self.gateway,
            self.tool_allowlist.as_deref(),
            &self.mcp_tool_schemas,
            self.selected_mcp_servers.as_ref(),
            plan_mode::is_active(&self.parent_run_id),
        )
        .into_iter()
        .map(|schema| {
            let gateway_capability = gateway_capabilities
                .iter()
                .find(|capability| capability.name == schema.name);
            let mode = match gateway_capability.map(|capability| capability.execution_mode) {
                Some(capability_gateway::ExecutionMode::ParallelSafe) => {
                    agent_core::ToolExecutionMode::ParallelSafe
                }
                Some(capability_gateway::ExecutionMode::Exclusive) => {
                    agent_core::ToolExecutionMode::Exclusive
                }
                Some(capability_gateway::ExecutionMode::Sequential) | None => {
                    agent_core::ToolExecutionMode::Sequential
                }
            };
            let side_effect = match gateway_capability.map(|capability| capability.side_effect) {
                Some(capability_gateway::SideEffect::ReadOnly) => {
                    agent_core::ToolSideEffect::ReadOnly
                }
                Some(capability_gateway::SideEffect::Write) => agent_core::ToolSideEffect::Write,
                Some(capability_gateway::SideEffect::Destructive) => {
                    agent_core::ToolSideEffect::Destructive
                }
                Some(capability_gateway::SideEffect::Network) => {
                    agent_core::ToolSideEffect::Network
                }
                Some(capability_gateway::SideEffect::Process) => {
                    agent_core::ToolSideEffect::Process
                }
                None => agent_core::ToolSideEffect::Destructive,
            };
            agent_core::ToolCapability {
                name: schema.name,
                schema: schema.input_schema,
                execution_mode: mode,
                side_effect,
                conflict_key: gateway_capability
                    .and_then(|capability| capability.conflict_key.clone()),
            }
        })
        .collect()
    }
}
