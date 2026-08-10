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
pub async fn create_child_run(spec: ChildRunSpec) -> Result<RunV2, String> {
    crate::global_run_manager()
        .create_run(spec.into_create_request())
        .map_err(|e| format!("create child run failed: {e}"))
}

/// Apply the child tool surface + parent-authored system prompt before the
/// engine starts. Both are keyed by the child run id and consumed once by
/// `ProductionRuntime::start_run`.
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

/// Reserve → create → surface → start a child run in one call. For callers
/// without an intermediate saga step (e.g. the Native Agent Hook).
pub async fn spawn_child_run(
    spec: ChildRunSpec,
    tool_allowlist: Vec<String>,
    agent_directive: Option<String>,
    start: StartRunRequest,
) -> Result<RunV2, String> {
    let created = create_child_run(spec).await?;
    let run_id = created.id.clone();
    apply_child_surface(&run_id, tool_allowlist, agent_directive).await;
    let mut start = start;
    start.run_id = Some(run_id.clone());
    start.conversation_id = start
        .conversation_id
        .or(Some(created.conversation_id.clone()));
    start.provider_id = start.provider_id.or(Some(created.provider_id.clone()));
    start.model_id = start.model_id.or(Some(created.model_id.clone()));
    start.key_id = start.key_id.or(created.key_id.clone());
    start.permission_profile = start
        .permission_profile
        .or(Some(created.permission_profile.clone()));
    start.max_steps = start.max_steps.or(Some(created.max_steps));
    start.project_path = start.project_path.or(created.project_path.clone());
    start.runtime_id = start.runtime_id.or(created.runtime_id.clone());
    start_child_run(start).await?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
