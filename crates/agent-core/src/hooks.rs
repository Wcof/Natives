//! Hook Runtime — Session/Prompt/Tool/Permission/Subagent/Compact/Stop.
//!
//! Security-sensitive events (PreToolUse, PermissionRequest) use fail-closed
//! aggregation: any Deny wins; empty registry for those events denies by default
//! when `fail_closed` is set on the registry.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// The Hook data model — event identity, provenance, matching — lives in
// `harness-core`. This module owns only the executable runtime around it.
pub use harness_core::hooks::{
    Condition, ConditionOperator, HookDefinition, HookEvent, HookFailurePolicy, HookId, HookKind,
    HookScope, HookSource, tool_pattern_matches,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDecision {
    Allow,
    Deny { reason: String },
    Modify { payload: Value },
    Inject { messages: Vec<String> },
    Rewake,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRequest {
    pub event: HookEvent,
    pub run_id: String,
    pub tool_name: Option<String>,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResponse {
    pub decision: HookDecision,
}

/// Trait for hook handlers (Rust / command / HTTP adapters implement this).
#[async_trait::async_trait]
pub trait HookHandler: Send + Sync {
    async fn handle(&self, request: HookRequest) -> HookResponse;

    /// Optional tool-name matcher (glob-ish: `*` any, exact otherwise).
    fn matches_tool(&self, _tool_name: Option<&str>) -> bool {
        true
    }
}

/// One registered hook: what it is, and how to run it.
///
/// Definition and handler are stored together rather than in parallel maps so
/// the description can never drift from what actually dispatches.
struct Registered {
    definition: HookDefinition,
    handler: Box<dyn HookHandler>,
}

/// Registry of hook handlers keyed by event.
#[derive(Default)]
pub struct HookRegistry {
    entries: HashMap<HookEvent, Vec<Registered>>,
    /// When true, security-sensitive events with zero matching handlers deny.
    pub fail_closed_security: bool,
}

impl HookRegistry {
    /// Empty registry. Security fail-closed is **off** by default so bare
    /// `AgentEngine::new()` tests keep working; call
    /// [`HookRegistry::enable_security_fail_closed`] after registering defaults.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            fail_closed_security: false,
        }
    }

    pub fn enable_security_fail_closed(&mut self) {
        self.fail_closed_security = true;
    }

    /// Register a handler that has no configuration provenance.
    ///
    /// Used by fixtures and programmatic setup. [`Self::describe`] still
    /// reports it, labelled `ad-hoc`, so the catalog is never silently partial.
    pub fn register(&mut self, event: HookEvent, handler: Box<dyn HookHandler>) {
        let ordinal = self.entries.get(&event).map_or(0, Vec::len);
        let source = HookSource::builtin(format!("ad-hoc/{ordinal}"));
        let definition = HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order: ordinal as i32,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin {
                name: "ad-hoc".into(),
            },
        };
        self.register_defined(definition, handler);
    }

    /// Register a handler together with the definition it was compiled from.
    pub fn register_defined(&mut self, definition: HookDefinition, handler: Box<dyn HookHandler>) {
        self.entries
            .entry(definition.event)
            .or_default()
            .push(Registered {
                definition,
                handler,
            });
    }

    /// Every registered hook, in canonical event order then dispatch order.
    ///
    /// This is the inspection surface the control plane renders: identity,
    /// provenance, order, matcher, timeout, trust, and failure policy.
    pub fn describe(&self) -> Vec<HookDefinition> {
        HookEvent::ALL
            .into_iter()
            .filter_map(|event| self.entries.get(&event))
            .flat_map(|entries| entries.iter().map(|e| e.definition.clone()))
            .collect()
    }

    /// Registered hooks for a single event, in dispatch order.
    pub fn describe_event(&self, event: HookEvent) -> Vec<HookDefinition> {
        self.entries
            .get(&event)
            .map(|entries| entries.iter().map(|e| e.definition.clone()).collect())
            .unwrap_or_default()
    }

    pub async fn dispatch(&self, request: HookRequest) -> Vec<HookResponse> {
        let mut out = Vec::new();
        if let Some(entries) = self.entries.get(&request.event) {
            for entry in entries {
                // Both gates are consulted so the described matcher is provably
                // load-bearing: a compiled handler carries the same matcher as
                // its definition, and if the two ever diverge the stricter one
                // wins rather than the catalog silently misreporting what runs.
                // Ad-hoc registrations have no definition matcher, so their
                // handler remains the only gate.
                if !entry
                    .definition
                    .applies_to(request.tool_name.as_deref(), &request.input)
                {
                    continue;
                }
                if !entry.handler.matches_tool(request.tool_name.as_deref()) {
                    continue;
                }
                out.push(entry.handler.handle(request.clone()).await);
            }
        }
        // Fail-closed for security events with no matching handler.
        if out.is_empty()
            && self.fail_closed_security
            && request.event.is_security_sensitive()
        {
            out.push(HookResponse {
                decision: HookDecision::Deny {
                    reason: format!(
                        "fail-closed: no hook handler allowed {:?} for tool {:?}",
                        request.event, request.tool_name
                    ),
                },
            });
        }
        out
    }

    /// Aggregate: any Deny wins (fail-closed for PreToolUse / PermissionRequest).
    pub fn aggregate_allow(responses: &[HookResponse]) -> Result<(), String> {
        for r in responses {
            if let HookDecision::Deny { reason } = &r.decision {
                return Err(reason.clone());
            }
        }
        Ok(())
    }

    pub fn events_covered(&self) -> Vec<HookEvent> {
        self.entries.keys().copied().collect()
    }
}

