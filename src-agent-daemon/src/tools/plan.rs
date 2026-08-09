//! Plan Mode: the Plan Mode latch, gear transitions, and human plan approval.

use agent_core::ToolExecutionResult;
use assistant_protocol::v2::RunEventKind;
use capability_gateway::plan_mode::{self, PlanDecision};
use capability_gateway::SideEffect;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::gated::PermissionGatedTools;

/// How long a plan approval card may sit unanswered before it is treated as a
/// rejection. Longer than a normal tool-permission prompt because approving a
/// plan is a materially different act from clicking "yes" on one command, and
/// shorter than forever because a run that nobody is watching must eventually
/// stop occupying a slot. A timeout is a rejection: the latch stays closed.
pub const PLAN_APPROVAL_TIMEOUT_SECS: u64 = 300;

/// Scope recorded when a plan approval resolves. Plan approval is a one-shot
/// gear change for a single run and is never a reusable grant.
pub const PLAN_APPROVAL_SCOPE: &str = "once";

impl PermissionGatedTools {
    /// Close the Plan Mode latch for a run the host started in Plan Mode.
    ///
    /// The permission profile on a run is immutable by design, so a host that
    /// wants Plan Mode says so once by passing the `plan` profile. This turns
    /// that declaration into a real session record the first time the run
    /// touches the tool surface, which is what gives approval something to
    /// release and gives the run a fallback profile to land on.
    pub(crate) fn ensure_plan_latch(&self) {
        if self
            .permission_profile
            .trim()
            .eq_ignore_ascii_case(plan_mode::PLAN_PROFILE)
            && plan_mode::snapshot(&self.parent_run_id).is_none()
        {
            plan_mode::enter(&self.parent_run_id, plan_mode::PLAN_PROFILE);
            self.emit_plan_transition(plan_mode::PlanTransition::Entered, None);
        }
    }

    /// Put a Plan Mode gear change on the run's event stream.
    ///
    /// Always call this **after** the mutation: the payload is built from a
    /// fresh snapshot, so an `Approved` emitted too early would report the plan
    /// ceiling as the profile in force and a `Rejected` would undercount the
    /// rejections. A run whose session has already been torn down emits
    /// nothing rather than a synthesised one — an invented gear change is worse
    /// than a missing one.
    pub(crate) fn emit_plan_transition(
        &self,
        transition: plan_mode::PlanTransition,
        reason: Option<&str>,
    ) {
        let Some(session) = plan_mode::snapshot(&self.parent_run_id) else {
            return;
        };
        self.events.append(
            &self.parent_run_id,
            plan_mode::changed_event(&session, transition, reason),
        );
    }

    /// The gear this run actually runs under: the plan ceiling while planning,
    /// the profile captured at entry once a plan is approved, otherwise the
    /// declared profile untouched.
    pub(crate) fn effective_permission_profile(&self) -> String {
        plan_mode::effective_profile(&self.parent_run_id, &self.permission_profile)
    }

    /// Plan Mode verdict for a tool call, fail-closed for anything the gateway
    /// does not know about (raw `mcp__*` names, orchestration stubs).
    pub(crate) fn plan_decision_for(&self, name: &str) -> PlanDecision {
        match self.gateway.get_tool(name) {
            Some(tool) => plan_mode::decision(name, tool.side_effect, tool.permission_class),
            None => plan_mode::decision(
                name,
                SideEffect::Destructive,
                capability_gateway::PermissionClass::Elevation,
            ),
        }
    }

