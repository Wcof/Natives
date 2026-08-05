//! Draft validation and version diffing.
//!
//! Validation is the gate in front of publish (design 第 9.2 节): a Draft that
//! does not pass never becomes an immutable version, and the currently
//! published version is left untouched. Diff is what makes a publish
//! reviewable — a user is asked to approve a change, not a blob.
//!
//! Both work on documents plus the *discovered* Hook set, because half the
//! interesting mistakes (overlaying a locked Hook, naming a Hook that is not
//! there) are only visible against real discovery.

use crate::blueprint::{CommandMode, HarnessBlueprint, HookAdapterSpecV3, HookOverlay};
use crate::hooks::{HookDefinition, HookId};
use crate::resolver::is_locked;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Smallest and largest Hook timeout a document may set, in milliseconds.
///
/// Matches the clamp `production_hooks` already applies when reading
/// `hooks.json`, so a value accepted here cannot be silently rewritten later.
pub const MIN_TIMEOUT_MS: u64 = 1_000;
pub const MAX_TIMEOUT_MS: u64 = 600_000;

/// A validation finding. `error` blocks publish; `warning` does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationFinding {
    pub severity: Severity,
    /// Stable machine code, for i18n and for tests that assert on cause.
    pub code: String,
    /// The Hook the finding is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_id: Option<HookId>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub findings: Vec<ValidationFinding>,
}

impl ValidationReport {
    pub fn is_publishable(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &ValidationFinding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
    }
}

