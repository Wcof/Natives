//! Layered resolution: discovered Hooks + Blueprint layers → effective Hooks.
//!
//! Precedence is design 第 9.1 节, top to bottom:
//!
//! 1. locked built-in topology and safety invariants;
//! 2. published global template;
//! 3. optional published project overlay;
//! 4. optional session selection of a published overlay.
//!
//! Discovery is *not* a layer. It is the ground truth this module overlays on
//! to, and it comes from the Daemon (`production_hooks::discover_production_hooks`)
//! because reading files is not this crate's job.
//!
//! The resolver never invents a Hook. Every [`ResolvedHook`] traces back to a
//! discovered [`HookDefinition`]; an overlay that names something undiscovered
//! produces an issue, not a row.

use crate::blueprint::{HarnessBlueprint, HookOverlay, HookSemanticsVersion};
use crate::hooks::{HookDefinition, HookId, HookScope, HookSource};
use serde::{Deserialize, Serialize};

/// Which configuration layer a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLayer {
    /// Published global template. Always present.
    Global,
    /// Published project overlay. Optional.
    Project,
    /// Published profile selected for one session. Optional.
    Session,
}

impl ProfileLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Session => "session",
        }
    }

    /// Application order, weakest first. Later layers win per field.
    pub const ORDER: [ProfileLayer; 3] = [Self::Global, Self::Project, Self::Session];
}

/// One field that a layer changed, and which layer changed it last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldOverride {
    pub field: String,
    pub layer: ProfileLayer,
}

/// Something the resolver could not honour, reported rather than swallowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolutionIssue {
    /// An overlay named a Hook that discovery did not find.
    ///
    /// Not fatal: a project overlay may legitimately mention a Hook file that
    /// only exists on another machine. It must still be visible, or the user
    /// stares at a setting that does nothing.
    UnknownHook {
        hook_id: HookId,
        layer: ProfileLayer,
    },
    /// An overlay tried to change a locked Hook. Ignored, never applied.
    LockedHook {
        hook_id: HookId,
        layer: ProfileLayer,
        fields: Vec<String>,
    },
}

/// A discovered Hook after every applicable layer has been applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedHook {
    /// The effective definition: what would actually be compiled and run.
    pub definition: HookDefinition,
    /// False when a layer disabled it. A disabled Hook stays in the catalog —
    /// hiding it would make "why did nothing happen?" unanswerable.
    pub enabled: bool,
    /// Locked Hooks cannot be overlaid at all (design 第 12.2 节).
    pub locked: bool,
    /// Which fields differ from discovery, and which layer set each.
    pub overrides: Vec<FieldOverride>,
}

/// The outcome of resolving one Run's Harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    /// Every discovered Hook, in effective dispatch order, enabled or not.
    pub hooks: Vec<ResolvedHook>,
    pub semantics: HookSemanticsVersion,
    pub issues: Vec<ResolutionIssue>,
}

impl Resolution {
    /// Only the Hooks that will actually be compiled, in dispatch order.
    pub fn enabled_definitions(&self) -> Vec<HookDefinition> {
        self.hooks
            .iter()
            .filter(|h| h.enabled)
            .map(|h| h.definition.clone())
            .collect()
    }
}

/// True for Hooks no overlay may touch.
///
/// Built-in Hooks are the engine's own safety defaults: the allow-all baseline
/// that makes the fail-closed aggregation meaningful. Letting configuration
/// reorder or disable them would make the security posture a setting.
pub fn is_locked(definition: &HookDefinition) -> bool {
    definition.source.scope == HookScope::Builtin
}

