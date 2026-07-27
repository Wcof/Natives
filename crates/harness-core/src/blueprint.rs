//! The Harness Blueprint — the only user-editable part of the Harness.
//!
//! A Blueprint is a **typed sparse overlay**, never a JSON merge patch. It
//! never restates a Hook; it names one by [`HookId`] and changes named fields.
//! That is what makes `.claude/settings.json` and friends stay read-only source
//! (design 第 12.2 节) while still being configurable from the UI.
//!
//! Unknown fields are rejected at parse time (`deny_unknown_fields`) so a
//! typo silently doing nothing is impossible, and a document written by a newer
//! Daemon fails loudly on an older one instead of being half-applied.

use crate::hooks::{HookEvent, HookFailurePolicy, HookId, HookKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Bumped when the Blueprint schema itself changes shape.
pub const BLUEPRINT_SCHEMA_VERSION: u32 = 2;

/// Which Hook dispatch semantics a published version commits to.
///
/// Recorded on the snapshot and **not** editable through an overlay: moving a
/// profile between the two is a validated, explicitly published change with a
/// visible diff (design 第 12.4 节), never a side effect of editing a timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSemanticsVersion {
    /// Today's dispatch and aggregation behaviour, bit for bit.
    #[default]
    LegacyV1,
    /// The approved sequential semantics. Not yet the migration target.
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

/// A sparse per-Hook overlay. Every field is optional; `None` means "inherit".
///
/// The set of fields is deliberately small and closed. It is exactly the set a
/// Profile is allowed to change without rewriting the source file:
/// design 第 12.2 节 names enable, order, matcher, timeout, and failure policy.
/// Anything that would change *what a Hook runs* — program, argv, URL, trust —
/// is absent by construction, so an overlay can never turn a read-only source
/// Hook into a different executable.
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

/// Natives-owned executable Hook. Imported Hooks can only be overlaid; these
/// definitions are the sole editable executable surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeHookSpec {
    pub id: String,
    pub event: HookEvent,
    pub kind: HookKind,
    #[serde(default)]
    pub order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub failure_policy: HookFailurePolicy,
    #[serde(default)]
    pub trusted: bool,
}

