//! The Harness Blueprint — the only user-editable part of the Harness.
//!
//! A Blueprint is a **typed sparse overlay**, never a JSON merge patch. It
//! never restates a Hook; it names one by [`HookId`] and changes named fields.
//!
//! Schema v3 adds complete Natives-owned `NativeHookSpecV3` definitions with 5
//! handler categories (Command, HTTP, MCP Tool, Prompt Assessment, Agent Sub-run)
//! and Natives-owned `PromptBlockSpecV3` prompt blocks.

use crate::hooks::{Condition, HookEvent, HookFailurePolicy, HookId, HookKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Bumped when the Blueprint schema itself changes shape.
pub const BLUEPRINT_SCHEMA_VERSION: u32 = 3;

/// Which Hook dispatch semantics a published version commits to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSemanticsVersion {
    #[default]
    LegacyV1,
    SequentialV2,
}

impl HookSemanticsVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyV1 => "legacy_v1",
            Self::SequentialV2 => "sequential_v2",
        }
    }
}

/// Prompt plan assembly semantics version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSemanticsVersion {
    #[default]
    LegacyV1,
    SequentialV2,
}

impl PromptSemanticsVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyV1 => "legacy_v1",
            Self::SequentialV2 => "sequential_v2",
        }
    }
}

/// A sparse per-Hook overlay for discovered/external Hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookOverlay {
    pub hook_id: HookId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_policy: Option<HookFailurePolicy>,
}

impl HookOverlay {
    pub fn new(hook_id: HookId) -> Self {
        Self {
            hook_id,
            enabled: None,
            order: None,
            matcher: None,
            timeout_ms: None,
            failure_policy: None,
        }
    }

    pub fn set_fields(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.enabled.is_some() {
            out.push("enabled");
        }
        if self.order.is_some() {
            out.push("order");
        }
        if self.matcher.is_some() {
            out.push("matcher");
        }
        if self.timeout_ms.is_some() {
            out.push("timeout_ms");
        }
        if self.failure_policy.is_some() {
            out.push("failure_policy");
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.set_fields().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandWorkingDirPolicy {
    #[default]
    ProjectRoot,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandMode {
    #[default]
    Exec,
    Shell,
}

/// Tagged union for 5 categories of Native Hook Adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookAdapterSpecV3 {
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        working_dir_policy: CommandWorkingDirPolicy,
        #[serde(default)]
        secret_env_refs: BTreeMap<String, String>,
        #[serde(default)]
        trusted: bool,
        #[serde(default)]
        mode: CommandMode,
    },
    Http {
        url: String,
        #[serde(default)]
        allow_hosts: Vec<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        secret_header_refs: BTreeMap<String, String>,
    },
    McpTool {
        server_id: String,
        tool_name: String,
        #[serde(default)]
        input_template: Option<String>,
    },
    Prompt {
        template: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_override: Option<String>,
    },
    Agent {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_override: Option<String>,
        #[serde(default = "default_max_steps")]
        max_steps: u32,
        #[serde(default)]
        readonly_tools: Vec<String>,
    },
}

impl HookAdapterSpecV3 {
    pub fn to_hook_kind(&self) -> HookKind {
        match self {
            Self::Command {
                program,
                args,
                trusted,
                ..
            } => HookKind::Command {
                program: program.clone(),
                args: args.clone(),
                trusted: *trusted,
            },
            Self::Http {
                url, allow_hosts, ..
            } => HookKind::Http {
                url: url.clone(),
                allow_hosts: allow_hosts.clone(),
            },
            Self::McpTool { tool_name, .. } => HookKind::Builtin {
                name: format!("mcp:{tool_name}"),
            },
            Self::Prompt { template, .. } => HookKind::Builtin {
                name: format!("prompt:{}", template.chars().take(20).collect::<String>()),
            },
            Self::Agent { prompt, .. } => HookKind::Builtin {
                name: format!("agent:{}", prompt.chars().take(20).collect::<String>()),
            },
        }
    }
}

fn default_max_steps() -> u32 {
    5
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    10_000
}

/// Complete Natives-owned Hook definition in Schema v3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHookSpecV3 {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub event: HookEvent,
    #[serde(default)]
    pub order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub failure_policy: HookFailurePolicy,
    pub adapter: HookAdapterSpecV3,
    #[serde(default)]
    pub trust_confirmed: bool,
}

pub type NativeHookSpec = NativeHookSpecV3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptBlockPlacement {
    #[default]
    AfterProjectInstructions,
    BeforeProfile,
    AfterProfile,
    Final,
}

/// Natives-owned system-prompt block fragment in Schema v3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptBlockSpecV3 {
    pub id: String,
    pub name: String,
    pub markdown: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub placement: PromptBlockPlacement,
}

pub type PromptBlock = PromptBlockSpecV3;

