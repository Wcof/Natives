//! Run start, preparing, and execution seams.

use super::manager::{global_run_manager, McpRunRefGuard, RunManager};
use crate::production::{FixtureMode, FixtureProvider};
use agent_core::{AgentEngine, EngineOutcome, EngineRunConfig, TransitionMetadata};
use assistant_protocol::v2::{CreateRunRequest, RunStatusV2, RunV2, StartRunRequest};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl RunManager {
    /// Production start: real provider path when credentials exist; fixture path only under test flag.
    /// Blocks until the engine reaches a terminal status (tests / in-process callers that wait).
    /// RPC must use [`start_detached`] / [`start_detached_global`] instead.
    pub async fn start(&self, req: StartRunRequest) -> Result<RunV2, String> {
        let run = if let Some(run_id) = &req.run_id {
            self.get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?
        } else {
            self.ensure_run_for_start(&req)?
        };

        let content = req
            .content
            .clone()
            .or_else(|| {
                self.last_content
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&run.id).cloned())
            })
            .unwrap_or_else(|| "continue".into());
        self.last_content
            .lock()
            .map_err(|e| e.to_string())?
            .insert(run.id.clone(), content.clone());

        self.commit_status(
            &run.id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        )?;

        let request_project_path = req.project_path.clone();
        let provider_id = req.provider_id.unwrap_or_else(|| run.provider_id.clone());
        let model_id = req.model_id.unwrap_or_else(|| run.model_id.clone());
        let key_id = req.key_id.or_else(|| run.key_id.clone());
        let permission_profile = req
            .permission_profile
            .unwrap_or_else(|| run.permission_profile.clone());
        let max_steps = req.max_steps.unwrap_or(run.max_steps);
        let runtime_id = req
            .runtime_id
            .clone()
            .or_else(|| run.runtime_id.clone())
            .unwrap_or_else(|| "native".into());
        // Persist selected runtime on the run row for UI / resume.
        {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            if let Some(r) = runs.get_mut(&run.id) {
                r.runtime_id = Some(runtime_id.clone());
                r.effort = req.effort.clone().or_else(|| r.effort.clone());
                self.persist_run_row(r)?;
            }
        }
        // Capability resolution (ADR-0016): merge run-level selection with the
        // conversation default, validate every referenced capability and the
        // runtime support matrix. Fail-closed BEFORE any provider call — this
        // is the Harness control plane's frozen resolve_run insertion point.
        let capability_snapshot = match crate::capability_resolution::resolve(
            req.capability_selection.as_ref(),
            &run.conversation_id,
            req.agent_profile_id
                .as_deref()
                .or(run.agent_profile_id.as_deref()),
            req.project_path
                .as_deref()
                .or(run.project_path.as_deref())
                .map(std::path::Path::new),
            &runtime_id,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.fail_run_if_active(&run.id, error.reason.clone(), &error.code);
                return Err(format!("{}: {}", error.code, error.reason));
            }
        };
        // Persist the audit snapshot on the run row (ids only, never secrets).
        {
            let audit = capability_snapshot.to_audit_json();
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            if let Some(r) = runs.get_mut(&run.id) {
                if capability_snapshot.selection_active {
                    r.capability_snapshot = Some(audit.clone());
                }
                if let Some(profile_id) = capability_snapshot.agent_profile_id.clone() {
                    r.agent_profile_id = Some(profile_id);
                }
                self.persist_run_row(r)?;
            }
        }
        let frozen_tool_schemas = if runtime_id == "native" {
            let project_root = request_project_path
                .as_deref()
                .or(run.project_path.as_deref())
                .map(std::path::Path::new)
                .ok_or_else(|| {
                    "project_path is required for daemon runs; process cwd fallback is disabled"
                        .to_string()
                })?;
            let mut allowlist = self
                .runtime
                .peek_run_tool_allowlist(&run.id)
                .await
                .or_else(|| {
                    capability_snapshot
                        .agent_profile_id
                        .as_deref()
                        .or(run.agent_profile_id.as_deref())
                        .and_then(crate::production::builtin_surface_allowlist)
                })
                .or_else(|| {
                    capability_snapshot
                        .profile
                        .as_ref()
                        .and_then(|profile| profile.tools.clone())
                });
            if let (Some(list), Some(disallowed)) = (
                allowlist.as_mut(),
                capability_snapshot
                    .profile
                    .as_ref()
                    .and_then(|profile| profile.disallowed_tools.as_ref()),
            ) {
                list.retain(|tool| !disallowed.iter().any(|denied| denied == tool));
            }
            let mut gateway = capability_gateway::CapabilityGateway::new();
            gateway.set_project_root(project_root.to_string_lossy().to_string());
            crate::production::register_tools_for_surface(&mut gateway, allowlist.as_deref());
            if let Err(error) = gateway.validate_registered_schemas() {
                self.fail_run_if_active(
                    &run.id,
                    format!("tool schema validation failed: {}", error.message),
                    "TOOL_SCHEMA_INVALID",
                );
                return Err(format!("tool schema validation failed: {}", error.message));
            }
            let selected_mcp_servers = capability_snapshot
                .selection_active
                .then(|| capability_snapshot.mcp_servers.iter().cloned().collect());
            let mut schemas = crate::production_tools::model_visible_tool_schemas(
                &gateway,
                allowlist.as_deref(),
                &capability_snapshot.mcp_tool_schemas,
                selected_mcp_servers.as_ref(),
                permission_profile.eq_ignore_ascii_case("plan"),
            );
            // P0-11: Settings disabledTools 最终减法（subtract-only，只能收紧）。
            // Host 在 run.create payload 注册；start 时消费一次。
            if let Some(disabled) = self.runtime.take_run_disabled_tools(&run.id).await {
                schemas.retain(|tool| !disabled.iter().any(|denied| denied == &tool.name));
            }
            if let Err(error) = crate::production_tools::validate_tool_limit(schemas.len()) {
                self.fail_run_if_active(&run.id, error.clone(), "tool_plan_too_large");
                return Err(error);
            }
            schemas
        } else {
            Vec::new()
        };
        // Harness is prepared exactly once after capability validation. It is
        // not persisted yet: the immutable evidence must also contain the exact
        // Provider prompt compiled from these same live inputs.
        let mut harness_plan = match crate::rpc::harness::control_plane::prepare_run_with_tool_plan(
            &run.id,
            Some(&run.conversation_id),
            run.project_id.as_deref(),
            request_project_path
                .as_deref()
                .or(run.project_path.as_deref())
                .map(std::path::Path::new),
            &frozen_tool_schemas,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                self.fail_run_if_active(&run.id, error.to_string(), error.code);
                return Err(error.to_string());
            }
        };
        let effective_project_path = request_project_path
            .as_deref()
            .or(run.project_path.as_deref())
            .map(std::path::PathBuf::from);
        let effective_prompt = if runtime_id == "native" {
            let project_root = effective_project_path.as_deref().ok_or_else(|| {
                "project_path is required for daemon runs; process cwd fallback is disabled"
                    .to_string()
            })?;
            let child_directive = self.runtime.take_run_agent_directive(&run.id).await;
            // P1-01 / PERF-002: warm prepare — the static prompt layers are
            // keyed by project identity + instruction fingerprint + capability
            // audit + provider/model/runtime. The cache is never used when a
            // per-run child directive is present (those are run-specific and
            // cannot be frozen). Credentials/permission/run-id are never part
            // of the key or the payload.
            //
            // PERF-002: the lookup is two-tier. The fast tier builds the key
            // from a METADATA fingerprint (canonical path + mtime/size +
            // skill-directory generation) — no instruction content is read and
            // no skill body is scanned before the lookup. Only when that
            // misses does the slow tier compute the full content SHA-256 and
            // consult the durable content-verified key. The skill prompt is
            // likewise deferred to the rebuild branch, so a warm hit performs
            // zero instruction I/O and zero skill discovery.
            use crate::prepared_session::PreparedResolve;
            let resolve = if child_directive.is_none() {
                self.runtime.prepared.resolve(
                    project_root,
                    &project_root.to_string_lossy(),
                    crate::prepared_session::capability_audit_revision(
                        &capability_snapshot.to_audit_json(),
                    ),
                    0,
                    &provider_id,
                    &model_id,
                    &runtime_id,
                    0,
                )
            } else {
                // A per-run child directive cannot be frozen into a cache key.
                PreparedResolve::NoCache
            };
            let effective_prompt = match resolve {
                // Warm metadata-tier hit: no skill scan, no instruction read.
                PreparedResolve::Hit(session) | PreparedResolve::ContentUnchanged(session) => {
                    session.effective_prompt.clone()
                }
                PreparedResolve::Miss {
                    fast_key,
                    full_digest,
                } => {
                    // Skills are selection-scoped when Capability Hub supplied
                    // an explicit selection; otherwise retain the existing
                    // trusted project catalog. Bodies remain progressive-
                    // disclosure only. Deferred to this branch (PERF-002).
                    let skill_prompt = capability_snapshot
                        .skill_prompt
                        .clone()
                        .unwrap_or_else(|| crate::skill_store::prompt_for_project(project_root));
                    let compiled = crate::production::compile_effective_prompt(
                        capability_snapshot
                            .agent_profile_id
                            .as_deref()
                            .or(run.agent_profile_id.as_deref()),
                        capability_snapshot.profile.as_ref(),
                        child_directive.as_deref(),
                        Some(project_root),
                        (!skill_prompt.is_empty()).then_some(skill_prompt.as_str()),
                        &harness_plan.prompt_blocks,
                        &harness_plan.builtin_prompt_replacements,
                        capability_snapshot.extra_system_prompt.as_deref(),
                    );
                    let session = crate::prepared_session::PreparedAgentSession {
                        effective_prompt: compiled.clone(),
                        prompt_digest: compiled.effective_full_text.clone(),
                        frozen_tool_schemas: frozen_tool_schemas.clone(),
                        skill_catalog_metadata: Vec::new(),
                    };
                    // Index under BOTH keys: the full content digest is the
                    // durable content-verified key; the metadata key makes the
                    // next identical lookup hit without any content I/O.
                    let full_key = crate::prepared_session::PreparedAgentSessionKey {
                        project_instruction_digest: full_digest,
                        ..fast_key.clone()
                    };
                    self.runtime.prepared.insert(full_key, session.clone());
                    self.runtime.prepared.insert(fast_key, session);
                    compiled
                }
                // Child-directive path: rebuild, never insert a run-specific
                // prompt into the static cache.
                PreparedResolve::NoCache => {
                    let skill_prompt = capability_snapshot
                        .skill_prompt
                        .clone()
                        .unwrap_or_else(|| crate::skill_store::prompt_for_project(project_root));
                    crate::production::compile_effective_prompt(
                        capability_snapshot
                            .agent_profile_id
                            .as_deref()
                            .or(run.agent_profile_id.as_deref()),
                        capability_snapshot.profile.as_ref(),
                        child_directive.as_deref(),
                        Some(project_root),
                        (!skill_prompt.is_empty()).then_some(skill_prompt.as_str()),
                        &harness_plan.prompt_blocks,
                        &harness_plan.builtin_prompt_replacements,
                        capability_snapshot.extra_system_prompt.as_deref(),
                    )
                }
            };
            effective_prompt
        } else {
            // Non-Native backends have their own prompt authority (for example
            // Claude CLI flags). Do not project a Native prompt they did not use.
            harness_core::PromptPlanBuilder::new().build()
        };
        if let Err(error) = crate::rpc::harness::control_plane::persist_run_plan(
            &mut harness_plan,
            &effective_prompt,
        ) {
            self.fail_run_if_active(&run.id, error.to_string(), error.code);
            return Err(error.to_string());
        }
        // A retry/continue plan is only consumed once the detached run has
        // passed preparation and its immutable execution plan is durable. A
        // provider/tool failure after this point is a real new-run outcome,
        // not a reason to revive the source future.
        self.mark_resume_plan_executed_for_run(&run)?;
        // Keep selected MCP servers warm for the run's lifetime (refcounted;
        // released on every exit path below via this guard).
        for server_id in &capability_snapshot.mcp_servers {
            crate::mcp_runtime::global_mcp().acquire(server_id, &run.id);
        }
        let _mcp_refs = McpRunRefGuard {
            run_id: run.id.clone(),
        };

        // REQ-T02: Codex remains fail-closed (app-server not implemented).
        if runtime_id == "codex_cli" {
            let cancel = self
                .runtime
                .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
                .await?;
            let err = match crate::codex_runtime_bridge::run_codex_cli_turn(
                &self.runtime,
                &run.id,
                &content,
                &model_id,
                request_project_path
                    .as_deref()
                    .or(run.project_path.as_deref())
                    .map(std::path::PathBuf::from)
                    .as_deref(),
                &permission_profile,
                cancel,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => e,
            };
            self.runtime.execution.mark_finished(&run.id).await;
            self.commit_status(
                &run.id,
                RunStatusV2::Failed,
                TransitionMetadata::empty()
                    .with_error_code("CODEX_UNAVAILABLE")
                    .with_reason(err.clone())
                    .with_lifecycle_hint("failed"),
            )?;
            return Err(if err.contains("unavailable") {
                err
            } else {
                "runtime codex_cli is unavailable (app-server not implemented)".into()
            });
        }

        // REQ-T01: Claude CLI session main path (stream-json → v2 events).
        if runtime_id == "claude_cli"
            && std::env::var("NATIVES_DAEMON_FIXTURE").ok().as_deref() != Some("1")
            && !cfg!(test)
        {
            let cancel = self
                .runtime
                .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
                .await?;
            // Commit Running before the turn: Preparing→{Completed,Cancelled} are not
            // legal edges, but Running→terminal are. CLI is actively running here.
            self.commit_status(
                &run.id,
                RunStatusV2::Running,
                TransitionMetadata::empty().with_lifecycle_hint("running"),
            )?;
            let project = request_project_path
                .as_deref()
                .or(run.project_path.as_deref())
                .map(std::path::PathBuf::from);
            let terminal = match crate::cli_runtime_bridge::run_claude_cli_turn(
                &self.runtime,
                &run.id,
                &content,
                &model_id,
                project.as_deref(),
                &permission_profile,
                &capability_snapshot,
                cancel,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => {
                    self.runtime.execution.mark_finished(&run.id).await;
                    // Do not append terminal events here; commit_status is sole lifecycle writer.
                    let meta = TransitionMetadata::empty()
                        .with_error_code("CLI_RUNTIME")
                        .with_reason(e)
                        .with_lifecycle_hint("failed");
                    self.commit_status(&run.id, RunStatusV2::Failed, meta)?;
                    return self
                        .get_run(&run.id)
                        .ok_or_else(|| "run missing after cli turn".to_string());
                }
            };
            self.runtime.execution.mark_finished(&run.id).await;
            let final_status = match terminal.as_str() {
                "completed" => RunStatusV2::Completed,
                "cancelled" => RunStatusV2::Cancelled,
                "interrupted" => RunStatusV2::Interrupted,
                _ => RunStatusV2::Failed,
            };
            let meta = match final_status {
                RunStatusV2::Failed => TransitionMetadata::empty()
                    .with_error_code("CLI_RUNTIME")
                    .with_lifecycle_hint("failed"),
                RunStatusV2::Cancelled => TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("cancelled"),
                RunStatusV2::Interrupted => TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("interrupted"),
                _ => TransitionMetadata::empty().with_lifecycle_hint(final_status.as_str()),
            };
            self.commit_status(&run.id, final_status, meta)?;
            return self
                .get_run(&run.id)
                .ok_or_else(|| "run missing after cli turn".to_string());
        }

        // Prefer real provider when credentials exist; otherwise fixture (tests only).
        let use_fixture = std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || crate::production::resolve_credential(&provider_id, key_id.as_deref()).is_err();

        if use_fixture && std::env::var("NATIVES_DAEMON_FIXTURE").is_err() {
            // Production without keys fails clearly rather than Echo mock success.
            if std::env::var("NATIVES_ALLOW_FIXTURE_FALLBACK")
                .ok()
                .as_deref()
                != Some("1")
            {
                // Still allow fixture for unit tests via explicit flag only.
                // For daemon tests we set NATIVES_DAEMON_FIXTURE=1.
                if cfg!(test) {
                    // unit tests can use fixture
                } else {
                    let err = format!(
                        "No credentials for provider '{provider_id}'. Configure an active key in Settings."
                    );
                    self.fail_run_if_active(&run.id, err.clone(), "NO_CREDENTIALS");
                    return Err(err);
                }
            }
        }

        let use_fixture_engine =
            std::env::var("NATIVES_DAEMON_FIXTURE").ok().as_deref() == Some("1") || cfg!(test);

        if use_fixture_engine {
            // Deterministic offline path for tests — real engine + permission tools,
            // never EchoProvider fake success text.
            let hooks = harness_plan.compile(
                request_project_path
                    .as_deref()
                    .or(run.project_path.as_deref())
                    .map(std::path::Path::new),
            );
            let cancel = self
                .runtime
                .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
                .await
                .unwrap_or_else(|_| CancellationToken::new());
            let engine = Arc::new(
                AgentEngine::with_live(self.runtime.events.clone(), self.runtime.live.clone())
                    .with_cancel_token(cancel)
                    .with_hooks(hooks)
                    .with_progress_sink(Arc::new(
                        crate::production_tools::DaemonToolProgressSink::new(
                            self.runtime.live.clone(),
                        ),
                    )),
            );
            self.runtime.register_engine(&run.id, engine.clone()).await;
            let provider = FixtureProvider {
                mode: FixtureMode::TextOnly,
            };
            let tool_allowlist = self.runtime.take_run_tool_allowlist(&run.id).await;
            let tools = crate::production::PermissionGatedTools {
                gateway: {
                    let mut g = capability_gateway::CapabilityGateway::new();
                    let _ = g.register_builtins();
                    if let Some(root) = request_project_path
                        .as_deref()
                        .or(run.project_path.as_deref())
                    {
                        g.set_project_root(root.to_string());
                    }
                    if let Some(ref list) = tool_allowlist {
                        // Restrict fixture gateway surface to allowlist names when present.
                        // Builtins still registered; PermissionGatedTools enforces allowlist.
                        let _ = list;
                    }
                    Arc::new(g)
                },
                permissions: self.runtime.permissions.clone(),
                events: self.runtime.events.clone(),
                interactions: self.runtime.interactions.clone(),
                subagents: self.runtime.subagents.clone(),
                task_outputs: self.runtime.task_outputs_ref(),
                engines: self.runtime.engine_handles().await,
                runtime: Some(self.runtime.clone()),
                provider_id: provider_id.clone(),
                key_id: key_id.clone(),
                parent_run_id: run.id.clone(),
                conversation_id: run.conversation_id.clone(),
                model_id: model_id.clone(),
                permission_profile: permission_profile.clone(),
                tool_allowlist,
                team: None,
                mcp_tool_schemas: Vec::new(),
                selected_mcp_servers: None,
            };
            let legacy_history = crate::conversation_store::engine_history(&run.conversation_id)?;
            let typed_history =
                crate::conversation_store::load_agent_messages(&run.conversation_id)?;
            let typed_history = if typed_history.is_empty() && !legacy_history.is_empty() {
                agent_core::engine_messages_to_agent_messages(&legacy_history)
            } else {
                typed_history
            };
            let config = EngineRunConfig {
                run_id: run.id.clone(),
                conversation_id: run.conversation_id.clone(),
                model: model_id.clone(),
                system_prompt: (!effective_prompt.effective_full_text.is_empty())
                    .then(|| effective_prompt.effective_full_text.clone()),
                messages: legacy_history,
                user_content: content,
                max_steps,
            };
            if let Err(error) =
                crate::production_tools::validate_tool_limit(frozen_tool_schemas.len())
            {
                self.fail_run_if_active(&run.id, error.clone(), "tool_plan_too_large");
                return Err(error);
            }
            let outcome = match engine
                .run_with_typed_messages(
                    config,
                    &provider,
                    &tools,
                    frozen_tool_schemas,
                    typed_history,
                )
                .await
            {
                Ok(o) => o,
                Err(e) => EngineOutcome::failed(e.code(), e.to_string(), e.retryable()),
            };
            self.runtime.remove_engine(&run.id).await;
            // A8: watermark-driven incremental projection — replays only events
            // after the run's projection watermark, never the full stream from
            // sequence 0. Idempotency/quarantine/partial-turn semantics are
            // unchanged (project_committed_turn).
            crate::conversation_projector::project_run_incremental(&run.conversation_id, &run.id)?;
            return self.commit_outcome(&run.id, &outcome);
        }

        let project_path = self.resolve_project_path(&run.id, request_project_path.as_deref());
        self.store_project_path(
            &run.id,
            project_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .as_deref(),
        );
        let outcome = self
            .runtime
            .start_run(crate::production::RunStartContext {
                run_id: run.id.clone(),
                parent_run_id: run.parent_run_id.clone(),
                conversation_id: run.conversation_id.clone(),
                provider_id,
                model_id,
                key_id,
                permission_profile,
                agent_profile_id: capability_snapshot
                    .agent_profile_id
                    .clone()
                    .or_else(|| run.agent_profile_id.clone()),
                user_content: content,
                max_steps,
                project_path: project_path.clone(),
                capability: Some(capability_snapshot),
                hooks: Some(harness_plan.compile(project_path.as_deref())),
                effective_prompt,
                frozen_tool_schemas,
            })
            .await?;
        // Sole terminal commit from EngineOutcome — never scan events or default Completed.
        self.commit_outcome(&run.id, &outcome)
    }
    /// Start with explicit seams (tests / advanced callers).
    pub async fn start_with_seams(
        &self,
        req: StartRunRequest,
        provider: &dyn agent_core::EngineProvider,
        tools: &dyn agent_core::EngineToolRuntime,
    ) -> Result<RunV2, String> {
        let run = if let Some(run_id) = &req.run_id {
            self.get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?
        } else {
            let conversation_id = req
                .conversation_id
                .clone()
                .ok_or_else(|| "conversation_id required".to_string())?;
            self.create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id,
                provider_id: req.provider_id.clone().unwrap_or_default(),
                model_id: req.model_id.clone().unwrap_or_default(),
                key_id: req.key_id.clone(),
                agent_profile_id: None,
                permission_profile: req.permission_profile.clone(),
                content: req.content.clone(),
                attachments: req.attachments.clone(),
                max_steps: req.max_steps,
                parent_run_id: None,
                project_path: None,
                idempotency_key: req.idempotency_key.clone(),
                effort: None,
                runtime_id: None,
            })?
        };

        // Empty default: do not invent a synthetic "continue" user turn when the
        // conversation history already holds the user messages (history reload tests).
        let content = req.content.clone().unwrap_or_default();
        // Register engine so cancel_run → request_cancel works mid-flight.
        let cancel = self
            .runtime
            .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
            .await
            .map_err(|error| format!("register run cancellation token: {error}"))?;
        let engine = Arc::new(
            AgentEngine::with_live(self.runtime.events.clone(), self.runtime.live.clone())
                .with_cancel_token(cancel),
        );
        self.runtime.register_engine(&run.id, engine.clone()).await;
        let _ = self.commit_status(
            &run.id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        );
        // Production path: load typed messages directly instead of legacy
        // EngineMessage. The typed transcript is the single source of truth for
        // the provider context; the config.messages field (Vec<EngineMessage>) is
        // unused when typed_transcript is provided.
        let typed_history = crate::conversation_store::load_agent_messages(&run.conversation_id)?;
        let config = EngineRunConfig {
            run_id: run.id.clone(),
            conversation_id: run.conversation_id.clone(),
            model: req.model_id.unwrap_or_else(|| run.model_id.clone()),
            system_prompt: None,
            messages: Vec::new(),
            user_content: content,
            max_steps: req.max_steps.unwrap_or(run.max_steps),
        };
        let tool_schemas = tools.list_tool_schemas().await;
        crate::production_tools::validate_tool_limit(tool_schemas.len())?;
        let outcome = match engine
            .run_with_typed_messages(config, provider, tools, tool_schemas, typed_history)
            .await
        {
            Ok(o) => o,
            Err(e) => EngineOutcome::failed(e.code(), e.to_string(), e.retryable()),
        };
        let outcome = if matches!(outcome, EngineOutcome::Completed { .. })
            // A8: watermark-driven incremental projection — replays only events
            // after the run's projection watermark (never from sequence 0).
            && crate::conversation_projector::project_run_incremental(
                &run.conversation_id,
                &run.id,
            )
            .is_err()
        {
            EngineOutcome::failed(
                "persist_assistant",
                "failed to persist assistant turn",
                false,
            )
        } else {
            outcome
        };
        self.runtime.remove_engine(&run.id).await;
        self.commit_outcome(&run.id, &outcome)
    }
    /// Non-blocking start for RPC / UI: returns immediately with Preparing status.
    /// Engine execution continues on a background task; clients poll events / cancel
    /// on the same session without waiting for completion.
    ///
    /// Idempotent: if the run is already active (or terminal), does **not** spawn a
    /// second engine — returns the current row (duplicate Start is a no-op).
    pub fn start_detached(self: &Arc<Self>, req: StartRunRequest) -> Result<RunV2, String> {
        let run = self.ensure_run_for_start(&req)?;

        // Honest runtime gate (REQ-T01/T02): codex never executable; claude_cli only if binary present.
        if let Some(rt) = req
            .runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match rt {
                "native" | "" => {}
                "codex_cli" => {
                    // Single source of truth for T02 red line.
                    if !crate::codex_runtime_bridge::codex_cli_available() {
                        return Err(
                            "runtime codex_cli is unavailable (app-server not implemented)".into(),
                        );
                    }
                }
                "claude_cli" => {
                    if !crate::cli_runtime_bridge::claude_cli_available() {
                        return Err(
                            "runtime claude_cli unavailable (claude binary not found)".into()
                        );
                    }
                }
                other => {
                    return Err(format!("unknown runtime_id: {other}"));
                }
            }
        }
        if run.status.is_active() {
            return Ok(run);
        }
        if run.status.is_terminal() {
            return Err(format!(
                "run {} is terminal ({:?}); use run.retry",
                run.id,
                run.status.as_str()
            ));
        }
        let mut req = req;
        req.run_id = Some(run.id.clone());
        if let Some(content) = &req.content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content.clone());
        }
        self.store_project_path(&run.id, req.project_path.as_deref());
        let preparing = self.mark_preparing(&run.id)?;
        self.persist_runs_snapshot()?;
        let rm = Arc::clone(self);
        let run_id_for_fail = preparing.id.clone();
        tokio::spawn(async move {
            if let Err(err) = rm.start(req).await {
                rm.fail_run_if_active(&run_id_for_fail, err, "START_FAILED");
            }
        });
        Ok(preparing)
    }
    /// Process-wide non-blocking start (sidecar RPC uses this via `global_run_manager`).
    /// Same idempotency rules as [`start_detached`].
    pub fn start_detached_global(req: StartRunRequest) -> Result<RunV2, String> {
        let rm = global_run_manager();
        let run = rm.ensure_run_for_start(&req)?;

        // Honest runtime gate (REQ-T01/T02): codex never executable; claude_cli only if binary present.
        if let Some(rt) = req
            .runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match rt {
                "native" | "" => {}
                "codex_cli" => {
                    // Single source of truth for T02 red line.
                    if !crate::codex_runtime_bridge::codex_cli_available() {
                        return Err(
                            "runtime codex_cli is unavailable (app-server not implemented)".into(),
                        );
                    }
                }
                "claude_cli" => {
                    if !crate::cli_runtime_bridge::claude_cli_available() {
                        return Err(
                            "runtime claude_cli unavailable (claude binary not found)".into()
                        );
                    }
                }
                other => {
                    return Err(format!("unknown runtime_id: {other}"));
                }
            }
        }
        if run.status.is_active() {
            return Ok(run);
        }
        if run.status.is_terminal() {
            return Err(format!(
                "run {} is terminal ({:?}); use run.retry",
                run.id,
                run.status.as_str()
            ));
        }
        let mut req = req;
        req.run_id = Some(run.id.clone());
        if let Some(content) = &req.content {
            rm.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content.clone());
        }
        rm.store_project_path(&run.id, req.project_path.as_deref());
        let preparing = rm.mark_preparing(&run.id)?;
        rm.persist_runs_snapshot()?;
        let run_id_for_fail = preparing.id.clone();
        tokio::spawn(async move {
            let rm = global_run_manager();
            if let Err(err) = rm.start(req).await {
                rm.fail_run_if_active(&run_id_for_fail, err, "START_FAILED");
            }
        });
        Ok(preparing)
    }
    /// Ensure a run row exists for `start` / `start_detached` (create if `run_id` absent).
    ///
    /// Always ensures the current user turn is recorded in the daemon conversation store
    /// under a daemon-owned `trigger_message_id` (never reuses host message ids as FKs).
    /// Ensure a run exists for start. Appends a daemon-local user message when content
    /// is present. When this manager is memory-only (`data_store` is None), skip SQLite
    /// message writes so pure unit tests never touch the developer's assistant.db.
    pub fn ensure_run_for_start(&self, req: &StartRunRequest) -> Result<RunV2, String> {
        let can_write_messages = self.data_store.is_some();
        // Existing run (host create_run + start path): still append daemon-local user message.
        if let Some(run_id) = &req.run_id {
            let mut run = self
                .get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if can_write_messages
                && run.trigger_message_id.is_none()
                && (req.content.as_ref().is_some_and(|c| !c.trim().is_empty())
                    || req.attachments.as_ref().is_some_and(|a| !a.is_empty()))
            {
                if let Some(id) = self.append_user_message_on_store(
                    &run.conversation_id,
                    req.content.as_deref(),
                    req.attachments.as_deref(),
                )? {
                    {
                        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
                        if let Some(stored) = runs.get_mut(&run.id) {
                            stored.trigger_message_id = Some(id.clone());
                            run = stored.clone();
                        }
                    }
                    self.persist_run_row(&run)?;
                    self.persist_runs_snapshot()?;
                }
            }
            return Ok(run);
        }
        if let Some(key) = &req.idempotency_key {
            let map = self.idempotency.lock().map_err(|e| e.to_string())?;
            if let Some(existing) = map.get(key) {
                let runs = self.runs.lock().map_err(|e| e.to_string())?;
                if let Some(run) = runs.get(existing) {
                    return Ok(run.clone());
                }
            }
        }
        let conversation_id = req
            .conversation_id
            .clone()
            .ok_or_else(|| "conversation_id required".to_string())?;
        let trigger_message_id = if can_write_messages {
            self.append_user_message_on_store(
                &conversation_id,
                req.content.as_deref(),
                req.attachments.as_deref(),
            )?
        } else {
            None
        };
        let mut run = match self.create_run(CreateRunRequest {
            // Seam A (ADR-0016): profile + selection flow through instead of
            // being dropped at the gateway boundary.
            capability_selection: req.capability_selection.clone(),
            disabled_tools: None,
            conversation_id: conversation_id.clone(),
            provider_id: req.provider_id.clone().unwrap_or_default(),
            model_id: req.model_id.clone().unwrap_or_default(),
            key_id: req.key_id.clone(),
            agent_profile_id: req.agent_profile_id.clone(),
            permission_profile: req.permission_profile.clone().or_else(|| {
                // Prefer conversation-row profile when available on this store.
                if let Some(store) = &self.data_store {
                    store.conn().ok().and_then(|conn| {
                        conn.query_row(
                            "SELECT COALESCE(permission_profile_id, 'ask') FROM conversation WHERE id = ?1",
                            rusqlite::params![conversation_id],
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                    })
                } else {
                    None
                }
            }),
            content: req.content.clone(),
            attachments: req.attachments.clone(),
            max_steps: req.max_steps,
            parent_run_id: None,
            project_path: req.project_path.clone(),
            idempotency_key: req.idempotency_key.clone(),
            effort: req.effort.clone(),
            runtime_id: req.runtime_id.clone(),
        }) {
            Ok(run) => run,
            Err(error) => {
                // Roll back the pre-created trigger message if create_run failed.
                if let (Some(store), Some(id)) = (&self.data_store, trigger_message_id.as_deref()) {
                    let _ = store.conn().and_then(|conn| {
                        conn.execute("DELETE FROM message WHERE id = ?1", rusqlite::params![id])
                            .map_err(|e| e.to_string())
                    });
                }
                return Err(error);
            }
        };
        if let Some(trigger_message_id) = trigger_message_id {
            {
                let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
                if let Some(stored) = runs.get_mut(&run.id) {
                    stored.trigger_message_id = Some(trigger_message_id.clone());
                    run = stored.clone();
                }
            }
            self.persist_run_row(&run)?;
            self.persist_runs_snapshot()?;
        }
        Ok(run)
    }
    /// Insert a user message on **this** manager's DataStore (never env-open another DB).
    fn append_user_message_on_store(
        &self,
        conversation_id: &str,
        content: Option<&str>,
        attachments: Option<&[assistant_protocol::v2::AttachmentRef]>,
    ) -> Result<Option<String>, String> {
        let Some(store) = &self.data_store else {
            return Ok(None);
        };
        let mut blocks = Vec::new();
        if let Some(content) = content.filter(|s| !s.trim().is_empty()) {
            blocks.push(serde_json::json!({ "type": "text", "text": content }));
        }
        for attachment in attachments.unwrap_or(&[]) {
            if attachment.path.trim().is_empty() {
                continue;
            }
            blocks.push(serde_json::json!({
                "type": "file_reference",
                "path": attachment.path,
                "name": attachment.name.clone(),
                "mime_type": attachment.mime_type.clone(),
                "size": attachment.size,
            }));
        }
        if blocks.is_empty() {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let conn = store.conn()?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO message (id, conversation_id, role, status, created_at)
             VALUES (?1, ?2, 'user', 'complete', ?3)",
            rusqlite::params![id, conversation_id, now],
        )
        .map_err(|e| e.to_string())?;
        for (index, block) in blocks.iter().enumerate() {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            let content = if block_type == "text" {
                serde_json::json!({
                    "text": block.get("text").and_then(|v| v.as_str()).unwrap_or_default()
                })
            } else {
                block.clone()
            };
            tx.execute(
                "INSERT INTO message_block (message_id, sort_order, block_type, block_json)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, index as i64, block_type, content.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id],
        )
        .and_then(|_| tx.commit())
        .map_err(|e| e.to_string())?;
        Ok(Some(id))
    }
    fn mark_preparing(&self, run_id: &str) -> Result<RunV2, String> {
        self.commit_status(
            run_id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        )
    }
}
