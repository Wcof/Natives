//! # Capability Gateway
//!
//! Tool schemas, policy enforcement, sandbox, and audit.
//! Every tool must be registered with a schema, side effect declaration,
//! permission class, path scope, timeout, output limit, and cancellation policy.

pub mod manifest;
pub mod plan_mode;
pub mod platform_sandbox;
pub mod policy;
pub mod process_supervisor;
pub mod tools;

pub use manifest::ToolManifest;
pub use plan_mode::{Plan, PlanDecision, PlanSession, PlanState, PlanStep, PLAN_PROFILE};
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
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

/// Best-effort output emitted by a long-running handler. The Gateway owns the
/// process/MCP safety boundary; the caller owns persistence and rate limiting.
#[derive(Debug, Clone)]
pub struct ToolProgressChunk {
    pub stream: String,
    pub text: String,
}

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
    /// Optional live output channel for handlers that can stream progress.
    /// `None` keeps lightweight/test handlers allocation-free.
    pub progress: Option<UnboundedSender<ToolProgressChunk>>,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
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
            progress: None,
            turn_id: None,
            message_id: None,
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
            progress: None,
            turn_id: None,
            message_id: None,
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

        validate_schema(&tool.schema, &input).map_err(|message| ToolError {
            code: "invalid_arguments".into(),
            message: format!("tool `{name}` arguments failed schema validation: {message}"),
            retryable: false,
        })?;
        self.enforce_input_policy(tool, &input)?;

        let timeout = std::time::Duration::from_millis(tool.timeout_ms.max(1));
        let tool_cancel = context.cancel.child_token();
        let mut tool_context = context.clone();
        tool_context.cancel = tool_cancel.clone();
        let result = tokio::select! {
            result = tool.handler.execute(input, &tool_context) => result,
            _ = tokio::time::sleep(timeout) => {
                tool_cancel.cancel();
                Err(ToolError {
                    code: "timeout".into(),
                    message: format!("tool `{name}` exceeded {}ms", tool.timeout_ms),
                    retryable: true,
                })
            },
            _ = context.cancel.cancelled() => {
                tool_cancel.cancel();
                Err(ToolError {
                    code: "cancelled".into(),
                    message: format!("tool `{name}` cancelled"),
                    retryable: true,
                })
            },
        }?;

        if policy::check_output_limit(result.result.to_string().as_bytes(), tool.output_limit) {
            return Err(ToolError {
                code: "output_limit".into(),
                message: format!("tool `{name}` output exceeded {} bytes", tool.output_limit),
                retryable: false,
            });
        }
        Ok(result)
    }

    /// Validate one registered schema without executing its handler.
    pub fn validate_tool_schema(&self, name: &str) -> Result<(), ToolError> {
        let tool = self.get_tool(name).ok_or_else(|| ToolError {
            code: "unknown_tool".into(),
            message: format!("unknown tool: {name}"),
            retryable: false,
        })?;
        validate_schema_definition(&tool.schema)
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

/// Small, dependency-free Draft-07 subset used by the built-in manifests.
/// It covers the executable boundary (`type`, `properties`, `required`,
/// `additionalProperties`, `items`, `enum`, and numeric bounds) without
/// duplicating a full JSON Schema engine in the Agent Core.
fn validate_schema(schema: &serde_json::Value, value: &serde_json::Value) -> Result<(), String> {
    if let Some(types) = schema.get("type") {
        let matches = types
            .as_str()
            .map(|ty| schema_type_matches(ty, value))
            .unwrap_or_else(|| {
                types.as_array().is_some_and(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str())
                        .any(|ty| schema_type_matches(ty, value))
                })
            });
        if !matches {
            return Err(format!("expected {}, got {}", types, value_type(value)));
        }
    }
    if let Some(enum_values) = schema.get("enum").and_then(|v| v.as_array()) {
        if !enum_values.iter().any(|candidate| candidate == value) {
            return Err(format!("value is not one of {enum_values:?}"));
        }
    }
    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        let object = value
            .as_object()
            .ok_or_else(|| "required only applies to objects".to_string())?;
        for key in required.iter().filter_map(|v| v.as_str()) {
            if !object.contains_key(key) {
                return Err(format!("missing required property `{key}`"));
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        if let Some(object) = value.as_object() {
            for (key, property) in object {
                if let Some(property_schema) = properties.get(key) {
                    validate_schema(property_schema, property)
                        .map_err(|error| format!("property `{key}`: {error}"))?;
                } else if schema.get("additionalProperties")
                    == Some(&serde_json::Value::Bool(false))
                {
                    return Err(format!("unknown property `{key}`"));
                }
            }
        }
    }
    if let Some(items) = schema.get("items") {
        if let Some(array) = value.as_array() {
            for (index, item) in array.iter().enumerate() {
                validate_schema(items, item).map_err(|error| format!("item {index}: {error}"))?;
            }
        }
    }
    if let Some(minimum) = schema.get("minimum").and_then(|v| v.as_f64()) {
        if value.as_f64().is_some_and(|number| number < minimum) {
            return Err(format!("number is below minimum {minimum}"));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(|v| v.as_f64()) {
        if value.as_f64().is_some_and(|number| number > maximum) {
            return Err(format!("number is above maximum {maximum}"));
        }
    }
    Ok(())
}

fn validate_schema_definition(schema: &serde_json::Value) -> Result<(), ToolError> {
    let Some(object) = schema.as_object() else {
        return Err(ToolError {
            code: "invalid_schema".into(),
            message: "schema must be an object".into(),
            retryable: false,
        });
    };
    if let Some(ty) = object.get("type") {
        let valid = ty
            .as_str()
            .map(|value| {
                matches!(
                    value,
                    "object" | "array" | "string" | "integer" | "number" | "boolean" | "null"
                )
            })
            .unwrap_or_else(|| {
                ty.as_array().is_some_and(|items| {
                    items.iter().all(|item| {
                        item.as_str().is_some_and(|value| {
                            matches!(
                                value,
                                "object"
                                    | "array"
                                    | "string"
                                    | "integer"
                                    | "number"
                                    | "boolean"
                                    | "null"
                            )
                        })
                    })
                })
            });
        if !valid {
            return Err(ToolError {
                code: "invalid_schema".into(),
                message: "unsupported schema type".into(),
                retryable: false,
            });
        }
    }
    if let Some(properties) = object.get("properties") {
        let Some(properties) = properties.as_object() else {
            return Err(ToolError {
                code: "invalid_schema".into(),
                message: "properties must be an object".into(),
                retryable: false,
            });
        };
        for property in properties.values() {
            validate_schema_definition(property)?;
        }
    }
    if let Some(items) = object.get("items") {
        validate_schema_definition(items)?;
    }
    Ok(())
}

fn schema_type_matches(expected: &str, value: &serde_json::Value) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

impl Default for CapabilityGateway {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod p0_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHandler(AtomicUsize);

    #[async_trait::async_trait]
    impl ToolHandler for CountingHandler {
        async fn execute(
            &self,
            _input: serde_json::Value,
            _context: &ToolCallContext,
        ) -> Result<ToolOutput, ToolError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput {
                result: serde_json::json!({"ok": true}),
                truncated: false,
                duration_ms: 0,
            })
        }
    }

    fn gateway(handler: Arc<dyn ToolHandler + Send + Sync>, timeout_ms: u64) -> CapabilityGateway {
        let mut gateway = CapabilityGateway::new();
        gateway.register(Tool {
            name: "p0_test",
            description: "test",
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}, "mode": {"type": "string", "enum": ["read"]}},
                "required": ["path"]
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::Any,
            timeout_ms,
            output_limit: 4096,
            cancellable: true,
            handler,
        });
        gateway
    }

    fn context(cancel: CancellationToken) -> ToolCallContext {
        ToolCallContext::with_cancel(
            std::env::current_dir().unwrap(),
            "run-p0".into(),
            "conversation-p0".into(),
            "call-p0".into(),
            "readonly".into(),
            cancel,
        )
    }

    #[tokio::test]
    async fn schema_failure_never_reaches_handler() {
        let handler = Arc::new(CountingHandler(AtomicUsize::new(0)));
        let gateway = gateway(handler.clone(), 1000);
        let error = gateway
            .execute(
                "p0_test",
                serde_json::json!({"path": 7}),
                &context(CancellationToken::new()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_arguments");
        assert_eq!(handler.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn all_builtin_schemas_are_supported_by_validator() {
        let mut gateway = CapabilityGateway::new();
        gateway.register_builtins();
        for tool in gateway.list_tools() {
            gateway
                .validate_tool_schema(tool.name)
                .unwrap_or_else(|error| panic!("{}: {}", tool.name, error.message));
        }
    }

    #[tokio::test]
    async fn cancellation_wins_over_blocking_handler() {
        struct Blocking;
        #[async_trait::async_trait]
        impl ToolHandler for Blocking {
            async fn execute(
                &self,
                _: serde_json::Value,
                _: &ToolCallContext,
            ) -> Result<ToolOutput, ToolError> {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(ToolOutput {
                    result: serde_json::json!({}),
                    truncated: false,
                    duration_ms: 0,
                })
            }
        }
        let cancel = CancellationToken::new();
        let gateway = gateway(Arc::new(Blocking), 5000);
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            trigger.cancel();
        });
        let error = gateway
            .execute(
                "p0_test",
                serde_json::json!({"path": "ok"}),
                &context(cancel),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "cancelled");
    }
}
