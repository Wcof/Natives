//! The capability gateway — tool registry, schema validation, path-scope
//! enforcement and bounded execution.

use super::context::{ToolCallContext, TrustedPath};
use super::contract::{
    ExecutionMode, PathScope, SideEffect, Tool, ToolCapability, ToolError, ToolOutput,
};

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

    /// Register a tool. A duplicate canonical name is rejected (D01): the
    /// gateway's advertised surface must be deterministic, so the second
    /// registration fails instead of silently shadowing the first.
    pub fn register(&mut self, tool: Tool) -> Result<(), String> {
        if let Some(existing) = self.tools.iter().find(|t| t.name == tool.name) {
            return Err(format!(
                "duplicate tool name '{}' conflicts with existing registration (side_effect={:?})",
                tool.name, existing.side_effect
            ));
        }
        self.tools.push(tool);
        Ok(())
    }

    pub fn get_tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// The conflict key a tool is bound to, if any. Used by the daemon to hold
    /// a cross-run lease so the same key never overlaps (D02).
    pub fn conflict_key_for(&self, name: &str) -> Option<String> {
        self.tools
            .iter()
            .find(|t| t.name == name)
            .and_then(|t| t.conflict_key.clone())
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
    pub fn register_builtins(&mut self) -> Result<(), String> {
        let builtins = super::tools::builtin_tools();
        for tool in builtins {
            self.register(tool)?;
        }
        Ok(())
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

        if super::policy::check_output_limit(
            result.result.to_string().as_bytes(),
            tool.output_limit,
        ) {
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
                super::policy::check_command_injection(cmd)?;
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
        super::policy::check_path_traversal(path)?;
        let effective_scope = self.effective_path_scope(&tool.path_scope);
        match super::policy::check_path_scope(path, &effective_scope) {
            super::policy::PolicyResult::Allowed => Ok(()),
            super::policy::PolicyResult::Denied(msg)
            | super::policy::PolicyResult::NeedsApproval(msg) => Err(ToolError {
                code: "path_scope_denied".into(),
                message: msg,
                retryable: false,
            }),
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
