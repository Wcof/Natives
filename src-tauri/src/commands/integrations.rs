//! AI Tool Integration 命令（CLD-001..007, CDX-001..007, GEM-001..007, OPC-001..007）。

use crate::integrations::{
    self, ApplyRequest, BackupResult, DetectResult, EnvRef, InspectResult, PlannedPatch,
    RollbackResult, ToolKind, VerifyResult,
};
use crate::{Error, Result};
use serde::Deserialize;

fn parse_tool_kind(tool: &str) -> Result<ToolKind> {
    match tool {
        "claude_code" | "claude" => Ok(ToolKind::ClaudeCode),
        "codex" => Ok(ToolKind::Codex),
        "gemini_cli" | "gemini" => Ok(ToolKind::GeminiCli),
        "opencode" => Ok(ToolKind::OpenCode),
        other => Err(Error::Internal(format!("Unsupported tool kind: {other}"))),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInput {
    pub tool: String,
    #[serde(default)]
    pub env_refs: Vec<EnvRefDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvRefDto {
    pub key: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInput {
    pub tool: String,
    pub patch: PlannedPatch,
    pub user_approved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackInput {
    pub tool: String,
    pub backup_path: Option<String>,
}

#[tauri::command]
pub async fn tool_detect(tool: String) -> Result<DetectResult> {
    let kind = parse_tool_kind(&tool)?;
    let adapter = integrations::get_integration(kind);
    adapter
        .detect()
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))
}

#[tauri::command]
pub async fn tool_inspect(tool: String) -> Result<InspectResult> {
    let kind = parse_tool_kind(&tool)?;
    let adapter = integrations::get_integration(kind);
    adapter
        .inspect()
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))
}

#[tauri::command]
pub async fn tool_backup(tool: String) -> Result<BackupResult> {
    let kind = parse_tool_kind(&tool)?;
    let adapter = integrations::get_integration(kind);
    adapter
        .backup()
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))
}

#[tauri::command]
pub async fn tool_plan(input: PlanInput) -> Result<PlannedPatch> {
    let kind = parse_tool_kind(&input.tool)?;
    let adapter = integrations::get_integration(kind);
    let env_refs: Vec<EnvRef> = input
        .env_refs
        .into_iter()
        .map(|dto| EnvRef {
            key: dto.key,
            source: dto.source,
        })
        .collect();
    adapter
        .plan(&env_refs)
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))
}

#[tauri::command]
pub async fn tool_apply(input: ApplyInput) -> Result<bool> {
    let kind = parse_tool_kind(&input.tool)?;
    let adapter = integrations::get_integration(kind);
    adapter
        .apply(ApplyRequest {
            patch: input.patch,
            user_approved: input.user_approved,
        })
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))?;
    Ok(true)
}

#[tauri::command]
pub async fn tool_verify(tool: String) -> Result<VerifyResult> {
    let kind = parse_tool_kind(&tool)?;
    let adapter = integrations::get_integration(kind);
    adapter
        .verify()
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))
}

#[tauri::command]
pub async fn tool_rollback(input: RollbackInput) -> Result<RollbackResult> {
    let kind = parse_tool_kind(&input.tool)?;
    let adapter = integrations::get_integration(kind);
    adapter
        .rollback()
        .await
        .map_err(|e| Error::Internal(format!("{}: {}", e.category, e.message)))
}
