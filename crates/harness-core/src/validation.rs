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

use crate::blueprint::{HarnessBlueprint, HookOverlay};
use crate::hooks::{HookDefinition, HookId};
use crate::resolver::is_locked;
use serde::{Deserialize, Serialize};

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

    ValidationReport { findings }
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
    use crate::blueprint::{HookSemanticsVersion, BLUEPRINT_SCHEMA_VERSION};
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
            hooks,
            hook_overlays: Vec::new(),
            native_hooks: Vec::new(),
            prompt_blocks: Vec::new(),
        }
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
