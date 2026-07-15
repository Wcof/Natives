//! runtime/native/capability.rs — Native 原子能力层
//!
//! 这个模块刻意把“能力目录”和“能力执行”分开：
//! - `CapabilityRegistry` 只保存 manifest、启停状态和 trait object。
//! - `CapabilityExecutor` 负责权限、hook、rule、执行、post-hook 的完整链路。
//! - 旧的 `execute_batch` 仍保留为兼容入口，AgentLoop 可以逐步迁移。

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Allow,
    Ask,
    Deny,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Ask
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityContext {
    pub session_id: String,
    pub working_dir: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub source: String,
    pub permission_mode: PermissionMode,
}

impl CapabilityContext {
    pub fn for_session(session_id: impl Into<String>, working_dir: Option<PathBuf>) -> Self {
        Self {
            session_id: session_id.into(),
            project_root: working_dir.clone(),
            working_dir,
            source: "native".into(),
            permission_mode: PermissionMode::Ask,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDecision {
    pub action: PermissionAction,
    pub reason: String,
    pub rule_source: String,
}

impl PermissionDecision {
    pub fn allow(reason: impl Into<String>, rule_source: impl Into<String>) -> Self {
        Self {
            action: PermissionAction::Allow,
            reason: reason.into(),
            rule_source: rule_source.into(),
        }
    }

    pub fn ask(reason: impl Into<String>, rule_source: impl Into<String>) -> Self {
        Self {
            action: PermissionAction::Ask,
            reason: reason.into(),
            rule_source: rule_source.into(),
        }
    }

    pub fn deny(reason: impl Into<String>, rule_source: impl Into<String>) -> Self {
        Self {
            action: PermissionAction::Deny,
            reason: reason.into(),
            rule_source: rule_source.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityVisibility {
    Public,
    RequiresApproval,
    Internal,
    Deprecated,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySideEffect {
    None,
    ReadFs,
    WriteFs,
    ExecuteProcess,
    Network,
    ModuleWrite,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPermission {
    Allow,
    Ask,
    Deny,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl CapabilityVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityExample {
    pub description: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEventMapping {
    pub hook_event: String,
    pub rule_event: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityMeta {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    /// Back-compat for existing AgentLoop code. Serialized as `parameters`.
    pub parameters: serde_json::Value,
    pub side_effects: Vec<CapabilitySideEffect>,
    pub has_side_effects: bool,
    pub permission: CapabilityPermission,
    pub default_enabled: bool,
    pub visibility: CapabilityVisibility,
    pub version: CapabilityVersion,
    pub depends_on: Vec<String>,
    pub group: String,
    pub timeout_ms: u64,
    pub examples: Vec<CapabilityExample>,
    pub source: String,
    pub event_mapping: CapabilityEventMapping,
    pub unsupported_reason: Option<String>,
}

impl CapabilityMeta {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        group: impl Into<String>,
    ) -> Self {
        let input_schema_for_parameters = input_schema.clone();
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            parameters: input_schema_for_parameters,
            side_effects: vec![CapabilitySideEffect::None],
            has_side_effects: false,
            permission: CapabilityPermission::Allow,
            default_enabled: true,
            visibility: CapabilityVisibility::Public,
            version: CapabilityVersion::new(1, 0, 0),
            depends_on: vec![],
            group: group.into(),
            timeout_ms: 0,
            examples: vec![],
            source: "native".into(),
            event_mapping: CapabilityEventMapping {
                hook_event: "PreToolUse".into(),
                rule_event: "all".into(),
            },
            unsupported_reason: None,
        }
    }

    pub fn with_side_effects(mut self, effects: Vec<CapabilitySideEffect>) -> Self {
        self.has_side_effects = effects.iter().any(|e| *e != CapabilitySideEffect::None);
        self.side_effects = effects;
        self
    }

    pub fn with_permission(mut self, permission: CapabilityPermission) -> Self {
        self.permission = permission;
        if self.permission == CapabilityPermission::Unsupported {
            self.default_enabled = false;
            self.visibility = CapabilityVisibility::Internal;
        }
        self
    }

    pub fn with_visibility(mut self, visibility: CapabilityVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn with_rule_event(mut self, rule_event: impl Into<String>) -> Self {
        self.event_mapping.rule_event = rule_event.into();
        self
    }

    pub fn unsupported(mut self, reason: impl Into<String>) -> Self {
        self.permission = CapabilityPermission::Unsupported;
        self.default_enabled = false;
        self.unsupported_reason = Some(reason.into());
        self
    }
}

#[derive(Clone, Debug)]
pub struct CapabilityRequest {
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub working_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityResult {
    pub call_id: String,
    pub status: CapabilityStatus,
    pub output: serde_json::Value,
    pub duration_ms: u64,
    pub permission: Option<PermissionDecision>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Success,
    Error,
    Rejected,
    CircuitBroken,
    TimedOut,
}

#[async_trait::async_trait]
pub trait AtomicCapability: Send + Sync {
    fn meta(&self) -> CapabilityMeta;

    async fn execute(
        &self,
        request: &CapabilityRequest,
        context: &CapabilityContext,
        cancellation: &CancellationToken,
    ) -> Result<serde_json::Value>;

    fn before_exec(
        &self,
        _request: &CapabilityRequest,
        _context: &CapabilityContext,
    ) -> std::result::Result<(), String> {
        Ok(())
    }

    fn after_exec(
        &self,
        _request: &CapabilityRequest,
        _context: &CapabilityContext,
        _result: &CapabilityResult,
    ) {
    }

    fn on_error(&self, _request: &CapabilityRequest, _context: &CapabilityContext, _error: &str) {}
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSettings {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub allow_managed_permission_rules_only: bool,
    #[serde(default)]
    pub allow_managed_hooks_only: bool,
    #[serde(default)]
    pub sandbox: serde_json::Value,
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self {
            allow: vec![],
            ask: vec!["Bash".into()],
            deny: vec![],
            allow_managed_permission_rules_only: false,
            allow_managed_hooks_only: false,
            sandbox: serde_json::json!({}),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PermissionEngine {
    settings: PermissionSettings,
}

impl PermissionEngine {
    pub fn new(settings: PermissionSettings) -> Self {
        Self { settings }
    }

    pub fn evaluate(
        &self,
        request: &CapabilityRequest,
        meta: &CapabilityMeta,
        context: &CapabilityContext,
    ) -> PermissionDecision {
        if matches_pattern_list(&self.settings.deny, &request.name) {
            return PermissionDecision::deny(
                "Denied by runtime permission settings",
                "settings.deny",
            );
        }
        if matches_pattern_list(&self.settings.allow, &request.name) {
            return PermissionDecision::allow(
                "Allowed by runtime permission settings",
                "settings.allow",
            );
        }
        if matches_pattern_list(&self.settings.ask, &request.name) {
            if context.permission_mode != PermissionMode::Allow {
                return PermissionDecision::ask(
                    "Requires approval by runtime permission settings",
                    "settings.ask",
                );
            }
        }

        match meta.permission {
            CapabilityPermission::Deny | CapabilityPermission::Unsupported => {
                PermissionDecision::deny(
                    "Capability is disabled or unsupported",
                    "capability.default",
                )
            }
            CapabilityPermission::Ask => match context.permission_mode {
                PermissionMode::Allow => {
                    PermissionDecision::allow("Auto-allowed by context permission mode", "context")
                }
                PermissionMode::Deny => {
                    PermissionDecision::deny("Denied by read-only context", "context")
                }
                PermissionMode::Ask => {
                    PermissionDecision::ask("Capability requires approval", "capability.default")
                }
            },
            CapabilityPermission::Allow => {
                PermissionDecision::allow("Capability is allowed", "capability.default")
            }
        }
    }
}

fn matches_pattern_list(patterns: &[String], name: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| capability_pattern_matches(pattern, name))
}

fn capability_pattern_matches(pattern: &str, name: &str) -> bool {
    let normalized = legacy_to_claude_tool_name(name);
    pattern == name
        || pattern == normalized
        || pattern == "*"
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| name.starts_with(prefix) || normalized.starts_with(prefix))
        || pattern
            .strip_prefix('*')
            .is_some_and(|suffix| name.ends_with(suffix) || normalized.ends_with(suffix))
}

pub struct CapabilityRegistry {
    capabilities: HashMap<String, Arc<dyn AtomicCapability>>,
    aliases: HashMap<String, String>,
    enabled: HashMap<String, bool>,
    groups: HashMap<String, Vec<String>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
            aliases: default_aliases(),
            enabled: HashMap::new(),
            groups: HashMap::new(),
        }
    }

    pub fn register(&mut self, cap: Arc<dyn AtomicCapability>) {
        let meta = cap.meta();
        let name = meta.name.clone();
        self.enabled
            .entry(name.clone())
            .or_insert(meta.default_enabled);
        self.groups
            .entry(meta.group.clone())
            .or_default()
            .push(name.clone());
        self.capabilities.insert(name, cap);
    }

    pub fn register_all(&mut self, caps: Vec<Arc<dyn AtomicCapability>>) {
        for cap in caps {
            self.register(cap);
        }
    }

    pub fn register_alias(&mut self, alias: impl Into<String>, canonical: impl Into<String>) {
        self.aliases.insert(alias.into(), canonical.into());
    }

    pub fn resolve_name(&self, name: &str) -> String {
        self.aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn AtomicCapability>> {
        let canonical = self.resolve_name(name);
        self.capabilities.get(&canonical)
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        let canonical = self.resolve_name(name);
        self.enabled.get(&canonical).copied().unwrap_or(false)
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        let canonical = self.resolve_name(name);
        self.enabled.insert(canonical, enabled);
    }

    pub fn list_metadata(&self) -> Vec<CapabilityMeta> {
        let mut metas: Vec<_> = self.capabilities.values().map(|c| c.meta()).collect();
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        metas
    }

    pub fn list_visible_tools(&self) -> Vec<CapabilityMeta> {
        self.list_metadata()
            .into_iter()
            .filter(|m| {
                (m.visibility == CapabilityVisibility::Public
                    || m.visibility == CapabilityVisibility::RequiresApproval)
                    && self.is_enabled(&m.name)
                    && m.permission != CapabilityPermission::Unsupported
            })
            .collect()
    }

    pub fn list_by_group(&self) -> HashMap<String, Vec<CapabilityMeta>> {
        let mut grouped: HashMap<String, Vec<CapabilityMeta>> = HashMap::new();
        for meta in self.list_metadata() {
            grouped.entry(meta.group.clone()).or_default().push(meta);
        }
        grouped
    }

    pub fn count(&self) -> usize {
        self.capabilities.len()
    }

    pub fn execute_batch(
        &self,
        requests: &[CapabilityRequest],
        hook_pipeline: Option<&super::hook_pipeline::HookPipeline>,
        rule_engine: Option<&super::rule_engine::RuleEngine>,
        session_id: &str,
    ) -> Vec<CapabilityResult> {
        let executor = CapabilityExecutor::new(self, PermissionEngine::default());
        let context = CapabilityContext::for_session(session_id, None);
        let cancellation = CancellationToken::new();
        let handle = tokio::runtime::Handle::try_current();
        match handle {
            Ok(handle) => handle.block_on(async {
                let mut results = Vec::with_capacity(requests.len());
                for req in requests {
                    results.push(
                        executor
                            .execute(req, &context, hook_pipeline, rule_engine, &cancellation)
                            .await,
                    );
                }
                results
            }),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .expect("create runtime for capability execution");
                runtime.block_on(async {
                    let mut results = Vec::with_capacity(requests.len());
                    for req in requests {
                        results.push(
                            executor
                                .execute(req, &context, hook_pipeline, rule_engine, &cancellation)
                                .await,
                        );
                    }
                    results
                })
            }
        }
    }
}

pub struct CapabilityExecutor<'a> {
    registry: &'a CapabilityRegistry,
    permission_engine: PermissionEngine,
}

impl<'a> CapabilityExecutor<'a> {
    pub fn new(registry: &'a CapabilityRegistry, permission_engine: PermissionEngine) -> Self {
        Self {
            registry,
            permission_engine,
        }
    }

    pub async fn execute(
        &self,
        request: &CapabilityRequest,
        context: &CapabilityContext,
        hook_pipeline: Option<&super::hook_pipeline::HookPipeline>,
        rule_engine: Option<&super::rule_engine::RuleEngine>,
        cancellation: &CancellationToken,
    ) -> CapabilityResult {
        let start = Instant::now();
        let canonical_name = self.registry.resolve_name(&request.name);
        let Some(capability) = self.registry.capabilities.get(&canonical_name) else {
            return result(
                request,
                CapabilityStatus::Error,
                serde_json::json!({
                    "error": "unknown_capability",
                    "message": format!("Unknown capability: '{}'", request.name),
                }),
                start,
                None,
            );
        };

        let meta = capability.meta();
        let mut canonical_request = request.clone();
        canonical_request.name = canonical_name;
        if canonical_request.working_dir.is_none() {
            canonical_request.working_dir = context.working_dir.clone();
        }

        if !self.registry.is_enabled(&meta.name) {
            return result(
                &canonical_request,
                CapabilityStatus::Rejected,
                serde_json::json!({
                    "error": "capability_disabled",
                    "message": format!("Capability '{}' is disabled", meta.name),
                }),
                start,
                None,
            );
        }

        let permission = self
            .permission_engine
            .evaluate(&canonical_request, &meta, context);
        if permission.action == PermissionAction::Deny
            || (permission.action == PermissionAction::Ask && meta.has_side_effects)
        {
            return result(
                &canonical_request,
                CapabilityStatus::Rejected,
                serde_json::json!({
                    "error": "permission_rejected",
                    "message": permission.reason,
                    "ruleSource": permission.rule_source,
                }),
                start,
                Some(permission),
            );
        }

        if let Some(pipeline) = hook_pipeline {
            match pipeline.before_tool(&canonical_request, &context.session_id) {
                Err(rejection) => {
                    return result(
                        &canonical_request,
                        CapabilityStatus::Rejected,
                        serde_json::json!({
                            "error": "hook_rejected",
                            "message": rejection,
                        }),
                        start,
                        Some(permission),
                    );
                }
                Ok(modified_args) => {
                    if !modified_args.is_null() {
                        canonical_request.arguments = modified_args;
                    }
                }
            }
        }

        if let Some(engine) = rule_engine {
            let evaluation = engine.evaluate(&canonical_request);
            match evaluation.action {
                super::rule_engine::RuleAction::Block => {
                    return result(
                        &canonical_request,
                        CapabilityStatus::Rejected,
                        serde_json::json!({
                            "error": "rule_blocked",
                            "rule": evaluation.matched_rule,
                            "message": evaluation.message,
                        }),
                        start,
                        Some(permission),
                    );
                }
                super::rule_engine::RuleAction::Warn => {
                    eprintln!(
                        "[CapabilityExecutor] warning from {:?}: {}",
                        evaluation.matched_rule, evaluation.message
                    );
                }
                super::rule_engine::RuleAction::Allow => {}
            }
        }

        if cancellation.is_cancelled() {
            return result(
                &canonical_request,
                CapabilityStatus::Rejected,
                serde_json::json!({"error": "cancelled"}),
                start,
                Some(permission),
            );
        }

        if let Err(rejection) = capability.before_exec(&canonical_request, context) {
            return result(
                &canonical_request,
                CapabilityStatus::Rejected,
                serde_json::json!({
                    "error": "capability_rejected",
                    "message": rejection,
                }),
                start,
                Some(permission),
            );
        }

        let exec = capability.execute(&canonical_request, context, cancellation);
        let exec_result = if meta.timeout_ms > 0 {
            match tokio::time::timeout(std::time::Duration::from_millis(meta.timeout_ms), exec)
                .await
            {
                Ok(outcome) => outcome,
                Err(_) => Err(Error::Internal(format!(
                    "Capability '{}' timed out after {}ms",
                    meta.name, meta.timeout_ms
                ))),
            }
        } else {
            exec.await
        };

        let mut capability_result = match exec_result {
            Ok(output) => result(
                &canonical_request,
                CapabilityStatus::Success,
                output,
                start,
                Some(permission.clone()),
            ),
            Err(err) => {
                let message = err.to_string();
                capability.on_error(&canonical_request, context, &message);
                result(
                    &canonical_request,
                    CapabilityStatus::Error,
                    serde_json::json!({
                        "error": "execution_failed",
                        "message": message,
                    }),
                    start,
                    Some(permission.clone()),
                )
            }
        };

        capability.after_exec(&canonical_request, context, &capability_result);
        if let Some(pipeline) = hook_pipeline {
            pipeline.after_tool(
                &canonical_request,
                &mut capability_result,
                &context.session_id,
            );
        }
        capability_result
    }
}

fn result(
    request: &CapabilityRequest,
    status: CapabilityStatus,
    output: serde_json::Value,
    start: Instant,
    permission: Option<PermissionDecision>,
) -> CapabilityResult {
    CapabilityResult {
        call_id: request.call_id.clone(),
        status,
        output,
        duration_ms: start.elapsed().as_millis() as u64,
        permission,
    }
}

fn default_aliases() -> HashMap<String, String> {
    HashMap::from([
        ("read_file".into(), "Read".into()),
        ("list_dir".into(), "LS".into()),
        ("write_file".into(), "Write".into()),
        ("run_terminal".into(), "Bash".into()),
        ("lint_module".into(), "lint_module".into()),
        ("write_module".into(), "write_module".into()),
    ])
}

pub fn legacy_to_claude_tool_name(name: &str) -> &str {
    match name {
        "read_file" => "Read",
        "list_dir" => "LS",
        "write_file" => "Write",
        "run_terminal" => "Bash",
        other => other,
    }
}

pub fn create_default_capabilities(modules_dir: &Path) -> Vec<Arc<dyn AtomicCapability>> {
    vec![
        Arc::new(crate::assistant_executor::ReadFileCapability::new()),
        Arc::new(crate::assistant_executor::ListDirCapability::new()),
        Arc::new(crate::assistant_executor::WriteFileCapability::new()),
        Arc::new(EditCapability),
        Arc::new(MultiEditCapability),
        Arc::new(GlobCapability),
        Arc::new(GrepCapability),
        Arc::new(crate::assistant_executor::WriteModuleCapability::new(
            modules_dir.to_path_buf(),
        )),
        Arc::new(crate::assistant_executor::RunTerminalCapability::new()),
        Arc::new(crate::assistant_executor::LintModuleCapability::new()),
        Arc::new(UnsupportedManifestCapability::new(
            "TodoWrite",
            "Workflow todo tracking is exposed in the catalog but not executable yet.",
        )),
        Arc::new(UnsupportedManifestCapability::new(
            "NotebookRead",
            "Notebook support is not enabled in Natives Native Runtime yet.",
        )),
        Arc::new(UnsupportedManifestCapability::new(
            "NotebookEdit",
            "Notebook support is not enabled in Natives Native Runtime yet.",
        )),
        Arc::new(UnsupportedManifestCapability::new(
            "WebSearch",
            "Network search is disabled in first-stage Native Runtime replication.",
        )),
        Arc::new(UnsupportedManifestCapability::new(
            "WebFetch",
            "Network fetch is disabled in first-stage Native Runtime replication.",
        )),
    ]
}

pub struct EditCapability;

#[async_trait::async_trait]
impl AtomicCapability for EditCapability {
    fn meta(&self) -> CapabilityMeta {
        CapabilityMeta::new(
            "Edit",
            "Replace a single exact text occurrence in a local file.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
            "file",
        )
        .with_side_effects(vec![CapabilitySideEffect::WriteFs])
        .with_permission(CapabilityPermission::Ask)
        .with_rule_event("file")
    }

    async fn execute(
        &self,
        request: &CapabilityRequest,
        _context: &CapabilityContext,
        cancellation: &CancellationToken,
    ) -> Result<serde_json::Value> {
        if cancellation.is_cancelled() {
            return Err(Error::Internal("cancelled".into()));
        }
        let path = path_arg(&request.arguments, "file_path")
            .or_else(|| path_arg(&request.arguments, "path"))
            .ok_or_else(|| Error::InvalidInput("missing file_path".into()))?;
        let old = string_arg(&request.arguments, "old_string")
            .or_else(|| string_arg(&request.arguments, "oldText"))
            .ok_or_else(|| Error::InvalidInput("missing old_string".into()))?;
        let new = string_arg(&request.arguments, "new_string")
            .or_else(|| string_arg(&request.arguments, "newText"))
            .ok_or_else(|| Error::InvalidInput("missing new_string".into()))?;
        let content = std::fs::read_to_string(&path)?;
        let count = content.matches(&old).count();
        if count != 1 {
            return Err(Error::InvalidInput(format!(
                "Edit expected exactly one match, found {count}"
            )));
        }
        let updated = content.replacen(&old, &new, 1);
        crate::module_manager::atomic_write(Path::new(&path), &updated)?;
        Ok(serde_json::json!({ "path": path, "replacements": 1 }))
    }
}

pub struct MultiEditCapability;

#[async_trait::async_trait]
impl AtomicCapability for MultiEditCapability {
    fn meta(&self) -> CapabilityMeta {
        CapabilityMeta::new(
            "MultiEdit",
            "Apply multiple exact text replacements to a local file.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": { "type": "string" },
                                "new_string": { "type": "string" }
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["file_path", "edits"]
            }),
            "file",
        )
        .with_side_effects(vec![CapabilitySideEffect::WriteFs])
        .with_permission(CapabilityPermission::Ask)
        .with_rule_event("file")
    }

    async fn execute(
        &self,
        request: &CapabilityRequest,
        _context: &CapabilityContext,
        cancellation: &CancellationToken,
    ) -> Result<serde_json::Value> {
        if cancellation.is_cancelled() {
            return Err(Error::Internal("cancelled".into()));
        }
        let path = path_arg(&request.arguments, "file_path")
            .or_else(|| path_arg(&request.arguments, "path"))
            .ok_or_else(|| Error::InvalidInput("missing file_path".into()))?;
        let edits = request
            .arguments
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::InvalidInput("missing edits".into()))?;
        let mut content = std::fs::read_to_string(&path)?;
        let mut replacements = 0usize;
        for edit in edits {
            let old = edit
                .get("old_string")
                .or_else(|| edit.get("oldText"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::InvalidInput("missing edits[].old_string".into()))?;
            let new = edit
                .get("new_string")
                .or_else(|| edit.get("newText"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::InvalidInput("missing edits[].new_string".into()))?;
            let count = content.matches(old).count();
            if count != 1 {
                return Err(Error::InvalidInput(format!(
                    "MultiEdit expected exactly one match for edit {}, found {count}",
                    replacements + 1
                )));
            }
            content = content.replacen(old, new, 1);
            replacements += 1;
        }
        crate::module_manager::atomic_write(Path::new(&path), &content)?;
        Ok(serde_json::json!({ "path": path, "replacements": replacements }))
    }
}

pub struct GlobCapability;

#[async_trait::async_trait]
impl AtomicCapability for GlobCapability {
    fn meta(&self) -> CapabilityMeta {
        CapabilityMeta::new(
            "Glob",
            "Find files by glob pattern under the working directory.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"]
            }),
            "file",
        )
        .with_side_effects(vec![CapabilitySideEffect::ReadFs])
    }

    async fn execute(
        &self,
        request: &CapabilityRequest,
        context: &CapabilityContext,
        _cancellation: &CancellationToken,
    ) -> Result<serde_json::Value> {
        let pattern = string_arg(&request.arguments, "pattern")
            .ok_or_else(|| Error::InvalidInput("missing pattern".into()))?;
        let base = request
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| context.working_dir.clone())
            .unwrap_or_else(|| PathBuf::from("."));
        let full_pattern = base.join(pattern).to_string_lossy().to_string();
        let mut paths = vec![];
        for entry in glob::glob(&full_pattern).map_err(|e| Error::InvalidInput(e.to_string()))? {
            let path = match entry {
                Ok(path) => path,
                Err(_) => continue,
            };
            if is_skipped_path(&path) {
                continue;
            }
            paths.push(path.to_string_lossy().to_string());
            if paths.len() >= 500 {
                break;
            }
        }
        paths.sort();
        Ok(serde_json::json!({ "paths": paths }))
    }
}

pub struct GrepCapability;

#[async_trait::async_trait]
impl AtomicCapability for GrepCapability {
    fn meta(&self) -> CapabilityMeta {
        CapabilityMeta::new(
            "Grep",
            "Search text files under the working directory with a regex pattern.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "glob": { "type": "string" }
                },
                "required": ["pattern"]
            }),
            "file",
        )
        .with_side_effects(vec![CapabilitySideEffect::ReadFs])
    }

    async fn execute(
        &self,
        request: &CapabilityRequest,
        context: &CapabilityContext,
        _cancellation: &CancellationToken,
    ) -> Result<serde_json::Value> {
        let pattern = string_arg(&request.arguments, "pattern")
            .ok_or_else(|| Error::InvalidInput("missing pattern".into()))?;
        let regex = regex::Regex::new(&pattern).map_err(|e| Error::InvalidInput(e.to_string()))?;
        let base = request
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| context.working_dir.clone())
            .unwrap_or_else(|| PathBuf::from("."));
        let glob_filter = request.arguments.get("glob").and_then(|v| v.as_str());
        let mut matches = vec![];
        for entry in walkdir::WalkDir::new(&base)
            .max_depth(8)
            .into_iter()
            .filter_entry(|entry| !is_skipped_path(entry.path()))
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if let Some(filter) = glob_filter {
                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !simple_glob_match(filter, file_name) {
                    continue;
                }
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            for (idx, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(serde_json::json!({
                        "path": path.to_string_lossy(),
                        "line": idx + 1,
                        "text": line,
                    }));
                    if matches.len() >= 500 {
                        return Ok(serde_json::json!({ "matches": matches }));
                    }
                }
            }
        }
        Ok(serde_json::json!({ "matches": matches }))
    }
}

