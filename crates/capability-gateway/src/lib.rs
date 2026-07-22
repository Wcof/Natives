//! # Capability Gateway
//!
//! Tool schemas, policy enforcement, sandbox, and audit.
//! Every tool must be registered with a schema, side effect declaration,
//! permission class, path scope, timeout, output limit, and cancellation policy.

pub mod policy;
pub mod manifest;
pub mod process_supervisor;
pub mod tools;

pub use manifest::ToolManifest;
pub use process_supervisor::{
    FakeProcessSupervisor, LocalProcessSupervisor, ProcessSnapshot, ProcessSpec, ProcessState,
    ProcessSupervisor, DEFAULT_FOREGROUND_BUDGET_MS,
};

use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError>;
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
    ) -> Result<ToolOutput, ToolError> {
        let tool = self.get_tool(name).ok_or_else(|| ToolError {
            code: "unknown_tool".into(),
            message: format!("unknown tool: {name}"),
            retryable: false,
        })?;

        self.enforce_input_policy(tool, &input)?;

        let timeout = std::time::Duration::from_millis(tool.timeout_ms.max(1));
        let result = tokio::time::timeout(timeout, tool.handler.execute(input))
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
