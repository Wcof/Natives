//! Wire projections for the Harness read surface.
//!
//! Every field emitted here has exactly one upstream source:
//!
//! | Field group | Source |
//! |---|---|
//! | stage / order / hook point / safe point | `harness_core::topology`, a code constant |
//! | trigger site | same constant, guarded by `harness_topology_truth.rs` |
//! | Hook identity, provenance, matcher, timeout, policy, trust | `production_hooks::discover_production_hooks` |
//! | enabled / locked / overrides | `harness_core::resolver`, applied to that discovery |
//! | profile / version / binding | `assistant.db` rows |
//!
//! There is no fourth column for "computed to look good". A field the sources
//! cannot fill is absent, not defaulted — an empty Hook list means the stage
//! really has no Hooks.
//!
//! Adapter configuration is redacted on the way out
//! (`harness_core::redaction`), so a Hook URL carrying a token in its query
//! string never reaches the Renderer.

use harness_core::redaction::redact_kind;
use harness_core::resolver::{Resolution, ResolvedHook};
use harness_core::topology::{hook_point_of, safe_point_name, STAGES};
use serde_json::Value;
use std::collections::BTreeMap;

/// One Hook, fully described.
pub fn resolved_hook(hook: &ResolvedHook) -> Value {
    let definition = &hook.definition;
    let (stage, point) = hook_point_of(definition.event);
    serde_json::json!({
        "id": definition.id.as_str(),
        "event": definition.event.as_str(),
        "stage": stage.as_str(),
        "dispatched": point.trigger.is_dispatched(),
        "dispatch_module": point.trigger.module(),
        "source": {
            "scope": definition.source.scope,
            "origin": definition.source.origin,
            "group_index": definition.source.group_index,
            "entry_index": definition.source.entry_index,
        },
        "order": definition.order,
        "matcher": definition.matcher,
        "conditions": definition.conditions,
        "timeout_ms": definition.timeout_ms,
        "failure_policy": definition.failure_policy,
        "kind": redact_kind(&definition.kind),
        "trusted": trusted(&definition.kind),
        "enabled": hook.enabled,
        "locked": hook.locked,
        "overrides": hook.overrides,
    })
}

/// Whether a Hook is allowed to spawn a process.
///
/// Only command Hooks can be untrusted; a builtin is engine code and an HTTP
/// Hook is gated by its allowlist instead. `null` says "trust does not apply",
/// which is different from "untrusted".
fn trusted(kind: &harness_core::hooks::HookKind) -> Option<bool> {
    match kind {
        harness_core::hooks::HookKind::Command { trusted, .. } => Some(*trusted),
        _ => None,
    }
}

/// The full Hook catalog for a resolution, in effective dispatch order.
pub fn hook_catalog(resolution: &Resolution) -> Value {
    let hooks: Vec<Value> = resolution.hooks.iter().map(resolved_hook).collect();
    serde_json::json!({
        "hooks": hooks,
        "hook_semantics_version": resolution.semantics.as_str(),
        "issues": resolution.issues,
        "counts": counts(resolution),
    })
}

/// Aggregate counts a UI header needs without re-walking the list.
pub fn counts(resolution: &Resolution) -> Value {
    let mut by_scope: BTreeMap<String, i64> = BTreeMap::new();
    let mut enabled = 0i64;
    let mut inert = 0i64;
    for hook in &resolution.hooks {
        let scope = serde_json::to_value(hook.definition.source.scope)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".into());
        *by_scope.entry(scope).or_insert(0) += 1;
        if hook.enabled {
            enabled += 1;
        }
        if !hook_point_of(hook.definition.event)
            .1
            .trigger
            .is_dispatched()
        {
            inert += 1;
        }
    }
    serde_json::json!({
        "total": resolution.hooks.len(),
        "enabled": enabled,
        "by_scope": by_scope,
        // Hooks attached to a point nothing dispatches. Surfaced rather than
        // hidden: a user configuring one deserves to know it can never fire.
        "attached_to_undispatched_points": inert,
    })
}

