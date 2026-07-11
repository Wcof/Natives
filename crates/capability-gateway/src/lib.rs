//! # Capability Gateway
//!
//! Tool schemas, policy enforcement, sandbox, and audit.
//! Every tool must be registered with a schema, side effect declaration,
//! permission class, path scope, timeout, output limit, and cancellation policy.

pub mod policy;
pub mod tools;

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
}

impl CapabilityGateway {
    pub fn new() -> Self {
        CapabilityGateway { tools: Vec::new() }
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
}

impl Default for CapabilityGateway {
    fn default() -> Self {
        Self::new()
    }
}