    /// `enter_plan_mode`: close the latch for this run.
    ///
    /// Handled here rather than left to the gateway loop below so it skips the
    /// ProjectIdentity gate. Entering Plan Mode only ever removes capability, so
    /// there is nothing for a project binding to protect — and refusing it on an
    /// unbound run would deny the agent the one move that makes an unbound run
    /// safer.
    pub(crate) async fn handle_enter_plan_mode(&self, input: Value) -> ToolExecutionResult {
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(CancellationToken::new)
        } else {
            CancellationToken::new()
        };
        let context = self
            .build_tool_call_context(
                uuid::Uuid::new_v4().to_string(),
                cancel,
                None,
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
                None,
                None,
            )
            .await;
        match self
            .gateway
            .execute(plan_mode::ENTER_PLAN_MODE_TOOL, input, &context)
            .await
        {
            Ok(out) => ToolExecutionResult {
                output: out.result,
                is_error: false,
                duration_ms: out.duration_ms,
            },
            Err(err) => ToolExecutionResult {
                output: serde_json::json!({"error": err.message, "code": err.code}),
                is_error: true,
                duration_ms: 0,
            },
        }
    }

    /// `exit_plan_mode`: submit a plan and block on a real human answer.
    ///
    /// The whole safety property of Plan Mode lives in this function: the model
    /// hands over a structured plan, the user is the one who answers, and the
    /// latch is released *only* on an approval that came back through the
    /// interaction hub. Nothing the model can say short-circuits it.
    pub(crate) async fn handle_exit_plan_mode(
        &self,
        input: Value,
        planning: bool,
    ) -> ToolExecutionResult {
        if !planning {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": "this run is not in Plan Mode",
                    "code": "not_in_plan_mode",
                    "approved": false,
                }),
                is_error: true,
                duration_ms: 0,
            };
        }
        let plan = match plan_mode::parse_plan(&input) {
            Ok(plan) => plan,
            Err(err) => {
                // A malformed plan is the model's problem to fix, not the
                // user's to squint at. Never show a card built from it.
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": err.message,
                        "code": err.code,
                        "approved": false,
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        if let Err(err) = plan_mode::record_submission(&self.parent_run_id, plan.clone()) {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": err.message,
                    "code": err.code,
                    "approved": false,
                }),
                is_error: true,
                duration_ms: 0,
            };
        }

        self.emit_plan_transition(plan_mode::PlanTransition::Submitted, None);

        let plan_json = serde_json::to_value(&plan).unwrap_or_else(|_| serde_json::json!({}));
        let started = Instant::now();
        let (approved, _scope) = self.await_plan_approval(&plan, &plan_json).await;

        if !approved {
            let rejections = plan_mode::reject(&self.parent_run_id).unwrap_or(0);
            self.emit_plan_transition(plan_mode::PlanTransition::Rejected, None);
            // Not `is_error`: a rejection is a legitimate answer to a question
            // the model asked. Flagging it as a failure invites retry logic to
            // treat "the user said no" as a transient fault.
            return ToolExecutionResult {
                output: serde_json::json!({
                    "approved": false,
                    "plan_mode": true,
                    "rejections": rejections,
                    "message": "The user did not approve this plan. You are still in Plan Mode: \
                                nothing has been executed. Ask what they want changed, revise, \
                                and submit again. Do not claim the plan was approved.",
                }),
                is_error: false,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }

        match plan_mode::approve(&self.parent_run_id) {
            Ok(profile) => {
                self.emit_plan_transition(plan_mode::PlanTransition::Approved, None);
                ToolExecutionResult {
                    output: serde_json::json!({
                        "approved": true,
                        "plan_mode": false,
                        "permission_profile": profile,
                        "plan": plan_json,
                        "message": "The user approved this plan. Plan Mode is off and the run is back \
                                    on its original permission profile. Execute the approved steps and \
                                    nothing beyond them.",
                    }),
                    is_error: false,
                    duration_ms: started.elapsed().as_millis() as u64,
                }
            }
            Err(err) => ToolExecutionResult {
                output: serde_json::json!({
                    "error": err.message,
                    "code": err.code,
                    "approved": false,
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        }
    }

    /// Put a plan in front of a human and wait.
    ///
    /// Deliberately does **not** go through `PermissionManager`: that path
    /// auto-approves on `autonomous` and refuses outright on `readonly`, and
    /// both would be wrong here. Plan approval is the one interaction that must
    /// always reach a person, whatever the profile says — an autonomous run in
    /// Plan Mode is exactly the case the gear was built for.
    ///
    /// Recorded as a `tool_permission` interaction so the existing respond RPC
    /// and the restart-recovery path resolve it unchanged; the GUI tells it
    /// apart by `tool_name` and renders the plan from the event payload.
    pub(crate) async fn await_plan_approval(
        &self,
        plan: &capability_gateway::Plan,
        plan_json: &Value,
    ) -> (bool, String) {
        let permission_id = uuid::Uuid::new_v4().to_string();
        let tool_call_id = uuid::Uuid::new_v4().to_string();
        let reason = format!("Approve plan: {}", plan.title);
        let card = serde_json::json!({
            "kind": "plan_approval",
            "plan": plan_json,
            "step_count": plan.steps.len(),
            "peak_risk": plan.peak_risk(),
            "has_irreversible_step": plan.has_irreversible_step(),
        });

        // Install the waiter before publishing the event, same race as the tool
        // permission path: a fast UI must not answer a question nobody is
        // listening for.
        let (tx, rx) = oneshot::channel::<(bool, String)>();
        self.interactions
            .register_permission(
                &permission_id,
                &self.parent_run_id,
                plan_mode::EXIT_PLAN_MODE_TOOL,
                tx,
            )
            .await;
        if crate::interaction_store::insert_pending(
            &permission_id,
            Some(&self.parent_run_id),
            Some(&self.conversation_id),
            "tool_permission",
            serde_json::json!({
                "tool_call_id": tool_call_id,
                "tool_name": plan_mode::EXIT_PLAN_MODE_TOOL,
                "reason": reason,
                "input": card,
            }),
        )
        .is_err()
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            return (false, "persistence_failed".into());
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, Some(permission_id.clone()));
        if crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id).is_err() {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            return (false, "persistence_failed".into());
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionRequested {
                    tool_call_id,
                    tool_name: plan_mode::EXIT_PLAN_MODE_TOOL.to_string(),
                    reason: reason.clone(),
                    permission_id: permission_id.clone(),
                    input: card,
                },
            )
            .is_err()
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return (false, "persistence_failed".into());
        }

        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(CancellationToken::new)
        } else {
            CancellationToken::new()
        };
        let (approved, scope) = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = self.interactions.resolve_permission(&permission_id).await;
                (false, "cancelled".to_string())
            }
            res = tokio::time::timeout(
                Duration::from_secs(PLAN_APPROVAL_TIMEOUT_SECS),
                rx,
            ) => {
                // Silence is not consent. Any failure to hear a real "yes"
                // resolves to rejection and leaves the latch closed.
                res.ok()
                    .and_then(|r| r.ok())
                    .map(|(ok, _)| (ok, PLAN_APPROVAL_SCOPE.to_string()))
                    .unwrap_or((false, "timeout".to_string()))
            }
        };

        if crate::interaction_store::mark_resolved(
            &permission_id,
            serde_json::json!({ "approved": approved, "scope": scope }),
        )
        .is_err()
        {
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return (false, "persistence_failed".into());
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, None);
        if crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id).is_err() {
            return (false, "persistence_failed".into());
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionResponded {
                    permission_id,
                    approved,
                    scope: scope.clone(),
                },
            )
            .is_err()
        {
            return (false, "persistence_failed".into());
        }
        (approved, scope)
    }
}