/// Apply `layers` to `discovered`.
///
/// `layers` is given weakest-first; callers should build it with
/// [`ProfileLayer::ORDER`] so precedence cannot be reversed by accident.
pub fn resolve(
    discovered: &[HookDefinition],
    layers: &[(ProfileLayer, HarnessBlueprint)],
) -> Resolution {
    // The strongest layer wins outright. Semantics is a whole-document property,
    // not a per-Hook field, so it does not go through the overlay path — and a
    // layer cannot "inherit" it, because serde cannot tell an omitted value from
    // an explicit `legacy_v1`.
    let semantics = layers
        .iter()
        .last()
        .map(|(_, doc)| doc.hook_semantics_version)
        .unwrap_or_default();

    let mut issues = Vec::new();
    let mut hooks: Vec<ResolvedHook> = Vec::with_capacity(discovered.len());

    for definition in discovered {
        let locked = is_locked(definition);
        let mut effective = definition.clone();
        let mut enabled = true;
        let mut overrides: Vec<FieldOverride> = Vec::new();

        for (layer, document) in layers {
            let Some(overlay) = document.overlay_for(&definition.id) else {
                continue;
            };
            if locked {
                let fields: Vec<String> = overlay
                    .set_fields()
                    .iter()
                    .map(|f| (*f).to_string())
                    .collect();
                if !fields.is_empty() {
                    issues.push(ResolutionIssue::LockedHook {
                        hook_id: definition.id.clone(),
                        layer: *layer,
                        fields,
                    });
                }
                continue;
            }
            apply_overlay(
                overlay,
                *layer,
                &mut effective,
                &mut enabled,
                &mut overrides,
            );
        }

        hooks.push(ResolvedHook {
            definition: effective,
            enabled,
            locked,
            overrides,
        });
    }

    for (_, document) in layers {
        for native in &document.native_hooks {
            let source =
                HookSource::file(HookScope::Project, format!("native:{}", native.id), 0, 0);
            hooks.push(ResolvedHook {
                definition: HookDefinition {
                    id: HookId::native(&native.id, native.event),
                    event: native.event,
                    source,
                    order: native.order,
                    matcher: native.matcher.clone(),
                    conditions: Vec::new(),
                    timeout_ms: native.timeout_ms,
                    failure_policy: native.failure_policy,
                    kind: native.kind.clone(),
                },
                enabled: true,
                locked: false,
                overrides: Vec::new(),
            });
        }
    }

    // Overlays that matched nothing must surface, once per layer.
    let discovered_ids: std::collections::BTreeSet<&HookId> =
        discovered.iter().map(|d| &d.id).collect();
    for (layer, document) in layers {
        for overlay in document.overlays() {
            if !discovered_ids.contains(&overlay.hook_id) {
                issues.push(ResolutionIssue::UnknownHook {
                    hook_id: overlay.hook_id.clone(),
                    layer: *layer,
                });
            }
        }
    }

    order_hooks(&mut hooks, semantics);
    Resolution {
        hooks,
        semantics,
        issues,
    }
}

fn apply_overlay(
    overlay: &HookOverlay,
    layer: ProfileLayer,
    effective: &mut HookDefinition,
    enabled: &mut bool,
    overrides: &mut Vec<FieldOverride>,
) {
    let record = |field: &str, overrides: &mut Vec<FieldOverride>| {
        overrides.retain(|o| o.field != field);
        overrides.push(FieldOverride {
            field: field.to_string(),
            layer,
        });
    };
    if let Some(value) = overlay.enabled {
        *enabled = value;
        record("enabled", overrides);
    }
    if let Some(value) = overlay.order {
        effective.order = value;
        record("order", overrides);
    }
    if let Some(value) = overlay.matcher.clone() {
        effective.matcher = Some(value);
        record("matcher", overrides);
    }
    if let Some(value) = overlay.timeout_ms {
        effective.timeout_ms = value;
        record("timeout_ms", overrides);
    }
    if let Some(value) = overlay.failure_policy {
        effective.failure_policy = value;
        record("failure_policy", overrides);
    }
}

