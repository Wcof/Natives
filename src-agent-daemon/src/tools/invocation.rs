//! Invocation / context construction for the gated tool runtime.

use agent_core::ToolSchema;
use capability_gateway::plan_mode::{self, PlanDecision};
use capability_gateway::CapabilityGateway;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::gated::PermissionGatedTools;
use super::policy::mcp_server_of_tool;

/// Maximum allowed model-visible tools for a single Run.
/// If count exceeds 200, the Run fails closed before Provider invocation with tool_plan_too_large.
pub const MAX_MODEL_VISIBLE_TOOLS: usize = 200;

pub fn validate_tool_limit(count: usize) -> Result<(), String> {
    if count > MAX_MODEL_VISIBLE_TOOLS {
        Err(format!(
            "tool_plan_too_large: Model-visible tools count ({count}) exceeds maximum limit of {MAX_MODEL_VISIBLE_TOOLS}"
        ))
    } else {
        Ok(())
    }
}

pub fn model_visible_tool_schemas(
    gateway: &CapabilityGateway,
    tool_allowlist: Option<&[String]>,
    mcp_tool_schemas: &[ToolSchema],
    selected_mcp_servers: Option<&std::collections::HashSet<String>>,
    planning: bool,
) -> Vec<ToolSchema> {
    let allowed = |name: &str| {
        if let (Some(selected), Some(server)) = (selected_mcp_servers, mcp_server_of_tool(name)) {
            if !selected.contains(server) {
                return false;
            }
        }
        tool_allowlist.is_none_or(|list| agent_core::tool_list_allows(list, name))
    };
    let visible = |tool: &capability_gateway::Tool| match plan_mode::decision(
        tool.name,
        tool.side_effect,
        tool.permission_class,
    ) {
        PlanDecision::Control if tool.name == plan_mode::EXIT_PLAN_MODE_TOOL => planning,
        PlanDecision::Control => !planning,
        PlanDecision::Deny => !planning,
        PlanDecision::Allow => true,
    };
    let mut schemas: Vec<ToolSchema> = gateway
        .list_tools()
        .into_iter()
        .filter(|tool| allowed(tool.name) && visible(tool))
        .map(|tool| ToolSchema {
            name: tool.name.to_string(),
            description: tool.description.to_string(),
            input_schema: tool.schema.clone(),
        })
        .collect();
    for schema in mcp_tool_schemas {
        if planning
            || !allowed(&schema.name)
            || schemas.iter().any(|existing| existing.name == schema.name)
        {
            continue;
        }
        schemas.push(schema.clone());
    }
    schemas
}

impl PermissionGatedTools {
    /// Resolve verified ProjectIdentity for the parent run (None if unbound/orphan).
    pub(crate) async fn verified_project_identity(
        &self,
    ) -> Option<crate::project_identity::ProjectIdentity> {
        let run = crate::run_manager::global_run_manager().get_run(&self.parent_run_id)?;
        let project_id = run.project_id.as_deref()?;
        // Prefer daemon DataStore used by RunManager (same assistant.db as create_run).
        let store = crate::run_manager::global_run_manager().data_store_ref()?;
        let conn = store.conn().ok()?;
        crate::project_identity::store::verify_for_invocation(&conn, project_id).ok()
    }

    pub(crate) async fn ensure_verified_project_for_tool(&self, name: &str) -> Result<(), String> {
        if !crate::runtime::tool_requires_verified_project(name) {
            return Ok(());
        }
        // Production: require verified ProjectIdentity when RunManager has a DataStore.
        // Fixture/unit tests (no data_store on global RunManager) keep gateway root as
        // workspace bound and are not fail-closed here — host create_run always binds
        // identity when a project path is provided.
        let has_store = crate::run_manager::global_run_manager()
            .data_store_ref()
            .is_some();
        if !has_store {
            return Ok(());
        }
        match self.verified_project_identity().await {
            Some(_) => Ok(()),
            None => Err(format!(
                "tool `{name}` requires a verified ProjectIdentity on the run (unbound/orphaned/fingerprint mismatch)"
            )),
        }
    }

    pub(crate) async fn build_tool_invocation(
        &self,
        name: &str,
        input: &Value,
    ) -> crate::runtime::ToolInvocation {
        if let Some(identity) = self.verified_project_identity().await {
            return crate::runtime::invocation_from_verified_identity(
                name,
                input,
                &self.conversation_id,
                &self.parent_run_id,
                Some(&self.conversation_id),
                &identity,
            );
        }
        crate::runtime::invocation_from_gate(
            name,
            input,
            &self.conversation_id,
            &self.parent_run_id,
            self.gateway.project_root.as_deref(),
        )
    }

    pub(crate) async fn build_tool_call_context(
        &self,
        tool_call_id: String,
        cancel: CancellationToken,
        progress: Option<tokio::sync::mpsc::Sender<capability_gateway::ToolProgressChunk>>,
        progress_dropped_bytes: Arc<std::sync::atomic::AtomicU64>,
        turn_id: Option<String>,
        message_id: Option<String>,
    ) -> capability_gateway::ToolCallContext {
        if let Some(identity) = self.verified_project_identity().await {
            let mut context =
                capability_gateway::ToolCallContext::from_verified_identity_with_cancel(
                    identity.project_id.clone(),
                    identity.identity_version,
                    std::path::PathBuf::from(&identity.canonical_path),
                    self.parent_run_id.clone(),
                    self.conversation_id.clone(),
                    tool_call_id,
                    self.permission_profile.clone(),
                    cancel,
                );
            context.set_progress(progress, progress_dropped_bytes);
            context.turn_id = turn_id;
            context.message_id = message_id;
            return context;
        }
        let project_root = self
            .gateway
            .project_root
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let mut context = capability_gateway::ToolCallContext::with_cancel(
            project_root,
            self.parent_run_id.clone(),
            self.conversation_id.clone(),
            tool_call_id,
            self.permission_profile.clone(),
            cancel,
        );
        context.set_progress(progress, progress_dropped_bytes);
        context.turn_id = turn_id;
        context.message_id = message_id;
        context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_visible_tool_limit_fails_closed_without_truncation() {
        assert!(validate_tool_limit(MAX_MODEL_VISIBLE_TOOLS).is_ok());
        let error = validate_tool_limit(MAX_MODEL_VISIBLE_TOOLS + 1).unwrap_err();
        assert!(error.contains("tool_plan_too_large"));
    }
}
