//! Creative-session draft tools: write / read / rollback / lint / create.
//!
//! These are the *only* write surface a creative session gets — the model can
//! shape a Workshop SPA without ever being handed `write_file` /
//! `apply_patch` / `run_terminal` (ADR-0014 invariant #3).

use crate::{
    PathScope, PermissionClass, SideEffect, Tool, ToolCallContext, ToolError, ToolHandler,
    ToolOutput,
};
use std::sync::Arc;

use super::store::*;

pub struct WriteDraftModuleTool {
    paths: Option<DraftPaths>,
}

pub struct ReadDraftModuleTool {
    paths: Option<DraftPaths>,
}

pub struct RollbackDraftRevisionTool {
    paths: Option<DraftPaths>,
}

pub struct LintDraftModuleTool;

impl WriteDraftModuleTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    #[cfg(test)]
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for WriteDraftModuleTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadDraftModuleTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    #[cfg(test)]
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for ReadDraftModuleTool {
    fn default() -> Self {
        Self::new()
    }
}

impl RollbackDraftRevisionTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    #[cfg(test)]
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for RollbackDraftRevisionTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ToolHandler for WriteDraftModuleTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = str_arg(&input, "draftId")?.to_string();
        validate_draft_id(&draft_id)?;
        let html = str_arg(&input, "htmlContent")?.to_string();
        ensure_html_size(&html)?;
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Lint first, before anything is opened or created: a rejected revision
        // must leave the draft byte-identical to what the user is previewing.
        let lint = contract_linter::lint_html(&html);
        if !lint.passed {
            return Err(lint_failure(&lint));
        }

        let paths = resolve_paths!(self);
        let conversation_id = context.conversation_id.clone();
        let draft_for_worker = draft_id.clone();
        let outcome = in_blocking(move || {
            append_revision(
                &paths,
                &draft_for_worker,
                &conversation_id,
                &html,
                name.as_deref(),
            )
        })
        .await?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "revision": outcome.revision,
                "contentHash": outcome.content_hash,
                "previewUrl": preview_url(&draft_id),
                "warnings": lint.warnings,
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[async_trait::async_trait]
impl ToolHandler for ReadDraftModuleTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = str_arg(&input, "draftId")?.to_string();
        validate_draft_id(&draft_id)?;

        let paths = resolve_paths!(self);
        let conversation_id = context.conversation_id.clone();
        let draft_for_worker = draft_id.clone();
        let (revision, html) =
            in_blocking(move || read_current_revision(&paths, &draft_for_worker, &conversation_id))
                .await?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "revision": revision,
                "content": html,
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[async_trait::async_trait]
impl ToolHandler for RollbackDraftRevisionTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = str_arg(&input, "draftId")?.to_string();
        validate_draft_id(&draft_id)?;

        let paths = resolve_paths!(self);
        let conversation_id = context.conversation_id.clone();
        let draft_for_worker = draft_id.clone();
        let (previous, revision) =
            in_blocking(move || rollback_revision(&paths, &draft_for_worker, &conversation_id))
                .await?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "revision": revision,
                "rolledBackFrom": previous,
                "previewUrl": preview_url(&draft_id),
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[async_trait::async_trait]
impl ToolHandler for LintDraftModuleTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let html = str_arg(&input, "htmlContent")?;
        ensure_html_size(html)?;
        let lint = contract_linter::lint_html(html);
        // A failed pre-flight is a successful tool call: the model asked a
        // question and got the answer it needs to fix the content.
        Ok(ToolOutput {
            result: serde_json::json!({
                "passed": lint.passed,
                "errors": lint.errors,
                "warnings": lint.warnings,
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

pub struct CreateCreativeDraftTool {
    paths: Option<DraftPaths>,
}

impl CreateCreativeDraftTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    #[cfg(test)]
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for CreateCreativeDraftTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Ordinary-assistant handoff (batch 3): create a draft row the creative
/// surface can continue. The user still publishes via a Host command — the
/// assistant only ever creates a draft, never an Application.
#[async_trait::async_trait]
impl ToolHandler for CreateCreativeDraftTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let intent = str_arg(&input, "intent")?.trim().to_string();
        if intent.is_empty() || intent.chars().count() > 4_000 {
            return Err(invalid_input(
                "intent must be 1-4000 characters describing the app idea",
            ));
        }
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Untitled creation".to_string());

        let paths = self.paths.clone().unwrap_or_else(DraftPaths::from_env);
        let draft_id = format!("draft-{}", uuid::Uuid::new_v4().simple());
        validate_draft_id(&draft_id)?;

        let conn = paths.open_db()?;
        let t = now_rfc3339();
        conn.execute(
            "INSERT INTO creative_drafts
                (draft_id, name, intent, conversation_id, origin_module_id,
                 current_revision, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, NULL, 0, 'drafting', ?4, ?4)",
            rusqlite::params![draft_id, name, intent, t],
        )
        .map_err(|e| draft_io_error(format!("create draft row failed: {e}")))?;
        // Ensure the draft directory exists so creative-session tools can write
        // revisions without a race.
        std::fs::create_dir_all(paths.draft_dir(&draft_id)?)
            .map_err(|e| draft_io_error(format!("create draft dir failed: {e}")))?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "name": name,
                "status": "draft_created",
                "message": "Draft created. Continue it in the Personal Creations surface; publishing is a user action, not available to the assistant.",
                "previewUrl": preview_url(&draft_id),
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

/// The ordinary-assistant handoff tool (batch 3). Distinct from the
/// creative-session surface so a general session can create a draft without
/// ever gaining the draft-writing tools.
pub fn creative_handoff_tool() -> Tool {
    Tool {
        name: "create_creative_draft",
        description: "Create a draft of a personal creation (a small web app) from a one-line idea. Returns a draftId the user can continue in the Personal Creations surface. Publishing to the app catalog is a user action, never available to the assistant.",
        schema: serde_json::json!({
            "type":"object",
            "properties":{
                "intent":{"type":"string","description":"One-line description of the app the user wants"},
                "name":{"type":"string","description":"Optional suggested app name"}
            },
            "required":["intent"]
        }),
        side_effect: SideEffect::Write,
        path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
        permission_class: PermissionClass::AlwaysAllowed,
        timeout_ms: 10_000,
        output_limit: 16_000,
        cancellable: true,
        parallel_safe: false,
        conflict_key: None,
        handler: Arc::new(CreateCreativeDraftTool::new()),
    }
}

/// The creative-session tool surface.
///
/// Deliberately *not* part of [`super::builtin_tools`]: a general session has no
/// business writing drafts, and keeping these out of the default surface means
/// they can only appear where an allowlist names them.
pub fn creative_draft_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "write_draft_module",
            description: "Write a new revision of a creative draft's HTML. Runs the Contract Linter first; on failure nothing is written and the errors are returned so you can fix them.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "draftId":{"type":"string","description":"The draft id given by the session"},
                    "htmlContent":{"type":"string","description":"Complete standalone HTML document"},
                    "name":{"type":"string","description":"Optional new draft name"}
                },
                "required":["draftId","htmlContent"]
            }),
            side_effect: SideEffect::Write,
            // Scope is the drafts root, not the project: drafts deliberately live
            // outside any workspace. Declared for audit — the gateway's path
            // policy never sees these inputs because none of them is a path key,
            // which is the point: the model supplies an id, never a path.
            path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
            // AlwaysAllowed because containment, not consent, is what makes this
            // safe (ADR-0014 section 8). Every iteration of the generate loop would
            // otherwise raise a prompt.
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 15_000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(WriteDraftModuleTool::new()),
        },
        Tool {
            name: "read_draft_module",
            description: "Read the creative draft's current revision HTML, so edits start from what the user is actually previewing.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{"draftId":{"type":"string"}},
                "required":["draftId"]
            }),
            side_effect: SideEffect::ReadOnly,
            path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 10_000,
            // One revision may be up to 5 MiB; JSON escaping needs headroom.
            output_limit: 8_388_608,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(ReadDraftModuleTool::new()),
        },
        Tool {
            name: "rollback_draft_revision",
            description: "Move the draft back to its previous revision. Revision files are kept, so a rollback can itself be rolled forward by writing again.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{"draftId":{"type":"string"}},
                "required":["draftId"]
            }),
            side_effect: SideEffect::Write,
            path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 10_000,
            output_limit: 16_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(RollbackDraftRevisionTool::new()),
        },
        Tool {
            name: "lint_draft_module",
            description: "Check HTML against the Contract Linter without writing anything. Same rules that gate publishing.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{"htmlContent":{"type":"string"}},
                "required":["htmlContent"]
            }),
            side_effect: SideEffect::ReadOnly,
            // Touches no path at all.
            path_scope: PathScope::None,
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 5_000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(LintDraftModuleTool),
        },
    ]
}
