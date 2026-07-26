//! The immutable evidence of which Harness a Run actually used.
//!
//! A Run binds a snapshot at start and never re-reads configuration
//! (design 第 12.3 节). Publishing a new version while a Run is in flight must
//! not change that Run, and the only way to *prove* it did not is to have
//! written down, before the engine started, exactly what was resolved.
//!
//! The snapshot is therefore taken before `ProductionRuntime` builds
//! `AgentEngine`, persisted, and only then handed to the engine. A snapshot
//! that cannot be persisted fails the Run (design 第 16 节) — an unprovable Run
//! is worse than no Run.

use crate::blueprint::{canonical_json, sha256_hex, HookSemanticsVersion};
use crate::redaction::redact_kind;
use crate::resolver::{ProfileLayer, Resolution, ResolutionIssue, ResolvedHook};
use crate::topology::TOPOLOGY_VERSION;
use serde::{Deserialize, Serialize};

/// The exact published layer a resolution consumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerRef {
    pub layer: ProfileLayer,
    pub profile_id: String,
    pub profile_name: String,
    pub version_id: String,
    pub version_number: i64,
    /// The published version's own canonical hash, so a snapshot can be
    /// checked against `harness_version` without re-resolving.
    pub canonical_hash: String,
}

/// What a Run ran with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedHarnessSnapshot {
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Weakest to strongest. Empty is impossible in production: the global
    /// template is seeded at first open.
    pub layers: Vec<LayerRef>,
    pub topology_version: u32,
    pub hook_semantics_version: HookSemanticsVersion,
    /// Every discovered Hook, redacted, in effective dispatch order.
    pub hooks: Vec<ResolvedHook>,
    pub issues: Vec<ResolutionIssue>,
    /// RFC 3339, set by the caller so tests can be deterministic.
    pub resolved_at: String,
}

impl ResolvedHarnessSnapshot {
    /// Build from a [`Resolution`], redacting adapter configuration on the way in.
    ///
    /// Redaction happens here rather than at the RPC edge so there is exactly
    /// one path into persistence and no second, unredacted one to forget about.
    pub fn new(
        run_id: impl Into<String>,
        conversation_id: Option<String>,
        project_id: Option<String>,
        layers: Vec<LayerRef>,
        resolution: &Resolution,
        resolved_at: impl Into<String>,
    ) -> Self {
        let hooks = resolution
            .hooks
            .iter()
            .map(|hook| {
                let mut redacted = hook.clone();
                redacted.definition.kind = redact_kind(&hook.definition.kind);
                redacted
            })
            .collect();
        Self {
            run_id: run_id.into(),
            conversation_id,
            project_id,
            layers,
            topology_version: TOPOLOGY_VERSION,
            hook_semantics_version: resolution.semantics,
            hooks,
            issues: resolution.issues.clone(),
            resolved_at: resolved_at.into(),
        }
    }

    /// Hash of the *configuration*, excluding which Run used it and when.
    ///
    /// Two Runs started a minute apart from the same published versions must
    /// produce the same hash — that equality is the whole point. Including
    /// `run_id` or `resolved_at` would make every snapshot unique and the field
    /// meaningless.
    pub fn canonical_hash(&self) -> String {
        sha256_hex(&self.configuration_json())
    }

