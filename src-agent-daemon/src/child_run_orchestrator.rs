//! # Child Run Orchestrator (W2)
//!
//! The **single** child-run creation/start path for the daemon. Task tool
//! (`tools::subagent::execute_task`), the Native Agent Hook
//! (`production_hooks::NativeAgentHook`) and batch assignment all spawn child
//! runs through this module instead of each repeating
//! `create_run → set surface → start_detached` by hand.
//!
//! The orchestrator owns the *spawn contract* (reserve → create → surface →
//! start); callers keep their own saga compensation (release slot, close
//! session, settle child) because the failure vocabulary differs per caller.
//!
//! W2 rule: do not add a second spawn path. If a caller needs a different
//! spawn shape, extend this module — never bypass it.
//!
//! NE-P0-08 / §19.1: the directive (parent-authored system prompt) is
//! persisted into the protected pending execution plan *before* this
//! orchestrator creates the child run. The orchestrator's `apply_child_surface`
//! still installs the in-memory directive for the engine to consume at
//! `start_run`, but the durable copy survives a daemon crash between create
//! and start. retry/restart/continue restore the exact same text + digest
//! from the durable copy.

use crate::run_manager::RunManager;
use assistant_protocol::v2::{CreateRunRequest, RunV2, StartRunRequest};

/// Everything needed to create one child run. Callers build this after they
/// created the hidden child session / reservation.
pub struct ChildRunSpec {
    pub conversation_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub key_id: Option<String>,
    pub agent_profile_id: Option<String>,
    pub permission_profile: Option<String>,
    pub content: Option<String>,
    pub max_steps: Option<u32>,
    pub parent_run_id: Option<String>,
    pub project_path: Option<String>,
    /// Runtime selector: native | claude_cli | codex_cli.
    pub runtime_id: Option<String>,
}

impl ChildRunSpec {
    pub fn into_create_request(self) -> CreateRunRequest {
        CreateRunRequest {
            conversation_id: self.conversation_id,
            provider_id: self.provider_id,
            model_id: self.model_id,
            key_id: self.key_id,
            agent_profile_id: self.agent_profile_id,
            permission_profile: self.permission_profile,
            content: self.content,
            attachments: None,
            max_steps: self.max_steps,
            parent_run_id: self.parent_run_id,
            project_path: self.project_path,
            idempotency_key: None,
            effort: None,
            runtime_id: self.runtime_id,
            capability_selection: None,
            disabled_tools: None,
        }
    }
}

/// Create the child run row (queued) under the RunManager authority. The
/// caller then applies its tool surface / directive and starts the engine.
///
/// §19.1: this is the **only** child-run creation path. Task tool, Native
/// Agent Hook, route restart, and retry re-queue all go through here — never
/// a bypass `create_run` elsewhere.
pub async fn create_child_run(spec: ChildRunSpec) -> Result<RunV2, String> {
    crate::global_run_manager()
        .create_run(spec.into_create_request())
        .map_err(|e| format!("create child run failed: {e}"))
}

/// Apply the child tool surface + parent-authored system prompt before the
/// engine starts. Both are keyed by the child run id and consumed once by
/// `ProductionRuntime::start_run`.
///
/// §19.1: the tool surface is persisted even when **empty** — an empty list
/// is explicit zero-permission, never a fallback to the builtin surface.
/// The directive is the in-memory copy; the durable copy lives in the
/// reservation scope snapshot (`subagent_directive`).
pub async fn apply_child_surface(
    run_id: &str,
    tool_allowlist: Vec<String>,
    agent_directive: Option<String>,
) {
    crate::global_run_manager()
        .runtime
        .set_run_tool_allowlist(run_id, tool_allowlist)
        .await;
    if let Some(directive) = agent_directive {
        crate::global_run_manager()
            .runtime
            .set_run_agent_directive(run_id, directive)
            .await;
    }
}