/// Validate `draft` against the Hooks discovery actually found.
///
/// `discovered` may be empty (no project context): unknown-Hook findings then
/// downgrade to warnings, because "this machine has no such file" is not the
/// same statement as "this document is wrong".
pub fn validate(draft: &HarnessBlueprint, discovered: &[HookDefinition]) -> ValidationReport {
    let mut findings = Vec::new();
    let index: std::collections::BTreeMap<&HookId, &HookDefinition> =
        discovered.iter().map(|d| (&d.id, d)).collect();

    for overlay in draft.overlays() {
        match index.get(&overlay.hook_id) {
            Some(definition) if is_locked(definition) => {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    code: "harness.overlay_on_locked_hook".into(),
                    hook_id: Some(overlay.hook_id.clone()),
                    message: format!(
                        "{} is a built-in safety Hook and cannot be overlaid \
                         (fields: {})",
                        overlay.hook_id,
                        overlay.set_fields().join(", ")
                    ),
                });
            }
            Some(_) => {}
            None => {
                findings.push(ValidationFinding {
                    severity: Severity::Warning,
                    code: "harness.unknown_hook".into(),
                    hook_id: Some(overlay.hook_id.clone()),
                    message: format!(
                        "{} was not discovered for this project; the overlay will \
                         have no effect until the source appears",
                        overlay.hook_id
                    ),
                });
            }
        }
        findings.extend(check_overlay_fields(overlay));
    }

    for hook in &draft.native_hooks {
        let hook_id = HookId::native(&hook.id, hook.event);
        let mut error = |code: &str, message: String| {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: code.into(),
                hook_id: Some(hook_id.clone()),
                message,
            });
        };
        if hook.name.trim().is_empty() {
            error(
                "harness.blank_native_hook_name",
                "a Native Hook name must not be blank".into(),
            );
        }
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&hook.timeout_ms) {
            error(
                "harness.timeout_out_of_range",
                format!(
                    "timeout {}ms is outside the supported range {MIN_TIMEOUT_MS}..={MAX_TIMEOUT_MS}",
                    hook.timeout_ms
                ),
            );
        }
        if hook
            .matcher
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            error(
                "harness.blank_matcher",
                "a blank matcher silently matches every tool; write \"*\" if that is the intent"
                    .into(),
            );
        }
        match &hook.adapter {
            HookAdapterSpecV3::Command {
                program,
                secret_env_refs,
                trusted,
                mode,
                ..
            } => {
                if program.trim().is_empty() {
                    error(
                        "harness.command_program_required",
                        "a Native Command Hook requires a program".into(),
                    );
                }
                if !secret_env_refs.is_empty() {
                    error(
                        "harness.command_secret_env_unsupported",
                        "Native Command Hook secret environment references are not wired".into(),
                    );
                }
                if *mode == CommandMode::Shell {
                    error(
                        "harness.shell_command_unsupported",
                        "shell-mode Native Hooks are not supported by the production registry"
                            .into(),
                    );
                } else if !*trusted || !hook.trust_confirmed {
                    error(
                        "harness.trust_required",
                        "a Native Command Hook requires explicit trust confirmation before publish"
                            .into(),
                    );
                }
            }
            HookAdapterSpecV3::Http {
                url,
                allow_hosts,
                headers,
                secret_header_refs,
            } => {
                if let Err(reason) = validate_http_hook_url(url, allow_hosts) {
                    error("harness.invalid_http_url", reason);
                }
                if !headers.is_empty() || !secret_header_refs.is_empty() {
                    error(
                        "harness.http_headers_unsupported",
                        "Native HTTP Hook headers are not wired and cannot be published".into(),
                    );
                }
            }
            HookAdapterSpecV3::McpTool {
                server_id,
                tool_name,
                input_template,
            } => {
                if server_id.trim().is_empty() || tool_name.trim().is_empty() {
                    error(
                        "harness.mcp_target_required",
                        "a Native MCP Hook requires both server_id and tool_name".into(),
                    );
                }
                if let Some(template) = input_template {
                    if serde_json::from_str::<serde_json::Value>(template).is_err() {
                        error(
                            "harness.invalid_mcp_input_template",
                            "MCP input_template must be valid JSON".into(),
                        );
                    }
                }
            }
            HookAdapterSpecV3::Prompt {
                template,
                model_override,
            } => {
                if template.trim().is_empty() {
                    error(
                        "harness.prompt_template_required",
                        "a Native Prompt Hook requires a decision template".into(),
                    );
                }
                if model_override
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    error(
                        "harness.blank_model_override",
                        "model_override must be omitted rather than blank".into(),
                    );
                }
            }
            HookAdapterSpecV3::Agent {
                prompt,
                model_override,
                max_steps,
                readonly_tools,
            } => {
                if prompt.trim().is_empty() {
                    error(
                        "harness.agent_prompt_required",
                        "a Native Agent Hook requires a task prompt".into(),
                    );
                }
                if !(1..=32).contains(max_steps) {
                    error(
                        "harness.agent_max_steps_out_of_range",
                        "Agent Hook max_steps must be between 1 and 32".into(),
                    );
                }
                if model_override
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
                    || readonly_tools.iter().any(|tool| tool.trim().is_empty())
                {
                    error(
                        "harness.blank_agent_field",
                        "Agent Hook optional fields must be omitted rather than blank".into(),
                    );
                }
            }
        }
    }

    for block in &draft.prompt_blocks {
        if block.name.trim().is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.blank_prompt_block_name".into(),
                hook_id: None,
                message: format!("Prompt Block {} has a blank name", block.id),
            });
        }
        if block.markdown.trim().is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.blank_prompt_block".into(),
                hook_id: None,
                message: format!("Prompt Block {} has no content", block.id),
            });
        }
    }

    let mut total_prompt_bytes = 0usize;
    for block in &draft.prompt_blocks {
        total_prompt_bytes += block.markdown.len();
    }

    for rep in &draft.builtin_prompt_replacements {
        if rep.surface_id.trim().is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.blank_builtin_prompt_replacement_surface".into(),
                hook_id: None,
                message: "Builtin prompt replacement has a blank surface_id".into(),
            });
        }
        if rep.markdown.trim().is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.blank_builtin_prompt_replacement".into(),
                hook_id: None,
                message: format!(
                    "Builtin prompt replacement for {} has no content",
                    rep.surface_id
                ),
            });
        }
        if rep.markdown.len() > 65_536 {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.prompt_replacement_too_large".into(),
                hook_id: None,
                message: format!(
                    "Builtin prompt replacement for {} exceeds 64 KiB limit ({} bytes)",
                    rep.surface_id,
                    rep.markdown.len()
                ),
            });
        }
        total_prompt_bytes += rep.markdown.len();
    }

    if total_prompt_bytes > 262_144 {
        findings.push(ValidationFinding {
            severity: Severity::Error,
            code: "harness.total_prompt_size_too_large".into(),
            hook_id: None,
            message: format!(
                "Total Harness prompt content exceeds 256 KiB limit ({} bytes)",
                total_prompt_bytes
            ),
        });
    }

    ValidationReport { findings }
}