/// One layer of Harness configuration document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessBlueprint {
    pub schema_version: u32,
    #[serde(default)]
    pub hook_semantics_version: HookSemanticsVersion,
    #[serde(default)]
    pub prompt_semantics_version: PromptSemanticsVersion,
    #[serde(default)]
    pub hooks: Vec<HookOverlay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_overlays: Vec<HookOverlay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_hooks: Vec<NativeHookSpecV3>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_blocks: Vec<PromptBlockSpecV3>,
}

impl Default for HarnessBlueprint {
    fn default() -> Self {
        Self {
            schema_version: BLUEPRINT_SCHEMA_VERSION,
            hook_semantics_version: HookSemanticsVersion::LegacyV1,
            prompt_semantics_version: PromptSemanticsVersion::LegacyV1,
            hooks: Vec::new(),
            hook_overlays: Vec::new(),
            native_hooks: Vec::new(),
            prompt_blocks: Vec::new(),
        }
    }
}

impl HarnessBlueprint {
    /// Parse strictly, validating schema versions 1..=3.
    pub fn parse(value: &Value) -> Result<Self, String> {
        let parsed: Self = serde_json::from_value(value.clone())
            .map_err(|e| format!("blueprint is not a valid document: {e}"))?;
        parsed.check_shape()?;
        Ok(parsed)
    }

    fn check_shape(&self) -> Result<(), String> {
        if self.schema_version < 1 || self.schema_version > BLUEPRINT_SCHEMA_VERSION {
            return Err(format!(
                "blueprint schema_version {} is not supported (this daemon speaks 1..={})",
                self.schema_version, BLUEPRINT_SCHEMA_VERSION
            ));
        }
        let mut seen = BTreeSet::new();
        for overlay in self.overlays() {
            if !seen.insert(overlay.hook_id.clone()) {
                return Err(format!(
                    "duplicate overlay for hook {}: a document must state each Hook at most once",
                    overlay.hook_id
                ));
            }
        }
        let mut native_ids = BTreeSet::new();
        for hook in &self.native_hooks {
            let parsed = uuid::Uuid::parse_str(&hook.id)
                .map_err(|_| format!("native hook id must be a UUID: {}", hook.id))?;
            if !native_ids.insert(parsed) {
                return Err(format!("duplicate native hook id: {}", hook.id));
            }
        }
        let mut block_ids = BTreeSet::new();
        for block in &self.prompt_blocks {
            let parsed = uuid::Uuid::parse_str(&block.id)
                .map_err(|_| format!("prompt block id must be a UUID: {}", block.id))?;
            if !block_ids.insert(parsed) {
                return Err(format!("duplicate prompt block id: {}", block.id));
            }
        }
        Ok(())
    }

    pub fn overlays(&self) -> &[HookOverlay] {
        if self.hook_overlays.is_empty() {
            &self.hooks
        } else {
            &self.hook_overlays
        }
    }

    pub fn overlay_for(&self, hook_id: &HookId) -> Option<&HookOverlay> {
        self.overlays().iter().find(|o| &o.hook_id == hook_id)
    }

    pub fn canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).unwrap_or(Value::Null))
    }

    pub fn canonical_hash(&self) -> String {
        sha256_hex(&self.canonical_json())
    }
}

pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            let body: Vec<String> = sorted
                .into_iter()
                .map(|(k, v)| format!("{}:{}", Value::String(k.clone()), canonical_json(v)))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", body.join(","))
        }
        other => other.to_string(),
    }
}

pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_document_is_empty_and_legacy() {
        let doc = HarnessBlueprint::default();
        assert!(doc.hooks.is_empty());
        assert_eq!(doc.hook_semantics_version, HookSemanticsVersion::LegacyV1);
        assert_eq!(doc.schema_version, BLUEPRINT_SCHEMA_VERSION);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let err = HarnessBlueprint::parse(&json!({
            "schema_version": 3,
            "hooks": [],
            "concurrency": 8
        }))
        .unwrap_err();
        assert!(err.contains("concurrency"), "got: {err}");
    }

    #[test]
    fn native_hook_v3_deserializes() {
        let json_data = json!({
            "schema_version": 3,
            "native_hooks": [{
                "id": "11111111-1111-1111-1111-111111111111",
                "name": "Audit Hook",
                "enabled": true,
                "event": "PreToolUse",
                "order": 1,
                "matcher": "Bash",
                "conditions": [],
                "timeout_ms": 5000,
                "failure_policy": "fail",
                "adapter": {
                    "type": "command",
                    "program": "/usr/bin/security_check",
                    "args": ["--verbose"],
                    "trusted": true
                }
            }],
            "prompt_blocks": [{
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Project Guidelines",
                "markdown": "# Rule\nDo not bypass checks.",
                "enabled": true,
                "order": 0,
                "placement": "after_project_instructions"
            }]
        });
        let bp = HarnessBlueprint::parse(&json_data).expect("should parse v3 blueprint");
        assert_eq!(bp.native_hooks.len(), 1);
        assert_eq!(bp.prompt_blocks.len(), 1);
    }
}