pub struct UnsupportedManifestCapability {
    name: String,
    reason: String,
}

impl UnsupportedManifestCapability {
    pub fn new(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            reason: reason.into(),
        }
    }
}

#[async_trait::async_trait]
impl AtomicCapability for UnsupportedManifestCapability {
    fn meta(&self) -> CapabilityMeta {
        CapabilityMeta::new(
            self.name.clone(),
            self.reason.clone(),
            serde_json::json!({ "type": "object", "properties": {} }),
            "workflow",
        )
        .with_permission(CapabilityPermission::Unsupported)
        .unsupported(self.reason.clone())
    }

    async fn execute(
        &self,
        _request: &CapabilityRequest,
        _context: &CapabilityContext,
        _cancellation: &CancellationToken,
    ) -> Result<serde_json::Value> {
        Err(Error::NotImplemented(self.reason.clone()))
    }
}

fn path_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn string_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn is_skipped_path(path: &Path) -> bool {
    path.components().any(|component| {
        let s = component.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            ".git" | "node_modules" | "target" | ".next" | ".codegraph" | ".natives"
        )
    })
}

fn simple_glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return text.ends_with(&format!(".{suffix}"));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return text.ends_with(suffix);
    }
    pattern == text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_registers_manifest_with_claude_shape() {
        let mut registry = CapabilityRegistry::new();
        registry.register(Arc::new(GlobCapability));

        let meta = registry.list_metadata().remove(0);

        assert_eq!(meta.name, "Glob");
        assert!(meta.input_schema.is_object());
        assert_eq!(meta.parameters, meta.input_schema);
        assert_eq!(meta.source, "native");
    }

    #[test]
    fn registry_keeps_legacy_aliases() {
        let mut registry = CapabilityRegistry::new();
        registry.register(Arc::new(GlobCapability));
        registry.register_alias("legacy_glob", "Glob");

        assert!(registry.get("legacy_glob").is_some());
    }

    #[test]
    fn permission_engine_denies_before_execution() {
        let engine = PermissionEngine::new(PermissionSettings {
            deny: vec!["Bash".into()],
            ..Default::default()
        });
        let meta = CapabilityMeta::new("Bash", "Run command", serde_json::json!({}), "terminal")
            .with_side_effects(vec![CapabilitySideEffect::ExecuteProcess])
            .with_permission(CapabilityPermission::Ask);
        let req = CapabilityRequest {
            call_id: "1".into(),
            name: "Bash".into(),
            arguments: serde_json::json!({}),
            working_dir: None,
        };
        let context = CapabilityContext::for_session("s", None);

        let decision = engine.evaluate(&req, &meta, &context);

        assert_eq!(decision.action, PermissionAction::Deny);
        assert_eq!(decision.rule_source, "settings.deny");
    }

    #[test]
    fn read_only_context_denies_ask_capabilities() {
        let engine = PermissionEngine::default();
        let meta = CapabilityMeta::new("Write", "Write file", serde_json::json!({}), "filesystem")
            .with_permission(CapabilityPermission::Ask);
        let request = CapabilityRequest {
            call_id: "1".into(),
            name: "Write".into(),
            arguments: serde_json::json!({}),
            working_dir: None,
        };
        let mut context = CapabilityContext::for_session("s", None);
        context.permission_mode = PermissionMode::Deny;

        assert_eq!(
            engine.evaluate(&request, &meta, &context).action,
            PermissionAction::Deny
        );
    }

    #[test]
    fn approved_context_overrides_default_ask_rule() {
        let engine = PermissionEngine::default();
        let meta = CapabilityMeta::new("Bash", "Run command", serde_json::json!({}), "terminal")
            .with_permission(CapabilityPermission::Ask);
        let request = CapabilityRequest { call_id: "1".into(), name: "Bash".into(), arguments: serde_json::json!({}), working_dir: None };
        let mut context = CapabilityContext::for_session("s", None);
        context.permission_mode = PermissionMode::Allow;

        assert_eq!(engine.evaluate(&request, &meta, &context).action, PermissionAction::Allow);
    }

    #[test]
    fn default_capabilities_include_claude_and_natives_tools() {
        let mut registry = CapabilityRegistry::new();
        registry.register_all(create_default_capabilities(Path::new("/tmp")));
        let names: Vec<String> = registry
            .list_metadata()
            .into_iter()
            .map(|m| m.name)
            .collect();

        assert!(names.contains(&"Read".into()));
        assert!(names.contains(&"Write".into()));
        assert!(names.contains(&"Edit".into()));
        assert!(names.contains(&"MultiEdit".into()));
        assert!(names.contains(&"LS".into()));
        assert!(names.contains(&"Glob".into()));
        assert!(names.contains(&"Grep".into()));
        assert!(names.contains(&"Bash".into()));
        assert!(names.contains(&"write_module".into()));
        assert!(names.contains(&"TodoWrite".into()));
    }

    #[test]
    fn unsupported_manifest_is_disabled() {
        let cap = UnsupportedManifestCapability::new("TodoWrite", "not yet");
        let meta = cap.meta();

        assert_eq!(meta.permission, CapabilityPermission::Unsupported);
        assert!(!meta.default_enabled);
        assert!(meta.unsupported_reason.is_some());
    }
}
