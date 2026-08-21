//! Codex CLI 集成 Adapter（CDX-001 ~ CDX-007）。

use super::adapter::{ApplyRequest, EnvRef, IntegrationError, ToolIntegration};
use super::common::*;
use super::model::{
    BackupResult, DetectResult, InspectResult, PlannedPatch, RollbackResult, ToolKind, VerifyResult,
};
use async_trait::async_trait;
use std::path::PathBuf;

pub struct CodexAdapter;

impl CodexAdapter {
    fn config_path() -> PathBuf {
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".codex")
            .join("config.json")
    }
}

#[async_trait]
impl ToolIntegration for CodexAdapter {
    fn kind(&self) -> ToolKind {
        ToolKind::Codex
    }

    async fn detect(&self) -> Result<DetectResult, IntegrationError> {
        let extra = vec![
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            home_dir().map(|h| h.join(".cargo/bin")).unwrap_or_default(),
            home_dir().map(|h| h.join(".codex/bin")).unwrap_or_default(),
        ];
        Ok(detect_tool(&["codex"], &extra))
    }

    async fn inspect(&self) -> Result<InspectResult, IntegrationError> {
        let path = Self::config_path();
        Ok(inspect_config(&path, &["key", "secret", "token", "auth"]))
    }

    async fn backup(&self) -> Result<BackupResult, IntegrationError> {
        let path = Self::config_path();
        backup_file(&path)
    }

    async fn plan(&self, desired_env: &[EnvRef]) -> Result<PlannedPatch, IntegrationError> {
        let path = Self::config_path();
        plan_patch(
            &path,
            desired_env,
            "OPENAI_BASE_URL",
            "OPENAI_API_KEY",
            "http://127.0.0.1:11434/v1",
        )
    }

    async fn apply(&self, request: ApplyRequest) -> Result<(), IntegrationError> {
        apply_patch(&request)
    }

    async fn verify(&self) -> Result<VerifyResult, IntegrationError> {
        let path = Self::config_path();
        verify_config(&path)
    }

    async fn rollback(&self) -> Result<RollbackResult, IntegrationError> {
        let path = Self::config_path();
        rollback_backup(&path, None)
    }
}
