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
#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
