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
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

/// Bounded live-output channel capacity for long-running handlers (H03). A
/// slow consumer must not grow memory unboundedly: overflow is dropped at the
/// producer and counted in `ToolCallContext::progress_dropped_bytes`.
pub const TERMINAL_PROGRESS_CAPACITY: usize = 4096;

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
    /// Bounded (`TERMINAL_PROGRESS_CAPACITY`); overflow is dropped and counted
    /// in `progress_dropped_bytes`. `None` keeps lightweight/test handlers
    /// allocation-free.
    pub progress: Option<Sender<ToolProgressChunk>>,
    /// Bytes of live progress dropped because the bounded channel was full.
    /// Progress is non-authoritative; overflow must never stall the handler.
    pub progress_dropped_bytes: Arc<AtomicU64>,
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
            progress_dropped_bytes: Arc::new(AtomicU64::new(0)),
            turn_id: None,
            message_id: None,
        }
    }

    /// Attach a bounded live-output channel and its dropped-bytes counter. The
    /// counter is shared with the daemon so overflow is observable.
    pub fn set_progress(
        &mut self,
        progress: Option<Sender<ToolProgressChunk>>,
        dropped_bytes: Arc<AtomicU64>,
    ) {
        self.progress = progress;
        self.progress_dropped_bytes = dropped_bytes;
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
            progress_dropped_bytes: Arc::new(AtomicU64::new(0)),
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

/// A path authorized by the Gateway for checkpoint/rewind I/O.
///
/// Produced only by [`CapabilityGateway::preflight_write_paths`]: `canonical`
/// is verified to be inside the project root, and `project_relative` is the
/// in-root form used as the checkpoint key and rewind path. Checkpoint must
/// consume only this type — a raw caller-supplied path cannot be represented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPath {
    /// Absolute canonical path on disk.
    pub canonical: PathBuf,
    /// Path relative to the project root.
    pub project_relative: PathBuf,
}

