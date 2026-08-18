//! Permission / grant decisions for the gated tool runtime.

use agent_core::{
    HookEvent, HookRegistry, HookRequest, PermissionAggregate, PermissionProfile,
    ToolExecutionResult, TransitionMetadata,
};
use assistant_protocol::v2::{RunEventKind, RunStatusV2};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::production::normalize_permission_scope;

use super::gated::PermissionGatedTools;
use super::policy::tool_pattern;

/// Scope recorded on the `PermissionResponded` event when a hook, rather than a
/// human, answered the prompt. Deliberately not a grant scope: auto-approval is
/// per invocation and is never remembered.
pub const HOOK_AUTO_APPROVE_SCOPE: &str = "hook_auto_approve";

/// What the permission gate should do after consulting the hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookPermissionGate {
    Deny(String),
    Prompt,
    AutoApprove(String),
}

/// Apply the permission-profile ceiling to a hook aggregate.
///
/// This is the whole of the "a hook cannot escalate" rule, in one pure function
/// so it can be tested without a daemon and so there is exactly one place to
/// audit. Two independent gates must both open before a prompt is skipped:
///
/// - `auto_approve_allowed`, the caller's policy check — the tool must be one
///   the profile would have *asked* about, not one it refuses outright;
/// - the profile must not be `ReadOnly`, which grants nothing that needs asking.
///
/// `Autonomous` is listed for completeness: it already auto-approves downstream,
/// so a hook allow there changes nothing but the audit line.
///
/// Deny is never filtered — a hook may always tighten, never loosen.
pub fn hook_permission_gate(
    aggregate: PermissionAggregate,
    profile: PermissionProfile,
    auto_approve_allowed: bool,
) -> HookPermissionGate {
    match aggregate {
        PermissionAggregate::Deny(reason) => HookPermissionGate::Deny(reason),
        PermissionAggregate::Prompt => HookPermissionGate::Prompt,
        PermissionAggregate::AutoApprove(reason) => {
            if !auto_approve_allowed || profile == PermissionProfile::ReadOnly {
                HookPermissionGate::Prompt
            } else {
                HookPermissionGate::AutoApprove(reason)
            }
        }
    }
}

