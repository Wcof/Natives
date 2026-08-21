//! AI Tool Integration adapter trait（05 §6 七步契约）。
//!
//! 每个外部 AI CLI（Claude Code / Codex / Gemini CLI / OpenCode）实现本
//! trait；配置流程统一为 Detect → Inspect → Backup → Plan → Apply → Verify
//! → Rollback。每次写入必须 backup + atomic patch + verify，失败可回滚。

use async_trait::async_trait;

use super::model::{
    BackupResult, DetectResult, InspectResult, PlannedPatch, RollbackResult, ToolKind, VerifyResult,
};

/// 一次配置变更的请求（含用户显式授权确认）。
#[derive(Debug, Clone)]
pub struct ApplyRequest {
    /// 用户确认后的补丁。
    pub patch: PlannedPatch,
    /// 是否已获得用户显式授权（所有非检测/非只读步骤必须为 true）。
    pub user_approved: bool,
}

/// AI 工具配置集成 adapter。
#[async_trait]
pub trait ToolIntegration: Send + Sync {
    fn kind(&self) -> ToolKind;

    /// 1. Detect：工具是否安装。
    async fn detect(&self) -> Result<DetectResult, IntegrationError>;

    /// 2. Inspect：当前配置状态（只读）。
    async fn inspect(&self) -> Result<InspectResult, IntegrationError>;

    /// 3. Backup：变更前备份（幂等）。
    async fn backup(&self) -> Result<BackupResult, IntegrationError>;

    /// 4. Plan：计算补丁（只读计算，不写盘）。
    async fn plan(&self, desired_env: &[EnvRef]) -> Result<PlannedPatch, IntegrationError>;

    /// 5. Apply：写入补丁（必须 backup + atomic patch）。
    async fn apply(&self, request: ApplyRequest) -> Result<(), IntegrationError>;

    /// 6. Verify：验证配置可用。
    async fn verify(&self) -> Result<VerifyResult, IntegrationError>;

    /// 7. Rollback：恢复备份。
    async fn rollback(&self) -> Result<RollbackResult, IntegrationError>;
}

/// 环境变量引用（仅引用名 + 来源，不携带明文 Secret）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRef {
    pub key: String,
    /// `secret_ref`（Keychain）或已存在环境变量名。
    pub source: String,
}

/// Integration 层错误：不携带 Secret / 配置明文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationError {
    pub category: String,
    pub message: String,
}

impl IntegrationError {
    pub fn new(category: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            category: category.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 七步契约的 fake 实现：证明 trait 可被任意工具 adapter 实现，
    /// 且 Apply 未获用户授权时必须拒绝。
    struct FakeTool {
        backed_up: bool,
    }

    #[async_trait]
    impl ToolIntegration for FakeTool {
        fn kind(&self) -> ToolKind {
            ToolKind::ClaudeCode
        }
        async fn detect(&self) -> Result<DetectResult, IntegrationError> {
            Ok(DetectResult {
                installed: true,
                version: Some("1.0.0".into()),
                executable: Some("/usr/local/bin/claude".into()),
            })
        }
        async fn inspect(&self) -> Result<InspectResult, IntegrationError> {
            Ok(InspectResult {
                config_path: Some("~/.claude/settings.json".into()),
                exists: true,
                managed: true,
                sensitive_keys: vec!["env.ANTHROPIC_API_KEY".into()],
                summary: "2 env entries".into(),
            })
        }
        async fn backup(&self) -> Result<BackupResult, IntegrationError> {
            Ok(BackupResult {
                backup_path: Some("/tmp/backup-1".into()),
                created: true,
            })
        }
        async fn plan(&self, _desired: &[EnvRef]) -> Result<PlannedPatch, IntegrationError> {
            Ok(PlannedPatch {
                target: "~/.claude/settings.json".into(),
                patch_json: r#"{"env":{"ANTHROPIC_API_KEY":"{{secret_ref}}"}}"#.into(),
                summary: "set env ref".into(),
            })
        }
        async fn apply(&self, request: ApplyRequest) -> Result<(), IntegrationError> {
            if !request.user_approved {
                return Err(IntegrationError::new(
                    "approval_required",
                    "user approval required before apply",
                ));
            }
            if !self.backed_up {
                return Err(IntegrationError::new(
                    "backup_required",
                    "backup must precede apply",
                ));
            }
            Ok(())
        }
        async fn verify(&self) -> Result<VerifyResult, IntegrationError> {
            Ok(VerifyResult {
                ok: true,
                checks: vec!["parses".into()],
                error: None,
            })
        }
        async fn rollback(&self) -> Result<RollbackResult, IntegrationError> {
            Ok(RollbackResult {
                restored: true,
                backup_path: Some("/tmp/backup-1".into()),
            })
        }
    }

    #[tokio::test]
    async fn seven_step_contract_flows_in_order() {
        let tool = FakeTool { backed_up: true };
        let detect = tool.detect().await.unwrap();
        assert!(detect.installed);
        assert_eq!(tool.kind(), ToolKind::ClaudeCode);

        let inspect = tool.inspect().await.unwrap();
        assert!(inspect.managed);
        assert!(inspect
            .sensitive_keys
            .contains(&"env.ANTHROPIC_API_KEY".to_string()));

        let backup = tool.backup().await.unwrap();
        assert!(backup.created);

        let patch = tool.plan(&[]).await.unwrap();
        assert!(!patch.patch_json.contains("sk-"), "补丁不得包含明文 Secret");

        tool.apply(ApplyRequest {
            patch,
            user_approved: true,
        })
        .await
        .unwrap();

        let verify = tool.verify().await.unwrap();
        assert!(verify.ok);

        let rollback = tool.rollback().await.unwrap();
        assert!(rollback.restored);
    }

    #[tokio::test]
    async fn apply_without_user_approval_is_rejected() {
        let tool = FakeTool { backed_up: true };
        let patch = tool.plan(&[]).await.unwrap();
        let error = tool
            .apply(ApplyRequest {
                patch,
                user_approved: false,
            })
            .await
            .unwrap_err();
        assert_eq!(error.category, "approval_required");
    }
}