/// The fixed topology, annotated with what is attached to each point.
pub fn topology(resolution: &Resolution) -> Value {
    let mut by_event: BTreeMap<&str, Vec<&ResolvedHook>> = BTreeMap::new();
    for hook in &resolution.hooks {
        by_event
            .entry(hook.definition.event.as_str())
            .or_default()
            .push(hook);
    }

    let stages: Vec<Value> = STAGES
        .iter()
        .map(|stage| {
            let hook_points: Vec<Value> = stage
                .hook_points
                .iter()
                .map(|point| {
                    let attached = by_event
                        .get(point.event.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    serde_json::json!({
                        "event": point.event.as_str(),
                        "security_sensitive": point.event.is_security_sensitive(),
                        "dispatched": point.trigger.is_dispatched(),
                        "dispatch_module": point.trigger.module(),
                        "hook_count": attached.len(),
                        "enabled_hook_count": attached.iter().filter(|h| h.enabled).count(),
                        "hook_ids": attached
                            .iter()
                            .map(|h| h.definition.id.as_str())
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
            serde_json::json!({
                "id": stage.id.as_str(),
                "order": stage.order,
                "hook_points": hook_points,
                "safe_points": stage
                    .safe_points
                    .iter()
                    .map(|p| safe_point_name(*p))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    serde_json::json!({
        "topology_version": harness_core::topology::TOPOLOGY_VERSION,
        "stages": stages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::hooks::{
        HookDefinition, HookEvent, HookFailurePolicy, HookId, HookKind, HookScope, HookSource,
    };
    use harness_core::resolver::resolve;

    fn http_hook(url: &str) -> HookDefinition {
        let source = HookSource::file(HookScope::Project, ".claude/hooks.json", 0, 0);
        HookDefinition {
            id: HookId::new(&source, HookEvent::PostToolUse),
            event: HookEvent::PostToolUse,
            source,
            order: 1,
            matcher: Some("Edit".into()),
            conditions: Vec::new(),
            timeout_ms: 5_000,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Http {
                url: url.into(),
                allow_hosts: vec!["hooks.example".into()],
            },
        }
    }

    fn subagent_hook() -> HookDefinition {
        let source = HookSource::builtin("allow-all");
        HookDefinition {
            id: HookId::new(&source, HookEvent::SubagentStart),
            event: HookEvent::SubagentStart,
            source,
            order: 0,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin {
                name: "allow-all".into(),
            },
        }
    }

    #[test]
    fn a_hook_projection_carries_its_full_provenance() {
        let resolution = resolve(&[http_hook("https://hooks.example/post")], &[]);
        let value = resolved_hook(&resolution.hooks[0]);
        assert_eq!(value["id"], "project/.claude/hooks.json#PostToolUse[0]/0");
        assert_eq!(value["stage"], "tool_execute");
        assert_eq!(value["source"]["origin"], ".claude/hooks.json");
        assert_eq!(value["source"]["group_index"], 0);
        assert_eq!(value["source"]["entry_index"], 0);
        assert_eq!(value["matcher"], "Edit");
        assert_eq!(value["timeout_ms"], 5_000);
        assert_eq!(value["failure_policy"], "fail");
        assert_eq!(value["dispatched"], true);
        assert_eq!(value["locked"], false);
        assert_eq!(
            value["trusted"],
            Value::Null,
            "trust does not apply to HTTP hooks"
        );
    }

    #[test]
    fn a_hook_url_secret_never_reaches_the_projection() {
        let resolution = resolve(&[http_hook("https://hooks.example/post?token=leak")], &[]);
        let text = hook_catalog(&resolution).to_string();
        assert!(!text.contains("leak"), "projection leaked a token: {text}");
    }

    #[test]
    fn topology_reports_attachment_per_hook_point() {
        let resolution = resolve(&[http_hook("https://hooks.example/post")], &[]);
        let value = topology(&resolution);
        let stages = value["stages"].as_array().unwrap();
        assert_eq!(stages.len(), STAGES.len());

        let execute = stages
            .iter()
            .find(|s| s["id"] == "tool_execute")
            .expect("tool_execute stage");
        let post = execute["hook_points"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["event"] == "PostToolUse")
            .expect("PostToolUse point");
        assert_eq!(post["hook_count"], 1);
        assert_eq!(post["enabled_hook_count"], 1);
        assert_eq!(
            post["hook_ids"][0],
            "project/.claude/hooks.json#PostToolUse[0]/0"
        );
        assert_eq!(execute["safe_points"][0], "after_tool");

        let gate = stages.iter().find(|s| s["id"] == "tool_gate").unwrap();
        assert_eq!(gate["hook_points"][0]["security_sensitive"], true);
        assert_eq!(gate["hook_points"][0]["hook_count"], 0);
    }

    #[test]
    /// The subagent points used to be inert, and this test pinned that. They
    /// are now dispatched from the daemon's `task` tool, so it pins the new
    /// truth instead. `attached_to_undispatched_points` stays in the payload:
    /// the counter is what tells a user their hook will never run, and it
    /// reading zero is the whole point of having wired the pair.
    fn the_subagent_points_report_their_tool_path_dispatch() {
        let resolution = resolve(&[subagent_hook()], &[]);
        let value = topology(&resolution);
        let subagent = value["stages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "subagent")
            .unwrap();
        assert_eq!(subagent["hook_points"][0]["dispatched"], true);
        assert_eq!(
            subagent["hook_points"][0]["dispatch_module"],
            "natives-agent-daemon::production_tools"
        );
        assert_eq!(subagent["hook_points"][0]["hook_count"], 1);
        assert_eq!(
            hook_catalog(&resolution)["counts"]["attached_to_undispatched_points"],
            0
        );
    }

    #[test]
    fn counts_group_by_source_scope() {
        let resolution = resolve(
            &[http_hook("https://hooks.example/post"), subagent_hook()],
            &[],
        );
        let counts = hook_catalog(&resolution)["counts"].clone();
        assert_eq!(counts["total"], 2);
        assert_eq!(counts["enabled"], 2);
        assert_eq!(counts["by_scope"]["project"], 1);
        assert_eq!(counts["by_scope"]["builtin"], 1);
    }
}