fn default_timeout() -> u64 {
    10_000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptBlock {
    pub id: String,
    pub label: String,
    pub content: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub order: i32,
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

    /// Names of the fields this overlay actually sets, in schema order.
    ///
    /// Rendered by the Hooks workspace as "changed by project overlay: timeout,
    /// order", so the list must stay in sync with the struct. It is derived
    /// from the values rather than hand-listed for exactly that reason.
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

/// One layer of Harness configuration.
///
/// The MVP keeps every engine policy slot except Hooks read-only
/// (design 第 21 节), so this struct has exactly one editable member. That is
/// the honest shape: an empty `hooks` list is a document that resolves to
/// today's production behaviour with nothing invented around it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessBlueprint {
    pub schema_version: u32,
    #[serde(default)]
    pub hook_semantics_version: HookSemanticsVersion,
    #[serde(default)]
    pub hooks: Vec<HookOverlay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_overlays: Vec<HookOverlay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_hooks: Vec<NativeHookSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_blocks: Vec<PromptBlock>,
}

impl Default for HarnessBlueprint {
    /// The migration default: no overlays at all.
    ///
    /// Required by design 第 5.3 节 — the default Blueprint must compile to
    /// behaviour equivalent to the current production path *before* anything
    /// becomes editable, and the only document that provably does is the empty
    /// one.
    fn default() -> Self {
        Self {
            schema_version: BLUEPRINT_SCHEMA_VERSION,
            hook_semantics_version: HookSemanticsVersion::LegacyV1,
            hooks: Vec::new(),
            hook_overlays: Vec::new(),
            native_hooks: Vec::new(),
            prompt_blocks: Vec::new(),
        }
    }
}

impl HarnessBlueprint {
    /// Parse strictly. Unknown fields and duplicate Hook ids are errors.
    pub fn parse(value: &Value) -> Result<Self, String> {
        let parsed: Self = serde_json::from_value(value.clone())
            .map_err(|e| format!("blueprint is not a valid document: {e}"))?;
        parsed.check_shape()?;
        Ok(parsed)
    }

    fn check_shape(&self) -> Result<(), String> {
        if self.schema_version != 1 && self.schema_version != BLUEPRINT_SCHEMA_VERSION {
            return Err(format!(
                "blueprint schema_version {} is not supported (this daemon speaks {})",
                self.schema_version, BLUEPRINT_SCHEMA_VERSION
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for overlay in self.overlays() {
            if !seen.insert(overlay.hook_id.clone()) {
                return Err(format!(
                    "duplicate overlay for hook {}: a document must state each \
                     Hook at most once, or precedence within one layer is undefined",
                    overlay.hook_id
                ));
            }
        }
        let mut native_ids = std::collections::BTreeSet::new();
        for hook in &self.native_hooks {
            let parsed = uuid::Uuid::parse_str(&hook.id)
                .map_err(|_| format!("native hook id must be a UUID: {}", hook.id))?;
            if !native_ids.insert(parsed) {
                return Err(format!("duplicate native hook id: {}", hook.id));
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

    /// Canonical JSON text: object keys sorted, no insignificant whitespace.
    pub fn canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).unwrap_or(Value::Null))
    }

    /// SHA-256 of [`Self::canonical_json`], lowercase hex.
    ///
    /// Two documents that differ only in key order or in the presence of an
    /// explicit `null` hash the same, which is the property a version identity
    /// needs: re-saving a draft through a different client must not look like
    /// a content change.
    pub fn canonical_hash(&self) -> String {
        sha256_hex(&self.canonical_json())
    }
}

/// Serialize `value` with object keys in sorted order and no extra whitespace.
///
/// `serde_json`'s own output already sorts keys **only** when the crate is
/// built without the `preserve_order` feature. Any dependency in the graph may
/// switch that on, at which point hashes computed here would silently start
/// depending on struct field order. Canonicalising explicitly makes the hash a
/// property of the document rather than of the build.
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

/// Lowercase hex SHA-256 of a string.
pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{HookEvent, HookScope, HookSource};
    use serde_json::json;

    fn hook_id(name: &str) -> HookId {
        HookId::new(&HookSource::builtin(name), HookEvent::PreToolUse)
    }

    #[test]
    fn default_document_is_empty_and_legacy() {
        let doc = HarnessBlueprint::default();
        assert!(doc.hooks.is_empty());
        assert_eq!(doc.hook_semantics_version, HookSemanticsVersion::LegacyV1);
        assert_eq!(doc.schema_version, BLUEPRINT_SCHEMA_VERSION);
    }

    /// Pins the identity of the seeded default template. If this hash changes,
    /// every existing default profile's `canonical_hash` has silently drifted.
    #[test]
    fn default_document_hash_is_stable() {
        assert_eq!(
            HarnessBlueprint::default().canonical_json(),
            r#"{"hook_semantics_version":"legacy_v1","hooks":[],"schema_version":2}"#
        );
        assert_eq!(
            HarnessBlueprint::default().canonical_hash(),
            sha256_hex(r#"{"hook_semantics_version":"legacy_v1","hooks":[],"schema_version":2}"#)
        );
    }

    #[test]
    fn unknown_fields_are_rejected_rather_than_ignored() {
        let err = HarnessBlueprint::parse(&json!({
            "schema_version": 1,
            "hooks": [],
            "concurrency": 8
        }))
        .unwrap_err();
        assert!(err.contains("concurrency"), "got: {err}");
    }

    #[test]
    fn unknown_overlay_fields_are_rejected() {
        let err = HarnessBlueprint::parse(&json!({
            "schema_version": 1,
            "hooks": [{ "hook_id": "builtin/x#PreToolUse", "program": "/bin/sh" }]
        }))
        .unwrap_err();
        assert!(err.contains("program"), "got: {err}");
    }

    #[test]
    fn a_future_schema_version_fails_loudly() {
        let err =
            HarnessBlueprint::parse(&json!({ "schema_version": 99, "hooks": [] })).unwrap_err();
        assert!(err.contains("99"), "got: {err}");
    }

    #[test]
    fn duplicate_overlays_for_one_hook_are_rejected() {
        let err = HarnessBlueprint::parse(&json!({
            "schema_version": 1,
            "hooks": [
                { "hook_id": "builtin/x#PreToolUse", "order": 1 },
                { "hook_id": "builtin/x#PreToolUse", "order": 2 }
            ]
        }))
        .unwrap_err();
        assert!(err.contains("duplicate"), "got: {err}");
    }

    #[test]
    fn set_fields_reports_only_what_the_overlay_states() {
        let mut overlay = HookOverlay::new(hook_id("x"));
        assert!(overlay.is_empty());
        overlay.timeout_ms = Some(1000);
        overlay.enabled = Some(false);
        assert_eq!(overlay.set_fields(), vec!["enabled", "timeout_ms"]);
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_depth() {
        let value = json!({ "b": 1, "a": { "d": 2, "c": [ { "f": 3, "e": 4 } ] } });
        assert_eq!(
            canonical_json(&value),
            r#"{"a":{"c":[{"e":4,"f":3}],"d":2},"b":1}"#
        );
    }

    #[test]
    fn hash_ignores_key_order_but_not_content() {
        let a: Value = serde_json::from_str(r#"{"x":1,"y":2}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"y":2,"x":1}"#).unwrap();
        let c: Value = serde_json::from_str(r#"{"x":1,"y":3}"#).unwrap();
        assert_eq!(
            sha256_hex(&canonical_json(&a)),
            sha256_hex(&canonical_json(&b))
        );
        assert_ne!(
            sha256_hex(&canonical_json(&a)),
            sha256_hex(&canonical_json(&c))
        );
    }

    #[test]
    fn document_round_trips_through_json() {
        let doc = HarnessBlueprint {
            schema_version: BLUEPRINT_SCHEMA_VERSION,
            hook_semantics_version: HookSemanticsVersion::SequentialV2,
            hooks: vec![HookOverlay {
                hook_id: HookId::new(
                    &HookSource::file(HookScope::Project, ".claude/settings.json", 0, 1),
                    HookEvent::PostToolUse,
                ),
                enabled: Some(false),
                order: Some(7),
                matcher: Some("Edit|Write".into()),
                timeout_ms: Some(2_500),
                failure_policy: Some(HookFailurePolicy::Skip),
            }],
            hook_overlays: Vec::new(),
            native_hooks: Vec::new(),
            prompt_blocks: Vec::new(),
        };
        let text = serde_json::to_string(&doc).unwrap();
        let back = HarnessBlueprint::parse(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(back, doc);
    }

    /// An overlay must not be able to restate what a Hook executes. This is a
    /// security property, so assert it on the serialized shape rather than
    /// trusting the struct definition to stay small.
    #[test]
    fn overlay_cannot_carry_an_executable() {
        for forbidden in ["program", "args", "url", "trusted", "kind", "conditions"] {
            let mut overlay = serde_json::Map::new();
            overlay.insert("hook_id".into(), json!("builtin/x#PreToolUse"));
            overlay.insert(forbidden.into(), json!("anything"));
            let err = HarnessBlueprint::parse(&json!({
                "schema_version": 1,
                "hooks": [Value::Object(overlay)]
            }))
            .unwrap_err();
            assert!(
                err.contains(forbidden),
                "overlay accepted `{forbidden}`, which would let configuration \
                 change what a read-only source Hook runs"
            );
        }
    }
}
