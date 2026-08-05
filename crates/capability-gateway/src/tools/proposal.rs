//! Agent creative proposal tool (batch 10 CR-1001).
//!
//! The model proposes creating/starting a creative app. This tool accepts a
//! versioned [`CreativeProposalPayload`] and returns it for the user to approve.
//! The real security boundary is the Host gate
//! (`creative_app::proposal::validate_protocol_proposal`), which rejects cwd
//! escape, privileged Compose, secret-like env keys, and arbitrary binaries.
//! The tool itself performs only structural validation so the model cannot
//! submit a malformed proposal; it never registers an Application.

use crate::tools::creative_draft::{invalid_input, str_arg};
use crate::{
    PathScope, PermissionClass, SideEffect, Tool, ToolCallContext, ToolError, ToolHandler,
    ToolOutput,
};
use assistant_protocol::v2::{CreativeProposalPayload, CreativeProposedDriver};
use std::sync::Arc;

/// The proposal tool. Registered in [`super::builtin_tools`].
pub fn creative_proposal_tool() -> Tool {
    Tool {
        name: "creative_proposal",
        description: "Propose creating or starting a creative app. The proposal is validated by the Host and must be approved by the user before anything is registered or started. Never bypasses user approval.",
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "schemaVersion": {"type": "integer", "const": 1, "description": "Proposal schema version (must be 1)"},
                "kind": {"type": "string", "enum": ["create", "start"]},
                "ownership": {"type": "string", "enum": ["managed", "attached", "remote"]},
                "title": {"type": "string", "minLength": 1},
                "projectRoot": {"type": "string", "description": "Absolute project root"},
                "driver": {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": ["python", "binary", "staticHttp", "compose"]}
                    },
                    "required": ["kind"]
                },
                "openPath": {"type": "string"},
                "healthPath": {"type": "string"},
                "environmentKeys": {"type": "array", "items": {"type": "string"}, "description": "Env KEY names only, never values"}
            },
            "required": ["schemaVersion", "kind", "ownership", "title", "projectRoot", "driver", "openPath", "healthPath", "environmentKeys"]
        }),
        side_effect: SideEffect::Write,
        path_scope: PathScope::None,
        // The tool only records a proposal; user approval gates registration.
        // Keep it AlwaysAllowed so the generate loop is not a prompt storm, but
        // note the Host gate and Renderer approval card are the real boundary.
        permission_class: PermissionClass::AlwaysAllowed,
        timeout_ms: 10_000,
        output_limit: 16_000,
        cancellable: true,
        parallel_safe: false,
        conflict_key: None,
        handler: Arc::new(CreativeProposalTool),
    }
}

/// Structural validation + passthrough of a proposal.
pub struct CreativeProposalTool;

#[async_trait::async_trait]
impl ToolHandler for CreativeProposalTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let schema_version = input
            .get("schemaVersion")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| invalid_input("schemaVersion must be an integer"))?;
        if schema_version != 1 {
            return Err(invalid_input(format!(
                "unsupported proposal schema version {schema_version}"
            )));
        }
        let kind = str_arg(&input, "kind")?;
        if kind != "create" && kind != "start" {
            return Err(invalid_input("kind must be create or start"));
        }
        let ownership = str_arg(&input, "ownership")?;
        if ownership != "managed" && ownership != "attached" && ownership != "remote" {
            return Err(invalid_input("ownership must be managed, attached, or remote"));
        }
        let title = str_arg(&input, "title")?.trim().to_string();
        if title.is_empty() || title.chars().count() > 200 {
            return Err(invalid_input("title must be 1-200 characters"));
        }
        let project_root = str_arg(&input, "projectRoot")?.to_string();
        if project_root.is_empty() {
            return Err(invalid_input("projectRoot cannot be empty"));
        }

        // Deserialize the full payload so the Host receives a well-formed proposal.
        let payload: CreativeProposalPayload = serde_json::from_value(input.clone())
            .map_err(|e| invalid_input(format!("malformed proposal: {e}")))?;

        // Driver kind must be one of the supported set.
        let driver_ok = matches!(
            payload.driver,
            CreativeProposedDriver::Python { .. }
                | CreativeProposedDriver::Binary { .. }
                | CreativeProposedDriver::StaticHttp
                | CreativeProposedDriver::Compose { .. }
        );
        if !driver_ok {
            return Err(invalid_input("unsupported driver kind"));
        }

        Ok(ToolOutput {
            result: serde_json::json!({
                "ok": true,
                "proposal": input,
                "message": "Proposal validated structurally. Awaiting user approval to register.",
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ToolCallContext {
        ToolCallContext::new(
            std::env::temp_dir(),
            "run-proposal".into(),
            "conv".into(),
            "tc".into(),
            "ask".into(),
        )
    }

    fn payload(kind: &str) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "kind": kind,
            "ownership": "managed",
            "title": "Dashboard",
            "projectRoot": "/proj",
            "driver": {"kind": "staticHttp"},
            "openPath": "/",
            "healthPath": "/",
            "environmentKeys": []
        })
    }

    #[tokio::test]
    async fn structural_validation_accepts_valid_proposal() {
        let tool = CreativeProposalTool;
        let out = tool.execute(payload("create"), &ctx()).await;
        assert!(out.is_ok());
        let out = out.unwrap();
        assert_eq!(out.result["ok"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn rejects_unknown_schema_version() {
        let tool = CreativeProposalTool;
        let mut p = payload("create");
        p["schemaVersion"] = serde_json::json!(99);
        let out = tool.execute(p, &ctx()).await;
        assert!(out.is_err());
    }

    #[tokio::test]
    async fn rejects_unknown_kind() {
        let tool = CreativeProposalTool;
        let out = tool.execute(payload("delete"), &ctx()).await;
        assert!(out.is_err());
    }

    #[tokio::test]
    async fn structural_pass_through_defers_to_host_gate() {
        // Structural layer does not reject privileged compose — the Host gate does.
        let tool = CreativeProposalTool;
        let mut p = payload("start");
        p["driver"] = serde_json::json!({"kind": "compose", "privileged": true});
        let out = tool.execute(p, &ctx()).await;
        assert!(out.is_ok(), "structural pass-through; Host gate rejects privileged");
    }
}