/// Built-in allow-all handler (useful for tests / default).
pub struct AllowAllHook;

#[async_trait::async_trait]
impl HookHandler for AllowAllHook {
    async fn handle(&self, _request: HookRequest) -> HookResponse {
        HookResponse {
            decision: HookDecision::Allow,
        }
    }
}

/// Deny when tool name matches a pattern (`*` = all).
pub struct MatcherDenyHook {
    pub tool_pattern: String,
    pub reason: String,
}

#[async_trait::async_trait]
impl HookHandler for MatcherDenyHook {
    fn matches_tool(&self, tool_name: Option<&str>) -> bool {
        tool_pattern_matches(Some(&self.tool_pattern), tool_name)
    }

    async fn handle(&self, _request: HookRequest) -> HookResponse {
        HookResponse {
            decision: HookDecision::Deny {
                reason: self.reason.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allow_all_hook_fires() {
        let mut reg = HookRegistry::new();
        reg.fail_closed_security = false;
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        let responses = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0].decision, HookDecision::Allow));
    }

    #[tokio::test]
    async fn matcher_deny_blocks_matching_tool() {
        let mut reg = HookRegistry::new();
        reg.register(
            HookEvent::PreToolUse,
            Box::new(MatcherDenyHook {
                tool_pattern: "bash*".into(),
                reason: "bash blocked".into(),
            }),
        );
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        let responses = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("bash".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert!(HookRegistry::aggregate_allow(&responses).is_err());
        let ok = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        // only AllowAll matches read_file
        assert!(HookRegistry::aggregate_allow(&ok).is_ok());
    }

    #[test]
    fn describe_reports_ad_hoc_registrations_with_unique_identities() {
        let mut reg = HookRegistry::new();
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        reg.register(HookEvent::Stop, Box::new(AllowAllHook));

        let described = reg.describe();
        assert_eq!(described.len(), 3, "describe must not omit ad-hoc handlers");
        assert_eq!(
            described.iter().map(|d| d.id.to_string()).collect::<Vec<_>>(),
            vec![
                "builtin/ad-hoc/0#PreToolUse",
                "builtin/ad-hoc/1#PreToolUse",
                "builtin/ad-hoc/0#Stop",
            ]
        );
        assert_eq!(
            described.iter().map(|d| d.order).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[test]
    fn describe_orders_by_canonical_event_then_registration() {
        let mut reg = HookRegistry::new();
        // Register in reverse canonical order.
        reg.register(HookEvent::Error, Box::new(AllowAllHook));
        reg.register(HookEvent::SessionStart, Box::new(AllowAllHook));
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));

        assert_eq!(
            reg.describe().iter().map(|d| d.event).collect::<Vec<_>>(),
            vec![HookEvent::SessionStart, HookEvent::PreToolUse, HookEvent::Error]
        );
    }

    #[test]
    fn describe_event_returns_only_that_event() {
        let mut reg = HookRegistry::new();
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        reg.register(HookEvent::Stop, Box::new(AllowAllHook));

        assert_eq!(reg.describe_event(HookEvent::PreToolUse).len(), 1);
        assert_eq!(reg.describe_event(HookEvent::Stop).len(), 1);
        assert!(reg.describe_event(HookEvent::Notification).is_empty());
    }

    #[tokio::test]
    async fn describe_length_matches_dispatch_length_for_unmatched_tools() {
        let mut reg = HookRegistry::new();
        reg.fail_closed_security = false;
        let source = HookSource::builtin("scoped");
        reg.register_defined(
            HookDefinition {
                id: HookId::new(&source, HookEvent::PreToolUse),
                event: HookEvent::PreToolUse,
                source,
                order: 0,
                matcher: Some("bash*".into()),
                conditions: Vec::new(),
                timeout_ms: 0,
                failure_policy: HookFailurePolicy::Fail,
                kind: HookKind::Builtin {
                    name: "scoped".into(),
                },
            },
            Box::new(MatcherDenyHook {
                tool_pattern: "bash*".into(),
                reason: "blocked".into(),
            }),
        );

        // The catalog lists the hook regardless of the tool being dispatched…
        assert_eq!(reg.describe_event(HookEvent::PreToolUse).len(), 1);
        // …while dispatch honours the matcher.
        let matched = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("bash".into()),
                input: serde_json::json!({}),
            })
            .await;
        let unmatched = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(matched.len(), 1);
        assert!(unmatched.is_empty());
    }

    /// The catalog must not be able to misreport what actually runs.
    #[tokio::test]
    async fn described_matcher_is_load_bearing() {
        let mut reg = HookRegistry::new();
        reg.fail_closed_security = false;
        let source = HookSource::builtin("scoped");
        reg.register_defined(
            HookDefinition {
                id: HookId::new(&source, HookEvent::PreToolUse),
                event: HookEvent::PreToolUse,
                source,
                order: 0,
                matcher: Some("Bash".into()),
                conditions: Vec::new(),
                timeout_ms: 0,
                failure_policy: HookFailurePolicy::Fail,
                kind: HookKind::Builtin {
                    name: "scoped".into(),
                },
            },
            // The handler itself would match anything.
            Box::new(AllowAllHook),
        );

        let dispatch_for = |tool: &'static str| {
            let reg = &reg;
            async move {
                reg.dispatch(HookRequest {
                    event: HookEvent::PreToolUse,
                    run_id: "r".into(),
                    tool_name: Some(tool.into()),
                    input: serde_json::json!({}),
                })
                .await
            }
        };

        assert_eq!(dispatch_for("Bash").await.len(), 1);
        assert!(
            dispatch_for("read_file").await.is_empty(),
            "the definition's matcher must gate dispatch, not just decorate it"
        );
    }

    #[tokio::test]
    async fn definition_conditions_gate_dispatch() {
        let mut reg = HookRegistry::new();
        reg.fail_closed_security = false;
        let source = HookSource::builtin("git-only");
        reg.register_defined(
            HookDefinition {
                id: HookId::new(&source, HookEvent::PreToolUse),
                event: HookEvent::PreToolUse,
                source,
                order: 0,
                matcher: None,
                conditions: vec![Condition {
                    field: "command".into(),
                    operator: ConditionOperator::Contains,
                    pattern: "git push".into(),
                }],
                timeout_ms: 0,
                failure_policy: HookFailurePolicy::Fail,
                kind: HookKind::Builtin {
                    name: "git-only".into(),
                },
            },
            Box::new(AllowAllHook),
        );

        let dispatch_with = |command: &'static str| {
            let reg = &reg;
            async move {
                reg.dispatch(HookRequest {
                    event: HookEvent::PreToolUse,
                    run_id: "r".into(),
                    tool_name: Some("Bash".into()),
                    input: serde_json::json!({ "command": command }),
                })
                .await
            }
        };

        assert_eq!(dispatch_with("git push origin main").await.len(), 1);
        assert!(dispatch_with("ls -la").await.is_empty());
    }

    #[tokio::test]
    async fn ad_hoc_registration_keeps_its_handler_matcher() {
        let mut reg = HookRegistry::new();
        reg.fail_closed_security = false;
        // No definition matcher; the handler is the only gate.
        reg.register(
            HookEvent::PreToolUse,
            Box::new(MatcherDenyHook {
                tool_pattern: "bash*".into(),
                reason: "bash blocked".into(),
            }),
        );

        let denied = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("bash".into()),
                input: serde_json::json!({}),
            })
            .await;
        let skipped = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(denied.len(), 1);
        assert!(skipped.is_empty());
    }

    #[tokio::test]
    async fn permission_request_fail_closed_without_handlers() {
        let mut reg = HookRegistry::new();
        reg.enable_security_fail_closed();
        assert!(reg.fail_closed_security);
        let responses = reg
            .dispatch(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: "r".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0].decision,
            HookDecision::Deny { .. }
        ));
    }
}