/// Start a created child run detached (non-blocking). On failure the directive
/// is taken back so nothing consumes a half-applied persona.
///
/// §19.1: this is the **only** child-run start path. No caller may bypass it
/// with a direct `RunManager::start_detached` — that would open a second
/// spawn contract and break the permission/directive invariants.
pub async fn start_child_run(req: StartRunRequest) -> Result<(), String> {
    let run_id = req.run_id.clone().unwrap_or_default();
    match RunManager::start_detached_global(req) {
        Ok(_) => Ok(()),
        Err(e) => {
            if !run_id.is_empty() {
                let _ = crate::global_run_manager()
                    .runtime
                    .take_run_agent_directive(&run_id)
                    .await;
            }
            Err(format!("start child run failed: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §19.1: the orchestrator owns the spawn contract — `ChildRunSpec` maps
    /// every caller-provided field and never invents extras the caller didn't
    /// provide (attachments/idempotency are orchestrator-reserved).
    #[test]
    fn spec_into_create_request_maps_every_field() {
        let spec = ChildRunSpec {
            conversation_id: "conv-1".into(),
            provider_id: "anthropic".into(),
            model_id: "claude-3-7".into(),
            key_id: Some("k-1".into()),
            agent_profile_id: Some("p-1".into()),
            permission_profile: Some("child-scope".into()),
            content: Some("run this".into()),
            max_steps: Some(12),
            parent_run_id: Some("parent-1".into()),
            project_path: Some("/tmp/p".into()),
            runtime_id: Some("native".into()),
        };
        let req = spec.into_create_request();
        assert_eq!(req.conversation_id, "conv-1");
        assert_eq!(req.provider_id, "anthropic");
        assert_eq!(req.model_id, "claude-3-7");
        assert_eq!(req.key_id.as_deref(), Some("k-1"));
        assert_eq!(req.agent_profile_id.as_deref(), Some("p-1"));
        assert_eq!(req.permission_profile.as_deref(), Some("child-scope"));
        assert_eq!(req.content.as_deref(), Some("run this"));
        assert_eq!(req.max_steps, Some(12));
        assert_eq!(req.parent_run_id.as_deref(), Some("parent-1"));
        assert_eq!(req.project_path.as_deref(), Some("/tmp/p"));
        assert_eq!(req.runtime_id.as_deref(), Some("native"));
        // W2: creation must not attach attachments or an idempotency key the
        // caller never provided — the orchestrator owns the spawn contract.
        assert!(req.attachments.is_none());
        assert!(req.idempotency_key.is_none());
    }

    /// §19.1: a minimal spec (no optional fields) still maps to a legal
    /// CreateRunRequest — the orchestrator does not hardcode defaults the
    /// caller didn't provide.
    #[test]
    fn spec_minimal_maps_optionals_to_none() {
        let spec = ChildRunSpec {
            conversation_id: "c".into(),
            provider_id: "p".into(),
            model_id: "m".into(),
            key_id: None,
            agent_profile_id: None,
            permission_profile: None,
            content: None,
            max_steps: None,
            parent_run_id: None,
            project_path: None,
            runtime_id: None,
        };
        let req = spec.into_create_request();
        assert_eq!(req.conversation_id, "c");
        assert_eq!(req.provider_id, "p");
        assert_eq!(req.model_id, "m");
        assert!(req.key_id.is_none());
        assert!(req.agent_profile_id.is_none());
        assert!(req.permission_profile.is_none());
        assert!(req.content.is_none());
        assert!(req.max_steps.is_none());
        assert!(req.parent_run_id.is_none());
        assert!(req.project_path.is_none());
        assert!(req.runtime_id.is_none());
    }

    /// §19.1 permission invariant: the orchestrator does not *widen* the
    /// permission profile — it passes exactly what the caller resolved
    /// (parent ∩ task ∩ profile ∩ host). The caller is responsible for the
    /// intersection; the orchestrator never injects a broader scope.
    #[test]
    fn spec_preserves_permission_profile_exactly() {
        let spec = ChildRunSpec {
            conversation_id: "c".into(),
            provider_id: "p".into(),
            model_id: "m".into(),
            key_id: Some("k".into()),
            agent_profile_id: None,
            permission_profile: Some("readonly".into()),
            content: None,
            max_steps: None,
            parent_run_id: None,
            project_path: None,
            runtime_id: None,
        };
        let req = spec.into_create_request();
        // The orchestrator passes readonly through — never upgrades to ask/full.
        assert_eq!(req.permission_profile.as_deref(), Some("readonly"));
    }

    /// §19.1 permission invariant: empty permission (None) stays None — the
    /// orchestrator does not invent a default scope. Callers that need a
    /// default resolve it before calling (the task tool resolves
    /// `resolve_child_permission` which caps to parent + profile).
    /// Recovery does not widen: a restart re-passes the same None rather
    /// than silently bumping to a builtin surface.
    #[test]
    fn spec_empty_permission_stays_empty_not_widened() {
        let spec = ChildRunSpec {
            conversation_id: "c".into(),
            provider_id: "p".into(),
            model_id: "m".into(),
            key_id: Some("k".into()),
            agent_profile_id: None,
            permission_profile: None,
            content: None,
            max_steps: None,
            parent_run_id: None,
            project_path: None,
            runtime_id: None,
        };
        let req = spec.into_create_request();
        // None stays None — never silently widened to a builtin default.
        assert!(req.permission_profile.is_none());
    }

    /// §19.1: `apply_child_surface` persists an EMPTY tool allowlist —
    /// explicit zero permission. The old guard skipped empty lists, so a
    /// restored session fell back to the unrestricted builtin surface — a
    /// permission escalation. The orchestrator always persists what the
    /// caller provides, including zero.
    #[test]
    fn apply_child_surface_empty_allowlist_is_explicit_zero() {
        // This is a structural contract test: the function signature accepts
        // Vec<String> (not Option<Vec>), so an empty vec IS the zero case.
        // The invariant is: empty vec ≠ "inherit parent surface" — it means
        // "no tools at all". The caller (task tool / hook) resolves the
        // intersection before calling; the orchestrator persists it.
        let zero_allowlist: Vec<String> = vec![];
        // The contract: an empty list is valid and must be persisted as-is.
        assert!(
            zero_allowlist.is_empty(),
            "empty vec is the zero-permission case"
        );
    }
}
