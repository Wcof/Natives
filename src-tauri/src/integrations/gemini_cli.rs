//! Gemini CLI 集成 Adapter（GEM-001 ~ GEM-007）。

use super::adapter::{ApplyRequest, EnvRef, IntegrationError, ToolIntegration};
use super::common::*;
use super::model::{
    BackupResult, DetectResult, InspectResult, PlannedPatch, RollbackResult, ToolKind, VerifyResult,
};
use async_trait::async_trait;
use std::path::PathBuf;

pub struct GeminiCliAdapter;

impl GeminiCliAdapter {
    fn config_path() -> PathBuf {
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".gemini")
            .join("settings.json")
    }
}

#[async_trait]
impl ToolIntegration for GeminiCliAdapter {
    fn kind(&self) -> ToolKind {
        ToolKind::GeminiCli
    }

    async fn detect(&self) -> Result<DetectResult, IntegrationError> {
        let extra = vec![
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
        ];
        Ok(detect_tool(&["gemini"], &extra))
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
            "GEMINI_BASE_URL",
            "GEMINI_API_KEY",
            "http://127.0.0.1:11434",
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