/// Runtime and publish-time SSRF validation share this exact rule.
pub fn validate_http_hook_url(url: &str, allow_hosts: &[String]) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid url: {error}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("only http/https hooks allowed".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "missing host".to_string())?;
    if !allow_hosts.is_empty()
        && !allow_hosts.iter().any(|allowed| {
            allowed.eq_ignore_ascii_case(host) || host.ends_with(&format!(".{allowed}"))
        })
    {
        return Err(format!("host '{host}' not in allowlist"));
    }
    if host.parse::<IpAddr>().is_ok_and(is_private_or_loopback)
        || ["localhost", "metadata.google.internal", "169.254.169.254"]
            .iter()
            .any(|blocked| host.eq_ignore_ascii_case(blocked))
    {
        return Err("private/loopback and metadata hosts are not allowed".into());
    }
    Ok(())
}

fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.octets()[0] == 169 && ip.octets()[1] == 254
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

fn check_overlay_fields(overlay: &HookOverlay) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();
    if overlay.is_empty() {
        findings.push(ValidationFinding {
            severity: Severity::Warning,
            code: "harness.empty_overlay".into(),
            hook_id: Some(overlay.hook_id.clone()),
            message: format!("{} has an overlay that sets nothing", overlay.hook_id),
        });
    }
    if let Some(timeout) = overlay.timeout_ms {
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&timeout) {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.timeout_out_of_range".into(),
                hook_id: Some(overlay.hook_id.clone()),
                message: format!(
                    "timeout {timeout}ms is outside the supported range \
                     {MIN_TIMEOUT_MS}..={MAX_TIMEOUT_MS}"
                ),
            });
        }
    }
    if let Some(matcher) = overlay.matcher.as_deref() {
        if matcher.trim().is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                code: "harness.blank_matcher".into(),
                hook_id: Some(overlay.hook_id.clone()),
                message:
                    "a blank matcher silently matches every tool; write \"*\" if that is the intent"
                        .to_string(),
            });
        }
    }
    findings
}

/// One reviewable change between two documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BlueprintChange {
    pub hook_id: Option<HookId>,
    pub field: String,
    /// JSON text of the previous value, or `null` when it was unset.
    pub from: serde_json::Value,
    pub to: serde_json::Value,
}