/// Plan Mode as the tool runtime actually enforces it.
///
/// The gateway unit tests pin the latch and the plan shape; these pin the part
/// that can only be observed from here — which tools the model is shown, that a
/// blocked write never becomes a permission prompt, and that the latch opens
/// only for an approval that came back through the interaction hub.

#[cfg(test)]
mod plan_mode_runtime_tests {
    use super::*;
    use crate::production::ProductionRuntime;
    use agent_core::EngineToolRuntime;
    use capability_gateway::CapabilityGateway;

    fn tools_for(run_id: &str, profile: &str, root: &std::path::Path) -> PermissionGatedTools {
        let rt = ProductionRuntime::new();
        let mut gateway = CapabilityGateway::new();
        gateway.set_project_root(root.to_string_lossy().to_string());
        let _ = gateway.register_builtins();
        PermissionGatedTools {
            gateway: Arc::new(gateway),
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "test".into(),
            key_id: None,
            parent_run_id: run_id.to_string(),
            conversation_id: format!("conv-{run_id}"),
            model_id: "test-model".into(),
            permission_profile: profile.to_string(),
            tool_allowlist: None,
            // No capability selection: these fixtures exercise Plan Mode, not
            // the ADR-0016 team/MCP gates, and `None` is the legacy surface.
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        }
    }

