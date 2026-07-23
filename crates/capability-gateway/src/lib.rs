//! # Capability Gateway
//!
//! Tool schemas, policy enforcement, sandbox, and audit.
//! Every tool must be registered with a schema, side effect declaration,
//! permission class, path scope, timeout, output limit, and cancellation policy.

pub mod policy;
pub mod manifest;
pub mod platform_sandbox;
pub mod process_supervisor;
pub mod tools;

pub use manifest::ToolManifest;
pub use platform_sandbox::{
    allow_autonomous_shell, wrap_command_macos, PlatformCapabilities, SandboxProfile,
};
pub use process_supervisor::{
    global_process_supervisor, FakeProcessSupervisor, LocalProcessSupervisor, ProcessSnapshot,
    ProcessSpec, ProcessState, ProcessSupervisor, DEFAULT_FOREGROUND_BUDGET_MS,
};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Context passed to tool handlers during execution.
///
/// Must be constructed from a verified ProjectIdentity (task-10). Do not
/// Context for a tool call execution.
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    /// The project root directory (canonical, absolute).
    pub project_root: PathBuf,
    /// The current working directory for the tool call.
    pub working_dir: PathBuf,
    /// The run ID for this execution.
    pub run_id: String,
    /// The conversation ID.
    pub conversation_id: String,
    /// The tool call ID (from provider).
    pub tool_call_id: String,
    /// Permission profile for this run.
    pub permission_profile: String,
    /// Stable project identity UUID (required for privileged tools).
    pub project_id: Option<String>,
    /// Identity version at verification time.
    pub project_identity_version: Option<u32>,
    /// Shared run cancellation token (task-03). Tools/MCP must select on this.
    pub cancel: CancellationToken,
}

impl ToolCallContext {
    /// Create a new context with required fields.
    ///
    /// Prefer [`Self::from_verified_identity`] when a ProjectIdentity is available.
    pub fn new(
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
    ) -> Self {
        Self::with_cancel(
            project_root,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            CancellationToken::new(),
        )
    }

    /// Context bound to a registry-owned cancel token.
    pub fn with_cancel(
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
        cancel: CancellationToken,
    ) -> Self {
        let working_dir = project_root.clone();
        Self {
            project_root,
            working_dir,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            project_id: None,
            project_identity_version: None,
            cancel,
        }
    }

    /// Construct context from a verified project identity (task-10).
    pub fn from_verified_identity(
        project_id: impl Into<String>,
        identity_version: u32,
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
    ) -> Self {
        Self::from_verified_identity_with_cancel(
            project_id,
            identity_version,
            project_root,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            CancellationToken::new(),
        )
    }

    /// Verified identity + registry-owned cancel token.
    pub fn from_verified_identity_with_cancel(
        project_id: impl Into<String>,
        identity_version: u32,
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
        cancel: CancellationToken,
    ) -> Self {
        let working_dir = project_root.clone();
        Self {
            project_root,
            working_dir,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            project_id: Some(project_id.into()),
            project_identity_version: Some(identity_version),
            cancel,
        }
    }

    /// Resolve a path relative to working_dir, then validate it's within project_root.
    pub fn resolve_path(&self, input_path: &str) -> Result<PathBuf, ToolError> {
        use std::path::Path;
        let path = Path::new(input_path);
        let resolved = if path.is_relative() {
            self.working_dir.join(path)
        } else {
            path.to_path_buf()
        };
        // Canonicalize if exists, otherwise check parent
        let canonical = if resolved.exists() {
            resolved.canonicalize().map_err(|e| ToolError {
                code: "PATH_ERROR".into(),
                message: format!("Failed to canonicalize path: {e}"),
                retryable: false,
            })?
        } else {
            // For new files, canonicalize parent and append filename
            let parent = resolved.parent().unwrap_or(Path::new("."));
            let file_name = resolved.file_name().ok_or_else(|| ToolError {
                code: "PATH_ERROR".into(),
                message: "Invalid path: no filename".into(),
                retryable: false,
            })?;
            let canonical_parent = parent.canonicalize().map_err(|e| ToolError {
                code: "PATH_ERROR".into(),
                message: format!("Failed to canonicalize parent path: {e}"),
                retryable: false,
            })?;
            canonical_parent.join(file_name)
        };
        // Verify within project root
        if !canonical.starts_with(&self.project_root) {
            return Err(ToolError {
                code: "PATH_ESCAPE".into(),
                message: format!(
                    "Path {} escapes project root {}",
                    canonical.display(),
                    self.project_root.display()
                ),
                retryable: false,
            });
        }
        Ok(canonical)
    }
}

