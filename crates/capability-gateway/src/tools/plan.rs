//! Plan Mode control tools.
//!
//! `enter_plan_mode` is self-contained: closing the latch only removes
//! capability, so the gateway can do it without asking anyone.
//!
//! `exit_plan_mode` is not. Leaving Plan Mode requires a human, and the only
//! component that can reach a human is the Agent Daemon's interaction hub. So
//! this registration carries the schema and refuses a bare execute, exactly like
//! the `task` / `skill` orchestration tools do. A handler that "succeeded" here
//! would be a tool that lets the model approve its own plan.

use crate::plan_mode;
use crate::{
    PathScope, PermissionClass, SideEffect, Tool, ToolCallContext, ToolError, ToolHandler,
    ToolOutput,
};
use std::sync::Arc;

pub struct EnterPlanModeTool;

#[async_trait::async_trait]
impl ToolHandler for EnterPlanModeTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        if context.run_id.trim().is_empty() {
            return Err(ToolError {
                code: "invalid_context".into(),
                message: "plan mode requires a run id".into(),
                retryable: false,
            });
        }
        let already = plan_mode::is_active(&context.run_id);
        let session = plan_mode::enter(&context.run_id, &context.permission_profile);
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        Ok(ToolOutput {
            result: serde_json::json!({
                "plan_mode": session.state,
                "already_active": already,
                "reason": reason,
                "fallback_profile": session.fallback_profile,
                "message": format!(
                    "Plan Mode is active. Read, search and fetch freely; every write, \
                     command and subagent call is blocked. When the plan is ready call \
                     `{}` — only the user can approve it.",
                    plan_mode::EXIT_PLAN_MODE_TOOL
                ),
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

pub struct ExitPlanModeTool;

#[async_trait::async_trait]
impl ToolHandler for ExitPlanModeTool {
    async fn execute(
        &self,
        _input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError {
            code: "orchestrator_required".into(),
            message: format!(
                "`{}` requires the Agent Daemon: leaving Plan Mode is a user action and \
                 must go through a real approval interaction",
                plan_mode::EXIT_PLAN_MODE_TOOL
            ),
            retryable: false,
        })
    }
}

/// JSON Schema for the plan payload, shared by the tool schema and the
/// approval card the GUI renders from it.
fn plan_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": "The plan the user will approve or reject.",
        "properties": {
            "title": {"type": "string", "description": "One line naming the outcome"},
            "summary": {"type": "string", "description": "Two or three sentences on the approach"},
            "steps": {
                "type": "array",
                "minItems": 1,
                "maxItems": plan_mode::MAX_PLAN_STEPS,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "Stable id; assigned as s1..sN when omitted"},
                        "title": {"type": "string", "description": "What this step does"},
                        "detail": {"type": "string", "description": "How, if it is not obvious from the title"},
                        "kind": {
                            "type": "string",
                            "enum": ["research", "edit", "command", "verify"],
                            "description": "research reads, edit changes files, command runs a process, verify checks the result"
                        },
                        "targets": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Files, directories or commands this step touches"
                        },
                        "risk": {"type": "string", "enum": ["low", "medium", "high"]},
                        "reversible": {
                            "type": "boolean",
                            "description": "Whether a checkpoint restore can undo this step. Say false when unsure."
                        }
                    },
                    "required": ["title", "kind"]
                }
            },
            "risks": {"type": "array", "items": {"type": "string"}, "description": "What could go wrong"},
            "open_questions": {"type": "array", "items": {"type": "string"}, "description": "Decisions you need from the user"},
            "out_of_scope": {"type": "array", "items": {"type": "string"}, "description": "Explicit non-goals"}
        },
        "required": ["title", "steps"]
    })
}

pub fn plan_mode_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: plan_mode::ENTER_PLAN_MODE_TOOL,
            description: "Switch this run into Plan Mode before doing anything risky or wide-reaching. \
                          Reading, searching and fetching keep working; writes, commands, subagents and \
                          MCP calls are refused until the user approves a plan. Entering costs nothing \
                          and can only reduce what you are allowed to do.",
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "reason": {"type": "string", "description": "Why this task warrants a plan first"}
                },
                "required": []
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 5_000,
            output_limit: 16_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(EnterPlanModeTool),
        },
        Tool {
            name: plan_mode::EXIT_PLAN_MODE_TOOL,
            description: "Submit your plan for the user to approve. Call this only when research is \
                          finished. The user sees your steps as a checklist and decides; you do not. \
                          If they reject it, you stay in Plan Mode — revise and submit again. Never \
                          state or imply that a plan is approved: approval only exists when this tool \
                          returns approved=true.",
            schema: serde_json::json!({
                "type": "object",
                "properties": {"plan": plan_schema()},
                "required": ["plan"]
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            // Waiting on a human: matches the daemon's 120s permission timeout.
            timeout_ms: 130_000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(ExitPlanModeTool),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(run_id: &str, profile: &str) -> ToolCallContext {
        ToolCallContext::new(
            std::env::temp_dir(),
            run_id.to_string(),
            "conv-plan".into(),
            "tc-plan".into(),
            profile.to_string(),
        )
    }

    #[tokio::test]
    async fn enter_closes_the_latch_and_captures_the_fallback() {
        let run_id = format!("enter-{}", uuid::Uuid::new_v4());
        let out = EnterPlanModeTool
            .execute(
                serde_json::json!({"reason": "touches the migration"}),
                &ctx(&run_id, "autonomous"),
            )
            .await
            .unwrap();
        assert_eq!(out.result["already_active"], false);
        assert_eq!(out.result["fallback_profile"], "autonomous");
        assert!(plan_mode::is_active(&run_id));

        // Second call is idempotent, not a reset.
        let again = EnterPlanModeTool
            .execute(serde_json::json!({}), &ctx(&run_id, "readonly"))
            .await
            .unwrap();
        assert_eq!(again.result["already_active"], true);
        assert_eq!(again.result["fallback_profile"], "autonomous");
        plan_mode::clear(&run_id);
    }

    #[tokio::test]
    async fn exit_cannot_be_self_served_by_the_model() {
        let run_id = format!("exit-{}", uuid::Uuid::new_v4());
        plan_mode::enter(&run_id, "ask");
        let err = ExitPlanModeTool
            .execute(
                serde_json::json!({"plan": {"title": "t", "steps": [{"title": "s", "kind": "edit"}]}}),
                &ctx(&run_id, "ask"),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "orchestrator_required");
        assert!(
            plan_mode::is_active(&run_id),
            "a bare exit_plan_mode must not release the latch"
        );
        plan_mode::clear(&run_id);
    }

    #[test]
    fn both_control_tools_are_registered_with_no_side_effects() {
        let tools = plan_mode_tools();
        assert_eq!(tools.len(), 2);
        for tool in &tools {
            assert_eq!(tool.side_effect, SideEffect::ReadOnly, "{}", tool.name);
            assert!(matches!(tool.path_scope, PathScope::None), "{}", tool.name);
        }
    }
}