/// Field-level diff between two Blueprints.
///
/// Ordered: document-level fields first, then Hooks by id, so two clients
/// reviewing the same publish see the same list.
pub fn diff(before: &HarnessBlueprint, after: &HarnessBlueprint) -> Vec<BlueprintChange> {
    let mut changes = Vec::new();
    if before.hook_semantics_version != after.hook_semantics_version {
        changes.push(BlueprintChange {
            hook_id: None,
            field: "hook_semantics_version".into(),
            from: serde_json::json!(before.hook_semantics_version.as_str()),
            to: serde_json::json!(after.hook_semantics_version.as_str()),
        });
    }
    if before.prompt_semantics_version != after.prompt_semantics_version {
        changes.push(BlueprintChange {
            hook_id: None,
            field: "prompt_semantics_version".into(),
            from: serde_json::json!(before.prompt_semantics_version.as_str()),
            to: serde_json::json!(after.prompt_semantics_version.as_str()),
        });
    }
    for (field, from, to) in [
        (
            "native_hooks",
            serde_json::json!(before.native_hooks),
            serde_json::json!(after.native_hooks),
        ),
        (
            "prompt_blocks",
            serde_json::json!(before.prompt_blocks),
            serde_json::json!(after.prompt_blocks),
        ),
    ] {
        if from != to {
            changes.push(BlueprintChange {
                hook_id: None,
                field: field.into(),
                from,
                to,
            });
        }
    }

    let ids: std::collections::BTreeSet<&HookId> = before
        .overlays()
        .iter()
        .chain(after.overlays())
        .map(|o| &o.hook_id)
        .collect();
    for id in ids {
        let old = before.overlay_for(id);
        let new = after.overlay_for(id);
        let field_value = |overlay: Option<&HookOverlay>, field: &str| -> serde_json::Value {
            let Some(overlay) = overlay else {
                return serde_json::Value::Null;
            };
            match field {
                "enabled" => serde_json::json!(overlay.enabled),
                "order" => serde_json::json!(overlay.order),
                "matcher" => serde_json::json!(overlay.matcher),
                "timeout_ms" => serde_json::json!(overlay.timeout_ms),
                "failure_policy" => serde_json::json!(overlay.failure_policy),
                _ => serde_json::Value::Null,
            }
        };
        for field in [
            "enabled",
            "order",
            "matcher",
            "timeout_ms",
            "failure_policy",
        ] {
            let from = field_value(old, field);
            let to = field_value(new, field);
            if from != to {
                changes.push(BlueprintChange {
                    hook_id: Some(id.clone()),
                    field: field.into(),
                    from,
                    to,
                });
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::{
        HookAdapterSpecV3, HookSemanticsVersion, NativeHookSpecV3, PromptBlockPlacement,
        PromptBlockSpecV3, BLUEPRINT_SCHEMA_VERSION,
    };
    use crate::hooks::{HookEvent, HookFailurePolicy, HookKind, HookScope, HookSource};

    fn definition(source: HookSource, event: HookEvent) -> HookDefinition {
        HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order: 0,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 10_000,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin { name: "x".into() },
        }
    }

    fn project_hook() -> HookDefinition {
        definition(
            HookSource::file(HookScope::Project, ".claude/settings.json", 0, 0),
            HookEvent::PostToolUse,
        )
    }

    fn builtin_hook() -> HookDefinition {
        definition(HookSource::builtin("allow-all"), HookEvent::PreToolUse)
    }

    fn doc(hooks: Vec<HookOverlay>) -> HarnessBlueprint {
        HarnessBlueprint {
            schema_version: BLUEPRINT_SCHEMA_VERSION,
            hook_semantics_version: HookSemanticsVersion::LegacyV1,
            prompt_semantics_version: crate::blueprint::PromptSemanticsVersion::LegacyV1,
            hooks,
            hook_overlays: Vec::new(),
            native_hooks: Vec::new(),
            prompt_blocks: Vec::new(),
            builtin_prompt_replacements: Vec::new(),
        }
    }

    fn native(adapter: HookAdapterSpecV3) -> NativeHookSpecV3 {
        NativeHookSpecV3 {
            id: "11111111-1111-1111-1111-111111111111".into(),
            name: "Gate".into(),
            enabled: true,
            event: HookEvent::PreToolUse,
            order: 0,
            matcher: Some("*".into()),
            conditions: Vec::new(),
            timeout_ms: 10_000,
            failure_policy: HookFailurePolicy::Fail,
            adapter,
            trust_confirmed: true,
        }
    }

    fn native_doc(adapter: HookAdapterSpecV3) -> HarnessBlueprint {
        let mut document = HarnessBlueprint::default();
        document.native_hooks.push(native(adapter));
        document
    }

    #[test]
    fn the_empty_document_is_publishable() {
        let report = validate(&HarnessBlueprint::default(), &[project_hook()]);
        assert!(report.findings.is_empty());
        assert!(report.is_publishable());
    }

    #[test]
    fn overlaying_a_locked_hook_blocks_publish() {
        let hook = builtin_hook();
        let mut overlay = HookOverlay::new(hook.id.clone());
        overlay.enabled = Some(false);
        let report = validate(&doc(vec![overlay]), &[hook]);
        assert!(!report.is_publishable());
        assert_eq!(
            report.errors().map(|f| f.code.as_str()).collect::<Vec<_>>(),
            vec!["harness.overlay_on_locked_hook"]
        );
    }

    #[test]
    fn an_unknown_hook_warns_but_still_publishes() {
        let mut overlay = HookOverlay::new(HookId::new(
            &HookSource::file(HookScope::Project, ".claude/absent.json", 0, 0),
            HookEvent::PreToolUse,
        ));
        overlay.timeout_ms = Some(5_000);
        let report = validate(&doc(vec![overlay]), &[project_hook()]);
        assert!(
            report.is_publishable(),
            "a missing source is not a broken document"
        );
        assert_eq!(
            report
                .findings
                .iter()
                .map(|f| f.code.as_str())
                .collect::<Vec<_>>(),
            vec!["harness.unknown_hook"]
        );
    }

    #[test]
    fn timeouts_outside_the_production_clamp_are_errors() {
        let hook = project_hook();
        for bad in [0, 1, MAX_TIMEOUT_MS + 1] {
            let mut overlay = HookOverlay::new(hook.id.clone());
            overlay.timeout_ms = Some(bad);
            let report = validate(&doc(vec![overlay]), &[hook.clone()]);
            assert!(!report.is_publishable(), "{bad} should be rejected");
        }
        let mut ok = HookOverlay::new(hook.id.clone());
        ok.timeout_ms = Some(30_000);
        assert!(validate(&doc(vec![ok]), &[hook]).is_publishable());
    }

    #[test]
    fn a_blank_matcher_is_an_error_not_a_silent_wildcard() {
        let hook = project_hook();
        let mut overlay = HookOverlay::new(hook.id.clone());
        overlay.matcher = Some("   ".into());
        let report = validate(&doc(vec![overlay]), &[hook]);
        assert_eq!(
            report.errors().map(|f| f.code.as_str()).collect::<Vec<_>>(),
            vec!["harness.blank_matcher"]
        );
    }

    #[test]
    fn an_overlay_that_sets_nothing_warns() {
        let hook = project_hook();
        let report = validate(&doc(vec![HookOverlay::new(hook.id.clone())]), &[hook]);
        assert!(report.is_publishable());
        assert_eq!(
            report
                .findings
                .iter()
                .map(|f| f.code.as_str())
                .collect::<Vec<_>>(),
            vec!["harness.empty_overlay"]
        );
    }

    #[test]
    fn native_adapters_reject_incomplete_configuration() {
        let cases = [
            native_doc(HookAdapterSpecV3::Command {
                program: " ".into(),
                args: Vec::new(),
                working_dir_policy: Default::default(),
                secret_env_refs: Default::default(),
                trusted: true,
                mode: Default::default(),
            }),
            native_doc(HookAdapterSpecV3::Http {
                url: " ".into(),
                allow_hosts: Vec::new(),
                headers: Default::default(),
                secret_header_refs: Default::default(),
            }),
            native_doc(HookAdapterSpecV3::McpTool {
                server_id: " ".into(),
                tool_name: " ".into(),
                input_template: Some("{".into()),
            }),
            native_doc(HookAdapterSpecV3::Prompt {
                template: " ".into(),
                model_override: None,
            }),
            native_doc(HookAdapterSpecV3::Agent {
                prompt: " ".into(),
                model_override: None,
                max_steps: 0,
                readonly_tools: Vec::new(),
            }),
        ];
        for document in cases {
            assert!(!validate(&document, &[]).is_publishable());
        }
    }

    #[test]
    fn native_hook_common_fields_and_prompt_blocks_are_validated() {
        let mut document = native_doc(HookAdapterSpecV3::Prompt {
            template: "allow or deny".into(),
            model_override: None,
        });
        document.native_hooks[0].name = " ".into();
        document.native_hooks[0].matcher = Some(" ".into());
        document.native_hooks[0].timeout_ms = 1;
        document.prompt_blocks.push(PromptBlockSpecV3 {
            id: "22222222-2222-2222-2222-222222222222".into(),
            name: " ".into(),
            markdown: " ".into(),
            enabled: true,
            order: 0,
            placement: PromptBlockPlacement::Final,
        });
        let report = validate(&document, &[]);
        let codes = report
            .errors()
            .map(|finding| finding.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"harness.blank_native_hook_name"));
        assert!(codes.contains(&"harness.blank_matcher"));
        assert!(codes.contains(&"harness.timeout_out_of_range"));
        assert!(codes.contains(&"harness.blank_prompt_block_name"));
        assert!(codes.contains(&"harness.blank_prompt_block"));
    }

    #[test]
    fn diff_includes_native_hooks_prompt_blocks_and_prompt_semantics() {
        let before = HarnessBlueprint::default();
        let mut after = native_doc(HookAdapterSpecV3::Prompt {
            template: "allow or deny".into(),
            model_override: None,
        });
        after.prompt_semantics_version = crate::blueprint::PromptSemanticsVersion::SequentialV2;
        after.prompt_blocks.push(PromptBlockSpecV3 {
            id: "22222222-2222-2222-2222-222222222222".into(),
            name: "Rules".into(),
            markdown: "Be safe.".into(),
            enabled: true,
            order: 0,
            placement: PromptBlockPlacement::Final,
        });
        let fields = diff(&before, &after)
            .into_iter()
            .map(|change| change.field)
            .collect::<Vec<_>>();
        assert!(fields.contains(&"prompt_semantics_version".into()));
        assert!(fields.contains(&"native_hooks".into()));
        assert!(fields.contains(&"prompt_blocks".into()));
    }

    #[test]
    fn diff_reports_added_removed_and_changed_fields() {
        let hook = project_hook();
        let mut before = HookOverlay::new(hook.id.clone());
        before.timeout_ms = Some(1_000);
        before.matcher = Some("Edit".into());
        let mut after = HookOverlay::new(hook.id.clone());
        after.timeout_ms = Some(2_000);
        after.enabled = Some(false);

        let changes = diff(&doc(vec![before]), &doc(vec![after]));
        assert_eq!(
            changes
                .iter()
                .map(|c| (c.field.as_str(), c.from.clone(), c.to.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("enabled", serde_json::Value::Null, serde_json::json!(false)),
                (
                    "matcher",
                    serde_json::json!("Edit"),
                    serde_json::Value::Null
                ),
                (
                    "timeout_ms",
                    serde_json::json!(1_000),
                    serde_json::json!(2_000)
                ),
            ]
        );
    }

    #[test]
    fn diff_reports_a_semantics_move_first() {
        let mut after = HarnessBlueprint::default();
        after.hook_semantics_version = HookSemanticsVersion::SequentialV2;
        let changes = diff(&HarnessBlueprint::default(), &after);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "hook_semantics_version");
        assert!(changes[0].hook_id.is_none());
    }

    #[test]
    fn identical_documents_diff_to_nothing() {
        let hook = project_hook();
        let mut overlay = HookOverlay::new(hook.id);
        overlay.order = Some(3);
        let document = doc(vec![overlay]);
        assert!(diff(&document, &document).is_empty());
    }
}