/// A registered tool.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: serde_json::Value,
    pub side_effect: SideEffect,
    pub permission_class: PermissionClass,
    pub path_scope: PathScope,
    pub timeout_ms: u64,
    pub output_limit: u64,
    pub cancellable: bool,
    pub handler: Arc<dyn ToolHandler + Send + Sync>,
}

/// Tool side effect classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideEffect {
    ReadOnly,
    Write,
    Destructive,
    Network,
    Process,
}

/// Permission class for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionClass {
    AlwaysAllowed,
    ProjectRead,
    ProjectWrite,
    ExternalWrite,
    Credentials,
    Elevation,
    DestructiveCommand,
    PrivacyResource,
}

/// Path scope restriction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathScope {
    None,
    Project(String),
    Glob(String),
    Any,
}

/// Tool handler trait.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError>;
}

/// Tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub result: serde_json::Value,
    pub truncated: bool,
    pub duration_ms: u64,
}

/// Tool error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// The capability gateway — manages tool registration and enforcement.
pub struct CapabilityGateway {
    tools: Vec<Tool>,
    /// When set, path tools are constrained to this project root even if
    /// the tool's declared `PathScope` is `Any`.
    pub project_root: Option<String>,
}

impl CapabilityGateway {
    pub fn new() -> Self {
        CapabilityGateway {
            tools: Vec::new(),
            project_root: None,
        }
    }

    pub fn with_project_root(mut self, root: impl Into<String>) -> Self {
        self.project_root = Some(root.into());
        self
    }

    pub fn set_project_root(&mut self, root: impl Into<String>) {
        self.project_root = Some(root.into());
    }

    pub fn register(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    pub fn get_tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }

    pub fn list_tools(&self) -> Vec<&Tool> {
        self.tools.iter().collect()
    }

    /// Register all built-in tools.
    pub fn register_builtins(&mut self) {
        let builtins = tools::builtin_tools();
        for tool in builtins {
            self.register(tool);
        }
    }

    /// Enforce path/timeout/output policy, then run the tool handler.
    /// Callers must use this instead of invoking `handler.execute` directly.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self.get_tool(name).ok_or_else(|| ToolError {
            code: "unknown_tool".into(),
            message: format!("unknown tool: {name}"),
            retryable: false,
        })?;

        self.enforce_input_policy(tool, &input)?;

        let timeout = std::time::Duration::from_millis(tool.timeout_ms.max(1));
        let result = tokio::time::timeout(timeout, tool.handler.execute(input, context))
            .await
            .map_err(|_| ToolError {
                code: "timeout".into(),
                message: format!("tool `{name}` exceeded {}ms", tool.timeout_ms),
                retryable: true,
            })??;

        if policy::check_output_limit(
            result.result.to_string().as_bytes(),
            tool.output_limit,
        ) {
            return Err(ToolError {
                code: "output_limit".into(),
                message: format!(
                    "tool `{name}` output exceeded {} bytes",
                    tool.output_limit
                ),
                retryable: false,
            });
        }
        Ok(result)
    }

    fn enforce_input_policy(
        &self,
        tool: &Tool,
        input: &serde_json::Value,
    ) -> Result<(), ToolError> {
        let path_keys = ["path", "root", "cwd", "file", "directory", "dir"];
        for key in path_keys {
            if let Some(path) = input.get(key).and_then(|v| v.as_str()) {
                policy::check_path_traversal(path)?;
                let effective_scope = self.effective_path_scope(&tool.path_scope);
                match policy::check_path_scope(path, &effective_scope) {
                    policy::PolicyResult::Allowed => {}
                    policy::PolicyResult::Denied(msg)
                    | policy::PolicyResult::NeedsApproval(msg) => {
                        // Outside scope is hard-denied at gateway; elevation uses
                        // PermissionClass Ask path at the engine layer when allowed.
                        return Err(ToolError {
                            code: "path_scope_denied".into(),
                            message: msg,
                            retryable: false,
                        });
                    }
                }
            }
        }

        if matches!(
            tool.side_effect,
            SideEffect::Process | SideEffect::Destructive
        ) {
            if let Some(cmd) = input
                .get("command")
                .or_else(|| input.get("cmd"))
                .and_then(|v| v.as_str())
            {
                policy::check_command_injection(cmd)?;
            }
        }
        Ok(())
    }

    fn effective_path_scope(&self, declared: &PathScope) -> PathScope {
        match declared {
            PathScope::Any => {
                if let Some(root) = &self.project_root {
                    PathScope::Project(root.clone())
                } else {
                    PathScope::Any
                }
            }
            other => other.clone(),
        }
    }
}

impl Default for CapabilityGateway {
    fn default() -> Self {
        Self::new()
    }
}