    fn run_id(tag: &str) -> String {
        format!("plan-{tag}-{}", uuid::Uuid::new_v4())
    }

    /// macOS hands out `/var/...` temp dirs that canonicalize to `/private/var`.
    /// The gateway compares canonical paths, so the fixture must too.
    fn project_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().canonicalize().unwrap()
    }

    fn sample_plan() -> Value {
        serde_json::json!({
            "plan": {
                "title": "Rename the config loader",
                "summary": "Move loader.rs into config/ and update imports.",
                "steps": [
                    {"title": "Find every import", "kind": "research"},
                    {"title": "Move the file", "kind": "edit", "targets": ["src/loader.rs"]}
                ]
            }
        })
    }

    #[tokio::test]
    async fn planning_hides_writes_and_offers_only_the_exit() {
        let id = run_id("visible");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);
        let names: Vec<String> = tools
            .list_tool_schemas()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect();

        assert!(names.iter().any(|n| n == "read_file"));
        assert!(names.iter().any(|n| n == "grep"));
        assert!(names.iter().any(|n| n == plan_mode::EXIT_PLAN_MODE_TOOL));
        for hidden in [
            "write_file",
            "edit_file",
            "apply_patch",
            "run_terminal",
            "task",
            "mcp_call",
            plan_mode::ENTER_PLAN_MODE_TOOL,
        ] {
            assert!(
                !names.iter().any(|n| n == hidden),
                "`{hidden}` must not be offered while planning"
            );
        }
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_normal_run_never_sees_the_exit_tool() {
        let id = run_id("normal");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, "ask", &root);
        let names: Vec<String> = tools
            .list_tool_schemas()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert!(names.iter().any(|n| n == "write_file"));
        assert!(names.iter().any(|n| n == plan_mode::ENTER_PLAN_MODE_TOOL));
        assert!(!names.iter().any(|n| n == plan_mode::EXIT_PLAN_MODE_TOOL));
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_blocked_write_is_refused_immediately_not_prompted() {
        let id = run_id("blocked");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);
        let cancel = CancellationToken::new();

        let out = tools
            .execute_tool(
                "write_file",
                serde_json::json!({
                    "path": root.join("x.txt").to_string_lossy(),
                    "content": "nope"
                }),
                &cancel,
            )
            .await;

        assert!(out.is_error);
        assert_eq!(out.output["code"], "plan_mode_blocked");
        assert_eq!(
            tools.interactions.permission_count().await,
            0,
            "Plan Mode must not turn a blocked write into an approval card"
        );
        assert!(!root.join("x.txt").exists());
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn reads_still_work_while_planning() {
        let id = run_id("read");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let file = root.join("notes.txt");
        std::fs::write(&file, "hello plan").unwrap();
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);

        let out = tools
            .execute_tool(
                "read_file",
                serde_json::json!({"path": file.to_string_lossy()}),
                &CancellationToken::new(),
            )
            .await;

        assert!(!out.is_error, "{:?}", out.output);
        assert_eq!(out.output["content"], "hello plan");
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn model_can_enter_plan_mode_on_its_own() {
        let id = run_id("enter");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, "full_access", &root);

        let out = tools
            .execute_tool(
                plan_mode::ENTER_PLAN_MODE_TOOL,
                serde_json::json!({"reason": "wide blast radius"}),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error, "{:?}", out.output);
        assert!(plan_mode::is_active(&id));

        // The autonomous run is now genuinely gated, not just labelled.
        let blocked = tools
            .execute_tool(
                "run_terminal",
                serde_json::json!({"command": "true", "cwd": "."}),
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(blocked.output["code"], "plan_mode_blocked");
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_malformed_plan_never_reaches_the_user() {
        let id = run_id("malformed");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, plan_mode::PLAN_PROFILE, &root);

        let out = tools
            .execute_tool(
                plan_mode::EXIT_PLAN_MODE_TOOL,
                serde_json::json!({"plan": {"title": "", "steps": []}}),
                &CancellationToken::new(),
            )
            .await;

        assert!(out.is_error);
        assert_eq!(out.output["code"], "invalid_plan");
        assert_eq!(tools.interactions.permission_count().await, 0);
        assert!(plan_mode::is_active(&id));
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn exit_outside_plan_mode_is_refused() {
        let id = run_id("outside");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = tools_for(&id, "ask", &root);

        let out = tools
            .execute_tool(
                plan_mode::EXIT_PLAN_MODE_TOOL,
                sample_plan(),
                &CancellationToken::new(),
            )
            .await;
        assert!(out.is_error);
        assert_eq!(out.output["code"], "not_in_plan_mode");
        plan_mode::clear(&id);
    }

    /// Poll the run's event log for the plan approval request and answer it the
    /// way the UI would.
    async fn answer_plan_prompt(tools: &PermissionGatedTools, approved: bool) -> String {
        for _ in 0..200 {
            let pending = tools.events.replay_after(&tools.parent_run_id, 0);
            let found = pending.iter().find_map(|event| match &event.payload {
                RunEventKind::PermissionRequested {
                    tool_name,
                    permission_id,
                    ..
                } if tool_name == plan_mode::EXIT_PLAN_MODE_TOOL => Some(permission_id.clone()),
                _ => None,
            });
            if let Some(permission_id) = found {
                if let Some((_run, _tool, tx)) =
                    tools.interactions.resolve_permission(&permission_id).await
                {
                    let _ = tx.send((approved, "once".into()));
                    return permission_id;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no plan approval prompt was raised");
    }

    #[tokio::test]
    async fn approval_releases_the_latch_and_restores_the_declared_profile() {
        // T01: hermetic. The plan-mode gate persists a pending interaction
        // (interaction_store) and the side-effect ledger, so the fixture needs
        // a temp SQLite store; the process-global RunManager is installed as a
        // MEMORY manager so the verified-project check escapes (no bound run)
        // and checkpointing is in-memory. ~/.natives is never touched.
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        let env_dir = tempfile::tempdir().unwrap();
        let db = env_dir.path().join("plan.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", env_dir.path());
        crate::storage::set_test_db_override(
            Some(db.clone()),
            Some(env_dir.path().join("artifacts")),
        );
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        crate::run_manager::install_memory_global_for_test();
        crate::checkpoint::install_checkpoint_global_for_test(
            crate::checkpoint::CheckpointManager::new(),
        );
        let id = run_id("approve");
        // interaction/session_actor rows reference conversation + run, so the
        // fixture must create the FK stubs before the prompt is raised.
        let conv_id = format!("conv-{id}");
        crate::conversation_store::ensure_conversation_stub(
            &conv_id, "openai", "gpt-4o", None, None,
        )
        .unwrap();
        {
            let store =
                crate::storage::DataStore::new(&db, &env_dir.path().join("artifacts")).unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                     VALUES (?1, ?2, 'created', 'openai', 'gpt-4o')",
                    rusqlite::params![id, conv_id],
                )
                .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = Arc::new(tools_for(&id, "full_access", &root));
        plan_mode::enter(&id, "full_access");
        // T01: the direct tool path bypasses production begin_run; register the
        // run with the installed memory checkpoint manager so capture_before
        // (write_file after approval) finds a live checkpoint.
        crate::checkpoint::global_checkpoint_manager()
            .begin_run(&id, &conv_id, &root)
            .unwrap();

        let submitting = {
            let tools = tools.clone();
            tokio::spawn(async move {
                tools
                    .execute_tool(
                        plan_mode::EXIT_PLAN_MODE_TOOL,
                        sample_plan(),
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        answer_plan_prompt(&tools, true).await;
        let out = submitting.await.unwrap();

        assert!(!out.is_error, "{:?}", out.output);
        assert_eq!(out.output["approved"], true);
        assert_eq!(out.output["permission_profile"], "full_access");
        assert!(!plan_mode::is_active(&id));
        assert_eq!(tools.effective_permission_profile(), "full_access");

        // And the writes it was denied a moment ago now go through.
        let after = tools
            .execute_tool(
                "write_file",
                serde_json::json!({
                    "path": root.join("done.txt").to_string_lossy(),
                    "content": "ok"
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!after.is_error, "{:?}", after.output);
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn rejection_keeps_the_run_planning() {
        // T01: hermetic — same env/DB + memory-global setup as the approval
        // sibling (pending interaction + ledger need a temp store).
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        let env_dir = tempfile::tempdir().unwrap();
        let db = env_dir.path().join("plan-reject.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", env_dir.path());
        crate::storage::set_test_db_override(
            Some(db.clone()),
            Some(env_dir.path().join("artifacts")),
        );
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        crate::run_manager::install_memory_global_for_test();
        crate::checkpoint::install_checkpoint_global_for_test(
            crate::checkpoint::CheckpointManager::new(),
        );
        let id = run_id("reject");
        // interaction/session_actor rows reference conversation + run, so the
        // fixture must create the FK stubs before the prompt is raised.
        let conv_id = format!("conv-{id}");
        crate::conversation_store::ensure_conversation_stub(
            &conv_id, "openai", "gpt-4o", None, None,
        )
        .unwrap();
        {
            let store =
                crate::storage::DataStore::new(&db, &env_dir.path().join("artifacts")).unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                     VALUES (?1, ?2, 'created', 'openai', 'gpt-4o')",
                    rusqlite::params![id, conv_id],
                )
                .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = Arc::new(tools_for(&id, "full_access", &root));
        plan_mode::enter(&id, "full_access");

        let submitting = {
            let tools = tools.clone();
            tokio::spawn(async move {
                tools
                    .execute_tool(
                        plan_mode::EXIT_PLAN_MODE_TOOL,
                        sample_plan(),
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        answer_plan_prompt(&tools, false).await;
        let out = submitting.await.unwrap();

        assert!(
            !out.is_error,
            "a rejection is an answer, not a tool failure: {:?}",
            out.output
        );
        assert_eq!(out.output["approved"], false);
        assert_eq!(out.output["rejections"], 1);
        assert!(plan_mode::is_active(&id), "the latch must stay closed");

        let blocked = tools
            .execute_tool(
                "write_file",
                serde_json::json!({
                    "path": root.join("nope.txt").to_string_lossy(),
                    "content": "x"
                }),
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(blocked.output["code"], "plan_mode_blocked");
        plan_mode::clear(&id);
    }

    #[tokio::test]
    async fn a_cancelled_run_does_not_count_as_approval() {
        let id = run_id("cancel");
        let dir = tempfile::tempdir().unwrap();
        let root = project_root(&dir);
        let tools = Arc::new(tools_for(&id, "full_access", &root));
        plan_mode::enter(&id, "full_access");

        let submitting = {
            let tools = tools.clone();
            tokio::spawn(async move {
                tools
                    .execute_tool(
                        plan_mode::EXIT_PLAN_MODE_TOOL,
                        sample_plan(),
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        // Wait for the waiter to exist, then tear it down the way run cancel does.
        for _ in 0..200 {
            if tools.interactions.permission_count().await > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tools
            .interactions
            .cancel_runs(std::slice::from_ref(&id))
            .await;
        let out = submitting.await.unwrap();

        assert_eq!(out.output["approved"], false);
        assert!(plan_mode::is_active(&id));
        plan_mode::clear(&id);
    }
}