    fn configuration_json(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.remove("run_id");
            object.remove("resolved_at");
            object.remove("conversation_id");
        }
        canonical_json(&value)
    }

    /// The Hooks that will actually dispatch, in order.
    pub fn enabled_hooks(&self) -> impl Iterator<Item = &ResolvedHook> {
        self.hooks.iter().filter(|h| h.enabled)
    }

    pub fn version_id_for(&self, layer: ProfileLayer) -> Option<&str> {
        self.layers
            .iter()
            .find(|l| l.layer == layer)
            .map(|l| l.version_id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::HarnessBlueprint;
    use crate::hooks::{
        HookDefinition, HookEvent, HookFailurePolicy, HookId, HookKind, HookScope, HookSource,
    };
    use crate::resolver::resolve;

    fn http_hook() -> HookDefinition {
        let source = HookSource::file(HookScope::Project, ".claude/hooks.json", 0, 0);
        HookDefinition {
            id: HookId::new(&source, HookEvent::PostToolUse),
            event: HookEvent::PostToolUse,
            source,
            order: 1,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 5_000,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Http {
                url: "https://hooks.example/post?token=super-secret".into(),
                allow_hosts: vec!["hooks.example".into()],
            },
        }
    }

    fn layer_ref() -> LayerRef {
        LayerRef {
            layer: ProfileLayer::Global,
            profile_id: "harness.global.default".into(),
            profile_name: "Default".into(),
            version_id: "v-1".into(),
            version_number: 1,
            canonical_hash: HarnessBlueprint::default().canonical_hash(),
        }
    }

    fn snapshot_for(run_id: &str, at: &str) -> ResolvedHarnessSnapshot {
        let resolution = resolve(&[http_hook()], &[]);
        ResolvedHarnessSnapshot::new(
            run_id,
            Some("conv-1".into()),
            Some("proj-1".into()),
            vec![layer_ref()],
            &resolution,
            at,
        )
    }

    #[test]
    fn a_secret_in_a_hook_url_never_reaches_the_snapshot() {
        let snapshot = snapshot_for("run-1", "2026-07-26T00:00:00Z");
        let text = serde_json::to_string(&snapshot).unwrap();
        assert!(
            !text.contains("super-secret"),
            "snapshot leaked a hook URL secret: {text}"
        );
        assert!(text.contains("https://hooks.example/post"));
    }

    #[test]
    fn the_hash_identifies_configuration_not_the_run() {
        let a = snapshot_for("run-1", "2026-07-26T00:00:00Z");
        let b = snapshot_for("run-2", "2026-07-26T09:30:00Z");
        assert_eq!(
            a.canonical_hash(),
            b.canonical_hash(),
            "two runs on the same published versions must share a hash"
        );
    }

    #[test]
    fn changing_a_resolved_hook_changes_the_hash() {
        let mut a = snapshot_for("run-1", "2026-07-26T00:00:00Z");
        let before = a.canonical_hash();
        a.hooks[0].definition.timeout_ms = 1;
        assert_ne!(before, a.canonical_hash());
    }

    #[test]
    fn changing_the_bound_version_changes_the_hash() {
        let mut a = snapshot_for("run-1", "2026-07-26T00:00:00Z");
        let before = a.canonical_hash();
        a.layers[0].version_number = 2;
        a.layers[0].version_id = "v-2".into();
        assert_ne!(before, a.canonical_hash());
    }

    #[test]
    fn the_snapshot_records_the_topology_it_was_taken_under() {
        let snapshot = snapshot_for("run-1", "2026-07-26T00:00:00Z");
        assert_eq!(snapshot.topology_version, TOPOLOGY_VERSION);
        assert_eq!(snapshot.version_id_for(ProfileLayer::Global), Some("v-1"));
        assert_eq!(snapshot.version_id_for(ProfileLayer::Session), None);
    }

    #[test]
    fn disabled_hooks_are_recorded_but_not_counted_as_dispatching() {
        let hook = http_hook();
        let mut overlay = crate::blueprint::HookOverlay::new(hook.id.clone());
        overlay.enabled = Some(false);
        let mut document = HarnessBlueprint::default();
        document.hooks = vec![overlay];
        let resolution = resolve(&[hook], &[(ProfileLayer::Project, document)]);
        let snapshot = ResolvedHarnessSnapshot::new(
            "run-1",
            None,
            None,
            vec![layer_ref()],
            &resolution,
            "2026-07-26T00:00:00Z",
        );
        assert_eq!(snapshot.hooks.len(), 1);
        assert_eq!(snapshot.enabled_hooks().count(), 0);
    }

    #[test]
    fn snapshot_round_trips_through_json() {
        let snapshot = snapshot_for("run-1", "2026-07-26T00:00:00Z");
        let text = serde_json::to_string(&snapshot).unwrap();
        let back: ResolvedHarnessSnapshot = serde_json::from_str(&text).unwrap();
        assert_eq!(back, snapshot);
    }
}