/// Order Hooks for dispatch, per the profile's semantics version.
///
/// `legacy_v1` deliberately does nothing. Discovery order *is* today's dispatch
/// order (`production_hooks` guarantees it), so re-sorting here — even into a
/// "better" order — would be a behaviour change smuggled in under a
/// configuration feature. Design 第 12.4 节 requires the semantics move to be
/// its own published, diffed decision.
fn order_hooks(hooks: &mut [ResolvedHook], semantics: HookSemanticsVersion) {
    if semantics == HookSemanticsVersion::LegacyV1 {
        return;
    }
    // `sequential_v2`: locked band first, then configured order, then a stable
    // Hook ID tie-break so the sequence is reproducible across machines.
    hooks.sort_by(|a, b| {
        let band = |h: &ResolvedHook| u8::from(!h.locked);
        band(a)
            .cmp(&band(b))
            .then_with(|| a.definition.order.cmp(&b.definition.order))
            .then_with(|| a.definition.id.cmp(&b.definition.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::BLUEPRINT_SCHEMA_VERSION;
    use crate::hooks::{HookEvent, HookFailurePolicy, HookKind, HookSource};

    fn builtin(name: &str, event: HookEvent, order: i32) -> HookDefinition {
        let source = HookSource::builtin(name);
        HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin { name: name.into() },
        }
    }

    fn project(
        path: &str,
        event: HookEvent,
        group: usize,
        entry: usize,
        order: i32,
    ) -> HookDefinition {
        let source = HookSource::file(HookScope::Project, path, group, entry);
        HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 10_000,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Command {
                program: "/bin/sh".into(),
                args: vec!["-lc".into(), "true".into()],
                trusted: true,
            },
        }
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
    fn no_layers_reproduces_discovery_exactly() {
        let discovered = vec![
            builtin("allow-all", HookEvent::PreToolUse, 0),
            project(".claude/settings.json", HookEvent::PreToolUse, 0, 0, 1),
        ];
        let resolved = resolve(&discovered, &[]);
        assert_eq!(resolved.enabled_definitions(), discovered);
        assert!(resolved.issues.is_empty());
        assert_eq!(resolved.semantics, HookSemanticsVersion::LegacyV1);
    }

    #[test]
    fn empty_documents_reproduce_discovery_exactly() {
        let discovered = vec![project(
            ".claude/settings.json",
            HookEvent::PostToolUse,
            0,
            0,
            1,
        )];
        let resolved = resolve(
            &discovered,
            &[
                (ProfileLayer::Global, HarnessBlueprint::default()),
                (ProfileLayer::Project, HarnessBlueprint::default()),
            ],
        );
        assert_eq!(resolved.enabled_definitions(), discovered);
        assert!(resolved.issues.is_empty());
    }

    #[test]
    fn a_stronger_layer_wins_per_field() {
        let hook = project(".claude/settings.json", HookEvent::PostToolUse, 0, 0, 1);
        let mut global = HookOverlay::new(hook.id.clone());
        global.timeout_ms = Some(1_000);
        global.matcher = Some("Edit".into());
        let mut project_layer = HookOverlay::new(hook.id.clone());
        project_layer.timeout_ms = Some(2_000);

        let resolved = resolve(
            &[hook.clone()],
            &[
                (ProfileLayer::Global, doc(vec![global])),
                (ProfileLayer::Project, doc(vec![project_layer])),
            ],
        );
        let only = &resolved.hooks[0];
        assert_eq!(
            only.definition.timeout_ms, 2_000,
            "project overrides global"
        );
        assert_eq!(
            only.definition.matcher.as_deref(),
            Some("Edit"),
            "a field the project layer left alone keeps the global value"
        );
        assert_eq!(
            only.overrides,
            vec![
                FieldOverride {
                    field: "matcher".into(),
                    layer: ProfileLayer::Global
                },
                FieldOverride {
                    field: "timeout_ms".into(),
                    layer: ProfileLayer::Project
                },
            ]
        );
    }

    #[test]
    fn session_is_the_strongest_layer() {
        let hook = project(".claude/settings.json", HookEvent::PostToolUse, 0, 0, 1);
        let over = |ms: u64| {
            let mut o = HookOverlay::new(hook.id.clone());
            o.timeout_ms = Some(ms);
            o
        };
        let resolved = resolve(
            &[hook.clone()],
            &[
                (ProfileLayer::Global, doc(vec![over(1)])),
                (ProfileLayer::Project, doc(vec![over(2)])),
                (ProfileLayer::Session, doc(vec![over(3)])),
            ],
        );
        assert_eq!(resolved.hooks[0].definition.timeout_ms, 3);
    }

    #[test]
    fn disabled_hooks_stay_in_the_catalog_but_leave_the_dispatch() {
        let hook = project(".claude/settings.json", HookEvent::PostToolUse, 0, 0, 1);
        let mut overlay = HookOverlay::new(hook.id.clone());
        overlay.enabled = Some(false);
        let resolved = resolve(&[hook], &[(ProfileLayer::Project, doc(vec![overlay]))]);
        assert_eq!(resolved.hooks.len(), 1);
        assert!(!resolved.hooks[0].enabled);
        assert!(resolved.enabled_definitions().is_empty());
    }

    #[test]
    fn locked_hooks_reject_every_overlay_and_say_so() {
        let hook = builtin("allow-all", HookEvent::PreToolUse, 0);
        let mut overlay = HookOverlay::new(hook.id.clone());
        overlay.enabled = Some(false);
        overlay.timeout_ms = Some(1);
        let resolved = resolve(
            &[hook.clone()],
            &[(ProfileLayer::Global, doc(vec![overlay]))],
        );

        assert!(
            resolved.hooks[0].enabled,
            "a locked hook cannot be disabled"
        );
        assert!(resolved.hooks[0].locked);
        assert_eq!(resolved.hooks[0].definition, hook, "nothing was applied");
        assert_eq!(
            resolved.issues,
            vec![ResolutionIssue::LockedHook {
                hook_id: hook.id,
                layer: ProfileLayer::Global,
                fields: vec!["enabled".into(), "timeout_ms".into()],
            }]
        );
    }

    #[test]
    fn an_overlay_for_an_undiscovered_hook_is_reported_not_dropped() {
        let missing = HookId::new(
            &HookSource::file(HookScope::Project, ".claude/gone.json", 0, 0),
            HookEvent::PreToolUse,
        );
        let resolved = resolve(
            &[builtin("allow-all", HookEvent::PreToolUse, 0)],
            &[(
                ProfileLayer::Project,
                doc(vec![HookOverlay::new(missing.clone())]),
            )],
        );
        assert_eq!(
            resolved.issues,
            vec![ResolutionIssue::UnknownHook {
                hook_id: missing,
                layer: ProfileLayer::Project
            }]
        );
    }

    /// The behaviour-preservation guarantee: `legacy_v1` must not reorder.
    #[test]
    fn legacy_semantics_never_reorders() {
        let discovered = vec![
            project(".claude/a.json", HookEvent::PostToolUse, 0, 0, 9),
            builtin("allow-all", HookEvent::PostToolUse, 0),
            project(".claude/b.json", HookEvent::PostToolUse, 0, 0, 1),
        ];
        let resolved = resolve(&discovered, &[(ProfileLayer::Global, doc(vec![]))]);
        assert_eq!(resolved.enabled_definitions(), discovered);
    }

    #[test]
    fn sequential_semantics_puts_locked_first_then_order_then_id() {
        let discovered = vec![
            project(".claude/a.json", HookEvent::PostToolUse, 0, 0, 9),
            builtin("allow-all", HookEvent::PostToolUse, 0),
            project(".claude/b.json", HookEvent::PostToolUse, 0, 0, 1),
        ];
        let mut document = HarnessBlueprint::default();
        document.hook_semantics_version = HookSemanticsVersion::SequentialV2;
        let resolved = resolve(&discovered, &[(ProfileLayer::Global, document)]);
        assert_eq!(
            resolved
                .hooks
                .iter()
                .map(|h| h.definition.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "builtin/allow-all#PostToolUse",
                "project/.claude/b.json#PostToolUse[0]/0",
                "project/.claude/a.json#PostToolUse[0]/0",
            ]
        );
    }

    #[test]
    fn the_strongest_layer_decides_the_semantics_version() {
        let mut session = HarnessBlueprint::default();
        session.hook_semantics_version = HookSemanticsVersion::SequentialV2;
        let resolved = resolve(
            &[],
            &[
                (ProfileLayer::Global, HarnessBlueprint::default()),
                (ProfileLayer::Session, session),
            ],
        );
        assert_eq!(resolved.semantics, HookSemanticsVersion::SequentialV2);
    }
}
