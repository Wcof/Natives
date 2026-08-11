//! Model-visible schema/capability listing for `PermissionGatedTools`.
//!
//! Inherent helpers backing the `EngineToolRuntime` impl that must stay in a
//! single block (Rust forbids splitting one trait impl across files, E0119).
//! Split out of `gated_execute` so each file stays under 1000 lines (W10).

use super::*;

impl PermissionGatedTools {
    pub(crate) fn list_tool_schemas_impl(&self) -> Vec<ToolSchema> {
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

    pub(crate) fn list_tool_capabilities_impl(&self) -> Vec<agent_core::ToolCapability> {
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