impl PermissionGatedTools {
    /// Move the owning run through the daemon's sole lifecycle authority.
    /// Unit-level tool fixtures do not always install a RunManager entry, so
    /// preserve their isolated behavior; a real run must journal the wait.
    fn enter_permission_wait(&self) -> Result<(), String> {
        let runs = crate::global_run_manager();
        let Some(run) = runs.get_run(&self.parent_run_id) else {
            return Ok(());
        };
        if run.status.is_terminal() || run.status == RunStatusV2::Cancelling {
            return Ok(());
        }
        match runs.commit_status(
            &self.parent_run_id,
            RunStatusV2::WaitingPermission,
            TransitionMetadata::empty()
                .with_reason("waiting_permission")
                .with_lifecycle_hint("waiting_permission"),
        ) {
            Ok(_) => Ok(()),
            Err(_error)
                if runs.get_run(&self.parent_run_id).is_some_and(|current| {
                    current.status.is_terminal() || current.status == RunStatusV2::Cancelling
                }) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Resume only a still-waiting run. A concurrent cancel/terminal commit
    /// wins and must never be revived by a late permission response.
    fn resume_after_permission(&self) -> Result<(), String> {
        let runs = crate::global_run_manager();
        if !runs
            .get_run(&self.parent_run_id)
            .is_some_and(|run| run.status == RunStatusV2::WaitingPermission)
        {
            return Ok(());
        }
        match runs.commit_status(
            &self.parent_run_id,
            RunStatusV2::Running,
            TransitionMetadata::empty()
                .with_reason("permission_resolved")
                .with_lifecycle_hint("running"),
        ) {
            Ok(_) => Ok(()),
            Err(_error)
                if runs.get_run(&self.parent_run_id).is_some_and(|current| {
                    current.status.is_terminal() || current.status == RunStatusV2::Cancelling
                }) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Run the permission gate for one tool call.
    ///
    /// `None` means "proceed"; `Some` is the denial to return to the model.
    ///
    /// `auto_approve_allowed` is the profile ceiling computed by the caller: it
    /// is the *only* switch that lets a hook's `permissionDecision: "allow"`
    /// skip the prompt. Hooks never widen it.
    pub(crate) async fn await_tool_permission(
        &self,
        tool_call_id: &str,
        name: &str,
        input: &Value,
        auto_approve_allowed: bool,
    ) -> Option<ToolExecutionResult> {
        let pattern = tool_pattern(name, input);
        let inv = self.build_tool_invocation(name, input).await;
        // Skip ask when structured grant already covers this invocation.
        if let Some(rt) = &self.runtime {
            if matches!(
                rt.check_tool_grant_invocation(&inv).await,
                crate::runtime::GrantDecision::Allowed { .. }
            ) {
                return None;
            }
        }
        let project_root = self
            .gateway
            .project_root
            .as_deref()
            .map(std::path::Path::new);
        // NE-P0-05: the permission gate dispatches through the Run's frozen
        // Hook Dispatcher — the same plan the engine was given at Run start —
        // instead of re-scanning hooks.json on every tool call. A mid-Run edit
        // only affects a new Run. Fail-closed aggregation is unchanged: a hook
        // may tighten, never loosen, and a failure still denies.
        let permission_hooks = crate::production_hooks::resolve_frozen_dispatcher(
            &self.parent_run_id,
            self.events.clone(),
            project_root,
        );
        let hook_outcomes = permission_hooks
            .dispatch_outcomes(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: self.parent_run_id.clone(),
                tool_name: Some(name.to_string()),
                input: input.clone(),
            })
            .await;
        // Plan Mode maps to ConfirmEach here: what survives the latch is a read
        // or a network read, and the user is still the one who says yes to it.
        let profile = match self.effective_permission_profile().as_str() {
            "readonly" | "read_only" => PermissionProfile::ReadOnly,
            "full_access" | "autonomous" | "full" => PermissionProfile::Autonomous,
            _ => PermissionProfile::ConfirmEach,
        };
        match hook_permission_gate(
            HookRegistry::aggregate_permission(&hook_outcomes),
            profile,
            auto_approve_allowed,
        ) {
            HookPermissionGate::Deny(reason) => {
                return Some(ToolExecutionResult {
                    output: serde_json::json!({
                        "error": reason,
                        "denied": true,
                        "denied_by_hook": true,
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
            HookPermissionGate::AutoApprove(reason) => {
                // Auto-approval must never be silent: a skipped confirmation
                // that leaves no trace is worse than a prompt. Until the
                // protocol grows a dedicated event, the request/response pair
                // carries the audit — it is the only existing shape that
                // records the tool, its input, and who answered.
                let permission_id = format!("hook-auto-{}", uuid::Uuid::new_v4());
                eprintln!(
                    "[production] hook auto-approved tool `{name}` on run {} ({reason})",
                    self.parent_run_id
                );
                if self
                    .events
                    .append_checked(
                        &self.parent_run_id,
                        RunEventKind::PermissionRequested {
                            tool_call_id: tool_call_id.to_string(),
                            tool_name: name.to_string(),
                            reason: format!("Approve tool `{name}`"),
                            permission_id: permission_id.clone(),
                            input: input.clone(),
                        },
                    )
                    .is_err()
                {
                    return Some(ToolExecutionResult {
                        output: serde_json::json!({"error": "permission event persistence failed"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
                if self
                    .events
                    .append_checked(
                        &self.parent_run_id,
                        RunEventKind::PermissionResponded {
                            permission_id,
                            approved: true,
                            scope: HOOK_AUTO_APPROVE_SCOPE.to_string(),
                        },
                    )
                    .is_err()
                {
                    return Some(ToolExecutionResult {
                        output: serde_json::json!({"error": "permission event persistence failed"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
                return None;
            }
            HookPermissionGate::Prompt => {}
        }

        let permission_id = match self
            .permissions
            .request_permission_for_profile(
                profile,
                &self.parent_run_id,
                tool_call_id,
                name,
                format!("Approve {name}?"),
                input.clone(),
            )
            .await
        {
            Ok(id) => id,
            Err(error) => {
                return Some(ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "PERMISSION_PERSISTENCE_FAILED",
                        "error": format!(
                            "permission request could not be created: {}",
                            error.technical_message
                        ),
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
        };

        if permission_id == "auto-approved" {
            return None;
        }
        // Install the waiter before publishing the event. Otherwise a fast UI
        // (or test responder) can observe PermissionRequested, respond, and
        // lose the race before the channel exists, leaving the engine blocked.
        let (tx, rx) = oneshot::channel::<(bool, String)>();
        self.interactions
            .register_permission(&permission_id, &self.parent_run_id, name, tx)
            .await;
        if let Err(error) = crate::interaction_store::insert_pending(
            &permission_id,
            Some(&self.parent_run_id),
            Some(&self.conversation_id),
            "tool_permission",
            serde_json::json!({
                "tool_call_id": tool_call_id,
                "tool_name": name,
                "reason": format!("Approve tool `{name}`"),
                "input": input,
            }),
        ) {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission interaction could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, Some(permission_id.clone()));
        if let Err(error) = crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id)
        {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "persistence_failed"}),
            );
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission actor snapshot could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        if self
            .events
            .append_checked(
                &self.parent_run_id,
                RunEventKind::PermissionRequested {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: name.to_string(),
                    reason: format!("Approve tool `{name}`"),
                    permission_id: permission_id.clone(),
                    input: input.clone(),
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
            return Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission event persistence failed"}),
                is_error: true,
                duration_ms: 0,
            });
        }
        if let Err(error) = self.enter_permission_wait() {
            let _ = self.interactions.resolve_permission(&permission_id).await;
            let _ = crate::interaction_store::mark_resolved(
                &permission_id,
                serde_json::json!({"approved": false, "scope": "lifecycle_failed"}),
            );
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "RUN_LIFECYCLE_FAILED",
                    "error": format!("permission wait lifecycle could not be committed: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        // Select permission response, timeout, and run cancel token (task-03).
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
                (false, "cancelled".into())
            }
            res = tokio::time::timeout(Duration::from_secs(120), rx) => {
                res.ok().and_then(|r| r.ok()).unwrap_or((false, "once".into()))
            }
        };
        let scope = normalize_permission_scope(&scope);
        // A timeout/cancellation has no UI RPC response to mark the durable
        // interaction complete. Leaving it pending makes reconnect replay an
        // orphaned approval card after its oneshot waiter has gone away.
        if let Err(error) = crate::interaction_store::mark_resolved(
            &permission_id,
            serde_json::json!({ "approved": approved, "scope": scope }),
        ) {
            crate::prompt_queue_store::global_harness()
                .set_pending_interaction(&self.conversation_id, None);
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission response could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        crate::prompt_queue_store::global_harness()
            .set_pending_interaction(&self.conversation_id, None);
        if let Err(error) = crate::prompt_queue_store::persist_actor_snapshot(&self.conversation_id)
        {
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERMISSION_PERSISTENCE_FAILED",
                    "error": format!("permission actor snapshot could not be persisted: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
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
            return Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission event persistence failed"}),
                is_error: true,
                duration_ms: 0,
            });
        }
        if let Err(error) = self.resume_after_permission() {
            return Some(ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "RUN_LIFECYCLE_FAILED",
                    "error": format!("permission resume lifecycle could not be committed: {error}"),
                }),
                is_error: true,
                duration_ms: 0,
            });
        }
        if approved {
            if let Some(rt) = &self.runtime {
                // Structured grant only — empty write_file pattern no longer means any path.
                rt.remember_tool_grant_invocation(&inv, &scope).await;
                let _ = pattern; // kept for legacy audit trails if needed
            }
        }
        if !approved {
            let _ = permission_hooks
                .dispatch(HookRequest {
                    event: HookEvent::PermissionDenied,
                    run_id: self.parent_run_id.clone(),
                    tool_name: Some(name.to_string()),
                    input: input.clone(),
                })
                .await;
        }
        // Phase 2: AfterPermissionResolved is a documented safe point. Message
        // mutation lives in AgentEngine (apply_safe_point); the tool layer cannot
        // push into provider history here. Call the harness so the seam is live,
        // then re-queue any interjection so Engine's AfterTool/ProviderBatch can
        // inject it into messages (on_safe_point consumes pending).
        match crate::prompt_queue_store::on_safe_point_checked(
            &self.conversation_id,
            agent_core::SafePoint::AfterPermissionResolved,
        ) {
            Ok(agent_core::HarnessAction::InjectInterjection { content }) => {
                if let Err(error) = crate::prompt_queue_store::restore_interjection_checked(
                    &self.conversation_id,
                    content,
                ) {
                    return Some(ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "QUEUE_PERSISTENCE_FAILED",
                            "error": format!("safe-point interjection restore failed: {error}"),
                        }),
                        is_error: true,
                        duration_ms: 0,
                    });
                }
            }
            Ok(_) => {}
            Err(error) => {
                return Some(ToolExecutionResult {
                    output: serde_json::json!({
                        "error_code": "QUEUE_PERSISTENCE_FAILED",
                        "error": format!("safe-point state could not be persisted: {error}"),
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
        }
        if approved {
            None
        } else {
            Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission denied", "denied": true}),
                is_error: true,
                duration_ms: 0,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{HookDecision, HookHandler, HookOutcome, HookResponse, PermissionVerdict};

    /// A hook that returns one fixed outcome, so the permission gate can be
    /// exercised without spawning processes.
    struct FixedHook(fn() -> HookOutcome);

    #[async_trait::async_trait]
    impl HookHandler for FixedHook {
        async fn handle(&self, _request: HookRequest) -> HookResponse {
            (self.0)().into_response()
        }

        async fn handle_outcome(&self, _request: HookRequest) -> HookOutcome {
            (self.0)()
        }
    }

    fn allowing() -> HookOutcome {
        HookOutcome::Permission(PermissionVerdict::Allow {
            reason: "hook approved".into(),
        })
    }

    fn denying() -> HookOutcome {
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny {
                reason: "hook denied".into(),
            },
        })
    }

    fn failing() -> HookOutcome {
        HookOutcome::Failed {
            reason: "hook crashed".into(),
        }
    }

    fn asking() -> HookOutcome {
        HookOutcome::Permission(PermissionVerdict::Ask {
            reason: "human please".into(),
        })
    }

    async fn gate_through_registry(
        outcomes: &[fn() -> HookOutcome],
        profile: PermissionProfile,
        auto_approve_allowed: bool,
    ) -> HookPermissionGate {
        let mut registry = HookRegistry::new();
        registry.enable_security_fail_closed();
        for outcome in outcomes {
            registry.register(HookEvent::PermissionRequest, Box::new(FixedHook(*outcome)));
        }
        let dispatched = registry
            .dispatch_outcomes(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: "run-1".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({ "path": "a.txt" }),
            })
            .await;
        hook_permission_gate(
            HookRegistry::aggregate_permission(&dispatched),
            profile,
            auto_approve_allowed,
        )
    }

    #[tokio::test]
    async fn hook_allow_skips_the_prompt() {
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::ConfirmEach, true).await,
            HookPermissionGate::AutoApprove("hook approved".into())
        );
    }

    /// The core fail-closed property, checked in both dispatch orders so the
    /// answer cannot depend on which hook happens to run first.
    #[tokio::test]
    async fn deny_wins_over_allow() {
        for order in [
            [allowing as fn() -> HookOutcome, denying],
            [denying, allowing],
        ] {
            assert_eq!(
                gate_through_registry(&order, PermissionProfile::ConfirmEach, true).await,
                HookPermissionGate::Deny("hook denied".into()),
                "a deny must survive any ordering"
            );
        }
    }

    /// A hook that could not run is treated as a deny on this security event,
    /// even next to a hook that approved.
    #[tokio::test]
    async fn hook_failure_is_fail_closed() {
        let gate =
            gate_through_registry(&[allowing, failing], PermissionProfile::ConfirmEach, true).await;
        match gate {
            HookPermissionGate::Deny(reason) => assert!(reason.contains("hook crashed")),
            other => panic!("a failed security hook must deny, got {other:?}"),
        }
    }

    /// The ceiling: a readonly session cannot be talked into skipping its
    /// confirmation by a hook.
    #[tokio::test]
    async fn readonly_profile_ignores_hook_allow() {
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::ReadOnly, true).await,
            HookPermissionGate::Prompt
        );
    }

    /// The other half of the ceiling: when the policy refused rather than
    /// asked, the caller withholds `auto_approve_allowed` and the hook's allow
    /// buys nothing.
    #[tokio::test]
    async fn policy_refusal_ignores_hook_allow() {
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::ConfirmEach, false).await,
            HookPermissionGate::Prompt
        );
        assert_eq!(
            gate_through_registry(&[allowing], PermissionProfile::Autonomous, false).await,
            HookPermissionGate::Prompt
        );
    }

    /// A deny is never filtered by the ceiling — hooks may always tighten.
    #[test]
    fn deny_passes_every_ceiling() {
        for profile in [
            PermissionProfile::ReadOnly,
            PermissionProfile::ConfirmEach,
            PermissionProfile::Autonomous,
        ] {
            for allowed in [false, true] {
                assert_eq!(
                    hook_permission_gate(PermissionAggregate::Deny("no".into()), profile, allowed),
                    HookPermissionGate::Deny("no".into())
                );
            }
        }
    }

    /// Silence from the hooks leaves the pre-existing prompt behaviour intact.
    #[test]
    fn no_hook_opinion_still_prompts() {
        assert_eq!(
            hook_permission_gate(
                PermissionAggregate::Prompt,
                PermissionProfile::ConfirmEach,
                true
            ),
            HookPermissionGate::Prompt
        );
    }

    /// An explicit `ask` from any hook forces the prompt back on.
    #[tokio::test]
    async fn hook_ask_overrides_hook_allow() {
        assert_eq!(
            gate_through_registry(&[allowing, asking], PermissionProfile::ConfirmEach, true).await,
            HookPermissionGate::Prompt
        );
    }

    /// NE-P0-05 §19.5: the permission gate dispatches through the Run's *frozen*
    /// Hook Dispatcher — the same shared read-only plan the notification hook
    /// and the subagent lifecycle use — not a freshly-compiled registry on every
    /// tool call. Two resolves for the same run return the same dispatcher and
    /// a mid-Run `hooks.json` edit cannot replace it. This is the guarantee the
    /// permission gate (`await_tool_permission`) relies on, checked here from
    /// the same seam the gate calls.
    #[tokio::test]
    async fn permission_gate_uses_frozen_dispatcher_shared_and_stable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().unwrap();
        const PROBE_URL: &str = "http://127.0.0.1:1/hook";
        std::fs::create_dir_all(root.join(".natives")).expect("create .natives dir");
        std::fs::write(
            root.join(".natives").join("hooks.json"),
            format!(r#"{{"hooks":{{"PermissionRequest":[{{"hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}]}}}}"#),
        )
        .expect("write hooks.json");
        let run_id = format!("perm-frozen-{}", uuid::Uuid::new_v4());
        let events = agent_core::EventSequencer::memory_only();
        let project = Some(root.as_path());

        let first =
            crate::production_hooks::resolve_frozen_dispatcher(&run_id, events.clone(), project);
        let plan_hash = first.plan_hash().to_string();

        // Second resolve for the same run must return the same frozen dispatcher.
        let second =
            crate::production_hooks::resolve_frozen_dispatcher(&run_id, events.clone(), project);
        assert_eq!(
            second.plan_hash(),
            plan_hash,
            "two resolves for the same run must agree on the plan hash"
        );

        // Mid-run edit: the frozen plan must not change.
        std::fs::write(
            root.join(".natives").join("hooks.json"),
            format!(
                r#"{{"hooks":{{"PermissionRequest":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        )
        .expect("overwrite hooks.json");
        let third = crate::production_hooks::resolve_frozen_dispatcher(&run_id, events, project);
        assert_eq!(
            third.plan_hash(),
            plan_hash,
            "a mid-Run hooks.json edit must not change the permission gate's frozen plan hash"
        );
    }
}