impl TrustedPath {
    pub fn new(canonical: PathBuf, project_relative: PathBuf) -> Self {
        Self {
            canonical,
            project_relative,
        }
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
    /// Explicit scheduling declaration. `false` by default so a tool is
    /// Sequential unless its author proves it safe to run concurrently with
    /// other tools (read-only file search/read tools). Never inferred from
    /// `SideEffect` — "read-only" does not imply parallel-safe.
    pub parallel_safe: bool,
    /// Tools that must not run concurrently with each other share a key.
    pub conflict_key: Option<String>,
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

/// Scheduling declaration owned by the Gateway. Core consumes this metadata
/// but never infers concurrency from tool names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    ParallelSafe,
    Sequential,
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCapability {
    pub name: String,
    pub schema: serde_json::Value,
    pub execution_mode: ExecutionMode,
    pub side_effect: SideEffect,
    pub conflict_key: Option<String>,
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

    pub fn list_capabilities(&self) -> Vec<ToolCapability> {
        self.tools
            .iter()
            .map(|tool| ToolCapability {
                name: tool.name.to_string(),
                schema: tool.schema.clone(),
                execution_mode: if tool.parallel_safe {
                    ExecutionMode::ParallelSafe
                } else {
                    // Explicit declaration only. Destructive/process tools are
                    // Exclusive (they must never share a slot); everything else
                    // defaults to Sequential rather than being auto-parallelized
                    // from a "read-only" side effect.
                    match tool.side_effect {
                        SideEffect::Destructive | SideEffect::Process => ExecutionMode::Exclusive,
                        SideEffect::ReadOnly | SideEffect::Write | SideEffect::Network => {
                            ExecutionMode::Sequential
                        }
                    }
                },
                side_effect: tool.side_effect,
                conflict_key: tool.conflict_key.clone(),
            })
            .collect()
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
        // Keep the handler alive long enough to observe cancellation. Dropping
        // the future at the select boundary would skip handler-owned cleanup
        // (child wait/reap, network close, MCP request disposal).
        let handler = tool.handler.clone();
        let handler_task = tokio::spawn(async move { handler.execute(input, &tool_context).await });
        let mut handler_task = Box::pin(handler_task);
        const CLEANUP_GRACE: std::time::Duration = std::time::Duration::from_millis(250);
        let result = tokio::select! {
            joined = &mut handler_task => joined
                .map_err(|error| ToolError {
                    code: "handler_error".into(),
                    message: format!("tool `{name}` task failed: {error}"),
                    retryable: true,
                })
                .and_then(|result| result),
            _ = tokio::time::sleep(timeout) => {
                tool_cancel.cancel();
                match tokio::time::timeout(CLEANUP_GRACE, &mut handler_task).await {
                    Ok(Ok(_)) => Err(ToolError {
                        code: "timeout".into(),
                        message: format!("tool `{name}` exceeded {}ms", tool.timeout_ms),
                        retryable: true,
                    }),
                    Ok(Err(error)) => Err(ToolError {
                        code: "cleanup_failed".into(),
                        message: format!("tool `{name}` cleanup task failed: {error}"),
                        retryable: false,
                    }),
                    Err(_) => {
                        // The handler ignored the cancellation token; abort the
                        // task so the wrapper cannot leak it past the call.
                        // `cleanup_failed` keeps the uncertain resource visible.
                        //
                        // ponytail: fixed grace window; handlers that need a
                        // longer shutdown must expose their own bounded cleanup.
                        handler_task.as_mut().abort();
                        Err(ToolError {
                            code: "cleanup_failed".into(),
                            message: format!("tool `{name}` did not stop after timeout"),
                            retryable: false,
                        })
                    }
                }
            },
            _ = context.cancel.cancelled() => {
                tool_cancel.cancel();
                match tokio::time::timeout(CLEANUP_GRACE, &mut handler_task).await {
                    Ok(Ok(_)) => Err(ToolError {
                        code: "cancelled".into(),
                        message: format!("tool `{name}` cancelled"),
                        retryable: true,
                    }),
                    Ok(Err(error)) => Err(ToolError {
                        code: "cleanup_failed".into(),
                        message: format!("tool `{name}` cleanup task failed: {error}"),
                        retryable: false,
                    }),
                    Err(_) => {
                        handler_task.as_mut().abort();
                        Err(ToolError {
                            code: "cleanup_failed".into(),
                            message: format!("tool `{name}` did not stop after cancellation"),
                            retryable: false,
                        })
                    }
                }
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

    /// Validate every registered schema before a run advertises the surface.
    /// A malformed manifest is a startup/preflight error, never a reason to
    /// silently disable validation for the whole Gateway.
    pub fn validate_registered_schemas(&self) -> Result<(), ToolError> {
        for tool in &self.tools {
            validate_schema_definition(&tool.schema).map_err(|mut error| {
                error.message = format!("{}: {}", tool.name, error.message);
                error
            })?;
        }
        Ok(())
    }

    /// Validate a dynamically advertised schema (for example an MCP tool)
    /// without registering a local handler.  The Gateway remains the single
    /// executable Schema boundary; callers must still perform their own
    /// lookup and permission checks before invoking the external transport.
    pub fn validate_external_input(
        schema: &serde_json::Value,
        input: &serde_json::Value,
    ) -> Result<(), ToolError> {
        validate_schema_definition(schema)?;
        validate_schema(schema, input).map_err(|message| ToolError {
            code: "schema_validation".into(),
            message,
            retryable: false,
        })
    }

    fn enforce_input_policy(
        &self,
        tool: &Tool,
        input: &serde_json::Value,
    ) -> Result<(), ToolError> {
        let path_keys = ["path", "root", "cwd", "file", "directory", "dir"];
        for key in path_keys {
            if let Some(path) = input.get(key).and_then(|v| v.as_str()) {
                self.check_path_for_scope(tool, path)?;
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

    /// Authorize the write paths a tool declares in `input` against the tool's
    /// path scope and canonical project root, returning only trusted paths.
    ///
    /// MUST run before any filesystem or checkpoint I/O. This is the single
    /// authority for "which paths may be read or written"; Checkpoint consumes
    /// only its output (via [`TrustedPath`]). Absolute, dotdot and symlink
    /// escapes are rejected here — the call must never reach the checkpoint,
    /// the ledger, or the handler.
    pub fn preflight_write_paths(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<Vec<TrustedPath>, ToolError> {
        let tool = self.get_tool(tool_name).ok_or_else(|| ToolError {
            code: "unknown_tool".into(),
            message: format!("unknown tool: {tool_name}"),
            retryable: false,
        })?;
        // Keep the same traversal/scope enforcement as execute(), so a call
        // rejected here is rejected identically there.
        self.enforce_input_policy(tool, input)?;
        let mut trusted = Vec::new();
        for raw in declared_write_paths(tool_name, input) {
            self.check_path_for_scope(tool, &raw)?;
            let canonical = context.resolve_path(&raw)?;
            let relative = canonical
                .strip_prefix(context.project_root.as_path())
                .map_err(|_| ToolError {
                    code: "PATH_ESCAPE".into(),
                    message: format!(
                        "resolved path {} escapes project root {}",
                        canonical.display(),
                        context.project_root.display()
                    ),
                    retryable: false,
                })?;
            trusted.push(TrustedPath::new(canonical.clone(), relative.to_path_buf()));
        }
        Ok(trusted)
    }

    /// Traversal + scope check for one path, matching the exact rejection
    /// `execute()` would produce. `NeedsApproval` is hard-denied here because
    /// the checkpoint/ledger path has no approval loop of its own.
    fn check_path_for_scope(&self, tool: &Tool, path: &str) -> Result<(), ToolError> {
        policy::check_path_traversal(path)?;
        let effective_scope = self.effective_path_scope(&tool.path_scope);
        match policy::check_path_scope(path, &effective_scope) {
            policy::PolicyResult::Allowed => Ok(()),
            policy::PolicyResult::Denied(msg) | policy::PolicyResult::NeedsApproval(msg) => {
                Err(ToolError {
                    code: "path_scope_denied".into(),
                    message: msg,
                    retryable: false,
                })
            }
        }
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

/// Extract the write paths a tool declares in its input for checkpoint capture.
/// write_file/edit_file use `path`; apply_patch uses `files[].path` plus the
/// optional top-level `path`. Entries are left unvalidated here — preflight
/// rejects invalid ones explicitly rather than silently dropping them.
fn declared_write_paths(name: &str, input: &serde_json::Value) -> Vec<String> {
    fn push_non_empty(out: &mut Vec<String>, value: Option<&str>) {
        if let Some(p) = value {
            if !p.is_empty() {
                out.push(p.to_string());
            }
        }
    }
    let mut paths = Vec::new();
    match name {
        "write_file" | "edit_file" => {
            push_non_empty(&mut paths, input.get("path").and_then(|v| v.as_str()));
        }
        "apply_patch" => {
            if let Some(files) = input.get("files").and_then(|v| v.as_array()) {
                for f in files {
                    push_non_empty(&mut paths, f.get("path").and_then(|v| v.as_str()));
                }
            }
            push_non_empty(&mut paths, input.get("path").and_then(|v| v.as_str()));
        }
        _ => {}
    }
    paths
}

/// Small, dependency-free Draft-07 subset used by the built-in manifests.
/// It covers the executable boundary (`type`, `properties`, `required`,
/// `additionalProperties`, `items`, `enum`, collection bounds, and numeric bounds) without
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
    if let Some(min_items) = schema.get("minItems").and_then(|v| v.as_u64()) {
        if let Some(array) = value.as_array() {
            if array.len() < min_items as usize {
                return Err(format!("array has fewer than {min_items} items"));
            }
        }
    }
    if let Some(max_items) = schema.get("maxItems").and_then(|v| v.as_u64()) {
        if let Some(array) = value.as_array() {
            if array.len() > max_items as usize {
                return Err(format!("array has more than {max_items} items"));
            }
        }
    }
    if let Some(min_length) = schema.get("minLength").and_then(|v| v.as_u64()) {
        if let Some(string) = value.as_str() {
            if string.chars().count() < min_length as usize {
                return Err(format!("string has fewer than {min_length} characters"));
            }
        }
    }
    if let Some(max_length) = schema.get("maxLength").and_then(|v| v.as_u64()) {
        if let Some(string) = value.as_str() {
            if string.chars().count() > max_length as usize {
                return Err(format!("string has more than {max_length} characters"));
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
    for keyword in ["minItems", "maxItems", "minLength", "maxLength"] {
        if let Some(value) = object.get(keyword) {
            if value.as_u64().is_none() {
                return Err(ToolError {
                    code: "invalid_schema".into(),
                    message: format!("{keyword} must be a non-negative integer"),
                    retryable: false,
                });
            }
        }
    }
    if let (Some(min), Some(max)) = (
        object.get("minItems").and_then(|value| value.as_u64()),
        object.get("maxItems").and_then(|value| value.as_u64()),
    ) {
        if min > max {
            return Err(ToolError {
                code: "invalid_schema".into(),
                message: "minItems cannot exceed maxItems".into(),
                retryable: false,
            });
        }
    }
    if let (Some(min), Some(max)) = (
        object.get("minLength").and_then(|value| value.as_u64()),
        object.get("maxLength").and_then(|value| value.as_u64()),
    ) {
        if min > max {
            return Err(ToolError {
                code: "invalid_schema".into(),
                message: "minLength cannot exceed maxLength".into(),
                retryable: false,
            });
        }
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
            parallel_safe: false,
            conflict_key: None,
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

    #[tokio::test]
    async fn array_bounds_are_enforced_before_handler() {
        let handler = Arc::new(CountingHandler(AtomicUsize::new(0)));
        let mut gateway = CapabilityGateway::new();
        gateway.register(Tool {
            name: "bounded_array",
            description: "test",
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {"type": "array", "minItems": 1, "maxItems": 2, "items": {"type": "string"}}
                },
                "required": ["items"]
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::Any,
            timeout_ms: 1000,
            output_limit: 4096,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: handler.clone(),
        });
        let error = gateway
            .execute(
                "bounded_array",
                serde_json::json!({"items": []}),
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
        gateway
            .validate_registered_schemas()
            .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    }

    #[test]
    fn every_tool_has_a_verifiable_mode_and_writes_are_not_parallel() {
        let mut gateway = CapabilityGateway::new();
        gateway.register_builtins();
        let capabilities = gateway.list_capabilities();
        assert!(!capabilities.is_empty(), "builtins must register");
        for capability in &capabilities {
            // Every tool has a real, explicit mode — never an inferred default
            // that could be mistaken for a missing declaration.
            match capability.execution_mode {
                ExecutionMode::ParallelSafe
                | ExecutionMode::Sequential
                | ExecutionMode::Exclusive => {}
            }
        }
        // Genuinely safe read-only file tools are explicitly parallel-safe.
        for name in ["read_file", "search_files", "list_dir", "grep"] {
            let cap = capabilities
                .iter()
                .find(|capability| capability.name == name)
                .unwrap_or_else(|| panic!("{name} must be registered"));
            assert_eq!(
                cap.execution_mode,
                ExecutionMode::ParallelSafe,
                "{name} must be explicitly parallel-safe"
            );
        }
        // write / shell / git / MCP / subagent tools must never be parallel.
        for name in [
            "write_file",
            "edit_file",
            "apply_patch",
            "run_terminal",
            "web_fetch",
            "task",
            "kill_task",
            "mcp_call",
            "write_draft_module",
            "rollback_draft_revision",
            "notification",
        ] {
            if let Some(cap) = capabilities
                .iter()
                .find(|capability| capability.name == name)
            {
                assert_ne!(
                    cap.execution_mode,
                    ExecutionMode::ParallelSafe,
                    "{name} must not be parallel-safe"
                );
            }
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
                context: &ToolCallContext,
            ) -> Result<ToolOutput, ToolError> {
                tokio::select! {
                    _ = context.cancel.cancelled() => Err(ToolError {
                        code: "cancelled".into(),
                        message: "fake handler observed cancellation".into(),
                        retryable: false,
                    }),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => Ok(ToolOutput {
                        result: serde_json::json!({}),
                        truncated: false,
                        duration_ms: 0,
                    }),
                }
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

#[cfg(test)]
mod path_scope_preflight_tests {
    //! TASK-001 (N01): `preflight_write_paths` is the single authority that
    //! authorizes write paths for checkpoint I/O. Absolute, dotdot and symlink
    //! escapes must be rejected BEFORE any path is returned as trusted.
    use super::*;
    use std::path::Path;

    struct NoopHandler;
    #[async_trait::async_trait]
    impl ToolHandler for NoopHandler {
        async fn execute(
            &self,
            _input: serde_json::Value,
            _context: &ToolCallContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput {
                result: serde_json::json!({}),
                truncated: false,
                duration_ms: 0,
            })
        }
    }

    fn gateway_with_root(root: &Path) -> CapabilityGateway {
        // Production binds project roots from verified identity (canonical);
        // mirror that here so macOS `/var` → `/private/var` symlinks resolve.
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut gateway = CapabilityGateway::new();
        gateway.set_project_root(root.to_string_lossy().into_owned());
        gateway.register_builtins();
        gateway
    }

    fn context_for(root: &Path) -> ToolCallContext {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        ToolCallContext::with_cancel(
            root,
            "run-path".into(),
            "conv-path".into(),
            "call-path".into(),
            "autonomous".into(),
            CancellationToken::new(),
        )
    }

    fn assert_path_rejection(err: ToolError) {
        assert!(
            matches!(
                err.code.as_str(),
                "path_traversal" | "path_scope_denied" | "PATH_ESCAPE"
            ),
            "expected a path rejection code, got {}: {}",
            err.code,
            err.message
        );
    }

    #[test]
    fn test_path_scope_preflight_rejects_absolute_system_path() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "/etc/passwd", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_absolute_outside_project() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "TOP-SECRET").unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": secret.to_string_lossy(), "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_dotdot() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "../escape.txt", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret.txt");
        std::fs::write(&target, "TOP-SECRET").unwrap();
        let link = root.path().join("evil-link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "evil-link", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_missing_path_with_symlink_parent() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join("evil-dir");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "evil-dir/new.txt", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_apply_patch_files_array_escape() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "apply_patch",
                &serde_json::json!({"files": [{"path": "/etc/passwd", "content": "x"}]}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_allows_project_relative() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let file = root.path().join("src").join("a.txt");
        std::fs::write(&file, "hi").unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let trusted = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "src/a.txt", "content": "x"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(trusted.len(), 1);
        assert_eq!(
            trusted[0].canonical,
            file.canonicalize().unwrap(),
            "trusted path must be canonical"
        );
        assert_eq!(
            trusted[0].project_relative,
            std::path::PathBuf::from("src/a.txt"),
            "trusted path must carry the project-relative form"
        );
    }

    #[test]
    fn test_path_scope_preflight_non_write_tool_returns_empty() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let trusted = gateway
            .preflight_write_paths("grep", &serde_json::json!({"pattern": "x"}), &ctx)
            .unwrap();
        assert!(trusted.is_empty());
    }

    #[test]
    fn test_path_scope_preflight_scope_none_is_denied() {
        let root = tempfile::tempdir().unwrap();
        let mut gateway = gateway_with_root(root.path());
        gateway.register(Tool {
            name: "no_path_tool",
            description: "test",
            schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::None,
            timeout_ms: 1000,
            output_limit: 4096,
            cancellable: false,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(NoopHandler),
        });
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths("no_path_tool", &serde_json::json!({"path": "a.txt"}), &ctx)
            .unwrap_err();
        assert_eq!(
            err.code, "path_scope_denied",
            "got {}: {}",
            err.code, err.message
        );
    }
}

#[cfg(test)]
mod progress_bounded_tests {
    //! TASK-007 (H03): live-output progress is bounded. Overflow is dropped at
    //! the producer and counted in `progress_dropped_bytes`, never buffered
    //! unboundedly, and never allowed to stall the handler.
    use super::*;
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc::error::TrySendError;

    #[tokio::test]
    async fn progress_channel_is_bounded_and_overflow_drops() {
        let capacity = 2usize;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ToolProgressChunk>(capacity);
        let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
        // Producer behavior mirrors tools/mod.rs run_terminal: try_send, and on
        // Full drop the chunk and count its bytes.
        for i in 0..100 {
            let chunk = ToolProgressChunk {
                stream: "out".into(),
                text: format!("line {i}"),
            };
            match tx.try_send(chunk) {
                Ok(()) => {}
                Err(TrySendError::Full(full)) => {
                    dropped.fetch_add(full.text.len() as u64, Ordering::Relaxed);
                }
                Err(TrySendError::Closed(_)) => break,
            }
        }
        // Slow consumer: the queue holds at most `capacity` — it never grows
        // with the producer.
        let mut drained = 0usize;
        while let Ok(chunk) = rx.try_recv() {
            let _ = chunk;
            drained += 1;
        }
        assert_eq!(drained, capacity, "queue is bounded at capacity");
        assert!(dropped.load(Ordering::Relaxed) > 0, "overflow is counted");
    }

    #[test]
    fn context_attaches_bounded_progress_and_counter() {
        let mut ctx = ToolCallContext::new(
            std::path::PathBuf::from("."),
            "r".into(),
            "c".into(),
            "t".into(),
            "full_access".into(),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel::<ToolProgressChunk>(4);
        let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
        ctx.set_progress(Some(tx), dropped.clone());
        assert!(ctx.progress.is_some());
        assert_eq!(ctx.progress_dropped_bytes.load(Ordering::Relaxed), 0);
    }
}
