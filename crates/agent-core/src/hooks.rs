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
    /// Historical variant with **no engine implementation**.
    ///
    /// Nothing constructs this any more: `parse_hook_stdout` used to turn
    /// `{"decision":"rewake"}` into it and the engine matched it into an empty
    /// arm, so a hook asking to resume a finished Run was silently discarded.
    /// A promise the runtime does not keep is worse than a missing feature, so
    /// the producer was removed and `rewake` is now refused out loud (see
    /// `hook_handlers::parse_hook_stdout`).
    ///
    /// The variant itself survives only so the engine's match arm keeps
    /// compiling while the removal is coordinated; do not add producers.
    Rewake,
}

/// What a hook wants to happen to a permission prompt.
///
/// `HookDecision` cannot express either of these: its `Allow` means "no
/// objection, carry on with the normal flow", which is *not* the same as
/// "approve this without asking the human".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionVerdict {
    /// Claude `permissionDecision: "allow"` — approve without prompting.
    ///
    /// This only ever *skips a confirmation the profile would have asked for*.
    /// It is never a grant: the permission profile ceiling is enforced by the
    /// consumer and a hook cannot raise it.
    Allow { reason: String },
    /// Claude `permissionDecision: "ask"` — force the prompt even if some other
    /// hook would have skipped it.
    Ask { reason: String },
}

/// What one hook produced for a dispatch.
///
/// Handlers may report an infrastructure `Failed` (spawn error, timeout,
/// oversized IO) separately from a deliberate `Deny`, which is what makes
/// [`HookFailurePolicy`] implementable: without the distinction every failure
/// looks like a decision and `Skip` / `Default` have nothing to act on.
///
/// [`HookRegistry::dispatch_outcomes`] resolves `Failed` through the policy and
/// therefore never returns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    Decided(HookResponse),
    Permission(PermissionVerdict),
    Failed { reason: String },
}

impl HookOutcome {
    /// Collapse to the engine-facing decision, fail-closed.
    ///
    /// A permission verdict has no `HookDecision` spelling, so it degrades to
    /// `Allow` — "no objection", which routes back into the normal permission
    /// flow. Degrading must never be able to skip a prompt.
    pub fn into_response(self) -> HookResponse {
        match self {
            HookOutcome::Decided(response) => response,
            HookOutcome::Permission(_) => HookResponse {
                decision: HookDecision::Allow,
            },
            HookOutcome::Failed { reason } => HookResponse {
                decision: HookDecision::Deny { reason },
            },
        }
    }
}

/// Aggregated verdict for a permission prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionAggregate {
    /// Block the tool call outright.
    Deny(String),
    /// Run the normal permission flow (profile decides: ask / auto / deny).
    Prompt,
    /// Skip the confirmation, subject to the caller's profile ceiling.
    AutoApprove(String),
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

    /// Richer outcome: lets a handler distinguish "I failed" from "I denied",
    /// and report a permission verdict `HookDecision` cannot carry.
    ///
    /// The default treats every result as a deliberate decision, which is
    /// correct for in-process handlers — they cannot time out or fail to spawn.
    async fn handle_outcome(&self, request: HookRequest) -> HookOutcome {
        HookOutcome::Decided(self.handle(request).await)
    }

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

    /// Dispatch, resolving each hook's failure through its [`HookFailurePolicy`].
    ///
    /// Never returns [`HookOutcome::Failed`] — the policy has already turned it
    /// into a decision or dropped the hook.
    pub async fn dispatch_outcomes(&self, request: HookRequest) -> Vec<HookOutcome> {
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
                match entry.handler.handle_outcome(request.clone()).await {
                    HookOutcome::Failed { reason } => {
                        if let Some(resolved) = resolve_failure(
                            request.event,
                            entry.definition.failure_policy,
                            &entry.definition.id.to_string(),
                            reason,
                        ) {
                            out.push(resolved);
                        }
                    }
                    other => out.push(other),
                }
            }
        }
        // Fail-closed for security events with no matching handler. A hook that
        // was dropped by `Skip` counts as "no handler" here, so skipping cannot
        // quietly turn a security event into an allow.
        if out.is_empty()
            && self.fail_closed_security
            && request.event.is_security_sensitive()
        {
            out.push(HookOutcome::Decided(HookResponse {
                decision: HookDecision::Deny {
                    reason: format!(
                        "fail-closed: no hook handler allowed {:?} for tool {:?}",
                        request.event, request.tool_name
                    ),
                },
            }));
        }
        out
    }

    pub async fn dispatch(&self, request: HookRequest) -> Vec<HookResponse> {
        self.dispatch_outcomes(request)
            .await
            .into_iter()
            .map(HookOutcome::into_response)
            .collect()
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

    /// Fold hook outcomes into a single permission verdict, fail-closed.
    ///
    /// Precedence, strongest first — the ordering *is* the security property:
    ///
    /// 1. `Deny` (including a resolved failure and the fail-closed empty case).
    ///    One denying hook beats any number of allowing ones.
    /// 2. `Ask` — an explicit request for the prompt beats an `Allow`.
    /// 3. `Allow` — only when nothing denied and nothing asked.
    /// 4. otherwise `Prompt`, i.e. the pre-existing behaviour.
    ///
    /// `AutoApprove` is a *request* to skip the prompt. The caller still has to
    /// check it against the permission profile; this function deliberately
    /// knows nothing about profiles so it cannot be the place a ceiling leaks.
    pub fn aggregate_permission(outcomes: &[HookOutcome]) -> PermissionAggregate {
        let mut approve: Option<String> = None;
        let mut ask = false;
        for outcome in outcomes {
            match outcome {
                HookOutcome::Decided(HookResponse {
                    decision: HookDecision::Deny { reason },
                }) => return PermissionAggregate::Deny(reason.clone()),
                // A handler-level failure that reached here unresolved is still
                // a failure: deny. `dispatch_outcomes` normally resolves these,
                // so this arm only guards hand-built inputs.
                HookOutcome::Failed { reason } => {
                    return PermissionAggregate::Deny(reason.clone())
                }
                HookOutcome::Permission(PermissionVerdict::Ask { .. }) => ask = true,
                HookOutcome::Permission(PermissionVerdict::Allow { reason }) => {
                    if approve.is_none() {
                        approve = Some(reason.clone());
                    }
                }
                HookOutcome::Decided(_) => {}
            }
        }
        match approve {
            Some(reason) if !ask => PermissionAggregate::AutoApprove(reason),
            _ => PermissionAggregate::Prompt,
        }
    }

    pub fn events_covered(&self) -> Vec<HookEvent> {
        self.entries.keys().copied().collect()
    }
}

/// Apply a hook's [`HookFailurePolicy`] to an infrastructure failure.
///
/// `None` means "drop this hook from the dispatch" (`Skip`).
///
/// Security-sensitive events ignore the configured policy and always deny, as
/// [`HookFailurePolicy`]'s own contract states. Otherwise a hook author could
/// downgrade the fail-closed guarantee on `PreToolUse` / `PermissionRequest`
/// just by writing `failure_policy: "default"` in `hooks.json`, and a hook that
/// merely has to be *made* to time out would become a permission bypass.
fn resolve_failure(
    event: HookEvent,
    policy: HookFailurePolicy,
    hook_id: &str,
    reason: String,
) -> Option<HookOutcome> {
    if event.is_security_sensitive() {
        return Some(HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny {
                reason: format!("hook {hook_id} failed on a security event: {reason}"),
            },
        }));
    }
    match policy {
        HookFailurePolicy::Fail => Some(HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny { reason },
        })),
        HookFailurePolicy::Skip => None,
        HookFailurePolicy::Default => Some(HookOutcome::Decided(HookResponse {
            decision: HookDecision::Allow,
        })),
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

    /// Always reports an infrastructure failure, so failure policies are testable
    /// without spawning a real process.
    struct FailingHook;

    #[async_trait::async_trait]
    impl HookHandler for FailingHook {
        async fn handle(&self, _request: HookRequest) -> HookResponse {
            HookResponse {
                decision: HookDecision::Deny {
                    reason: "failed".into(),
                },
            }
        }

        async fn handle_outcome(&self, _request: HookRequest) -> HookOutcome {
            HookOutcome::Failed {
                reason: "simulated hook failure".into(),
            }
        }
    }

    /// Returns a fixed permission verdict.
    struct VerdictHook(PermissionVerdict);

    #[async_trait::async_trait]
    impl HookHandler for VerdictHook {
        async fn handle(&self, _request: HookRequest) -> HookResponse {
            HookResponse {
                decision: HookDecision::Allow,
            }
        }

        async fn handle_outcome(&self, _request: HookRequest) -> HookOutcome {
            HookOutcome::Permission(self.0.clone())
        }
    }

    fn defined(event: HookEvent, name: &str, policy: HookFailurePolicy) -> HookDefinition {
        let source = HookSource::builtin(name);
        HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order: 0,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: policy,
            kind: HookKind::Builtin { name: name.into() },
        }
    }

    fn request(event: HookEvent) -> HookRequest {
        HookRequest {
            event,
            run_id: "r".into(),
            tool_name: Some("write_file".into()),
            input: serde_json::json!({}),
        }
    }

    fn allow_verdict() -> HookOutcome {
        HookOutcome::Permission(PermissionVerdict::Allow {
            reason: "hook approved".into(),
        })
    }

    fn deny_outcome(reason: &str) -> HookOutcome {
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny {
                reason: reason.into(),
            },
        })
    }

    // ---- permission aggregation -------------------------------------------

    #[test]
    fn permission_allow_alone_auto_approves() {
        assert_eq!(
            HookRegistry::aggregate_permission(&[allow_verdict()]),
            PermissionAggregate::AutoApprove("hook approved".into())
        );
    }

    /// The load-bearing security property: one deny beats any number of allows,
    /// in either order.
    #[test]
    fn deny_beats_allow_in_both_orders() {
        assert_eq!(
            HookRegistry::aggregate_permission(&[allow_verdict(), deny_outcome("nope")]),
            PermissionAggregate::Deny("nope".into())
        );
        assert_eq!(
            HookRegistry::aggregate_permission(&[deny_outcome("nope"), allow_verdict()]),
            PermissionAggregate::Deny("nope".into())
        );
    }

    /// An explicit `ask` outranks an allow: a hook that wants the human in the
    /// loop cannot be overridden by a more permissive sibling.
    #[test]
    fn ask_beats_allow_in_both_orders() {
        let ask = HookOutcome::Permission(PermissionVerdict::Ask {
            reason: "human please".into(),
        });
        assert_eq!(
            HookRegistry::aggregate_permission(&[allow_verdict(), ask.clone()]),
            PermissionAggregate::Prompt
        );
        assert_eq!(
            HookRegistry::aggregate_permission(&[ask, allow_verdict()]),
            PermissionAggregate::Prompt
        );
    }

    #[test]
    fn unresolved_failure_denies() {
        assert_eq!(
            HookRegistry::aggregate_permission(&[
                allow_verdict(),
                HookOutcome::Failed {
                    reason: "boom".into()
                }
            ]),
            PermissionAggregate::Deny("boom".into())
        );
    }

    #[test]
    fn no_opinion_prompts() {
        let plain = HookOutcome::Decided(HookResponse {
            decision: HookDecision::Allow,
        });
        assert_eq!(
            HookRegistry::aggregate_permission(&[plain]),
            PermissionAggregate::Prompt
        );
        assert_eq!(
            HookRegistry::aggregate_permission(&[]),
            PermissionAggregate::Prompt
        );
    }

    /// A failing hook on `PermissionRequest` must deny, whatever an allowing
    /// sibling says.
    #[tokio::test]
    async fn failing_permission_hook_denies_despite_an_allowing_sibling() {
        let mut reg = HookRegistry::new();
        reg.register_defined(
            defined(HookEvent::PermissionRequest, "ok", HookFailurePolicy::Fail),
            Box::new(VerdictHook(PermissionVerdict::Allow {
                reason: "approved".into(),
            })),
        );
        reg.register_defined(
            // Even the most permissive policy must not apply here.
            defined(
                HookEvent::PermissionRequest,
                "broken",
                HookFailurePolicy::Default,
            ),
            Box::new(FailingHook),
        );
        let outcomes = reg
            .dispatch_outcomes(request(HookEvent::PermissionRequest))
            .await;
        assert!(matches!(
            HookRegistry::aggregate_permission(&outcomes),
            PermissionAggregate::Deny(_)
        ));
    }

    /// `Skip` must not be usable to empty out a security event into an allow.
    #[tokio::test]
    async fn skipping_the_only_security_hook_still_fails_closed() {
        let mut reg = HookRegistry::new();
        reg.enable_security_fail_closed();
        reg.register_defined(
            defined(
                HookEvent::PermissionRequest,
                "broken",
                HookFailurePolicy::Skip,
            ),
            Box::new(FailingHook),
        );
        let outcomes = reg
            .dispatch_outcomes(request(HookEvent::PermissionRequest))
            .await;
        assert!(matches!(
            HookRegistry::aggregate_permission(&outcomes),
            PermissionAggregate::Deny(_)
        ));
    }

    // ---- failure policy ---------------------------------------------------

    async fn outcomes_for(policy: HookFailurePolicy) -> Vec<HookOutcome> {
        let mut reg = HookRegistry::new();
        reg.register_defined(
            defined(HookEvent::PostToolUse, "broken", policy),
            Box::new(FailingHook),
        );
        reg.dispatch_outcomes(request(HookEvent::PostToolUse)).await
    }

    #[tokio::test]
    async fn failure_policy_fail_denies() {
        let responses: Vec<HookResponse> = outcomes_for(HookFailurePolicy::Fail)
            .await
            .into_iter()
            .map(HookOutcome::into_response)
            .collect();
        assert_eq!(responses.len(), 1);
        assert!(HookRegistry::aggregate_allow(&responses).is_err());
    }

    #[tokio::test]
    async fn failure_policy_skip_drops_the_hook() {
        assert!(
            outcomes_for(HookFailurePolicy::Skip).await.is_empty(),
            "Skip must remove the hook from the dispatch entirely"
        );
    }

    #[tokio::test]
    async fn failure_policy_default_allows() {
        let responses: Vec<HookResponse> = outcomes_for(HookFailurePolicy::Default)
            .await
            .into_iter()
            .map(HookOutcome::into_response)
            .collect();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0].decision, HookDecision::Allow));
        assert!(HookRegistry::aggregate_allow(&responses).is_ok());
    }

    /// The remaining hooks still decide after one is skipped.
    #[tokio::test]
    async fn skip_leaves_siblings_in_charge() {
        let mut reg = HookRegistry::new();
        reg.register_defined(
            defined(HookEvent::PostToolUse, "broken", HookFailurePolicy::Skip),
            Box::new(FailingHook),
        );
        reg.register_defined(
            defined(HookEvent::PostToolUse, "guard", HookFailurePolicy::Fail),
            Box::new(MatcherDenyHook {
                tool_pattern: "*".into(),
                reason: "sibling denied".into(),
            }),
        );
        let responses = reg.dispatch(request(HookEvent::PostToolUse)).await;
        assert_eq!(responses.len(), 1);
        assert_eq!(
            HookRegistry::aggregate_allow(&responses).unwrap_err(),
            "sibling denied"
        );
    }

    /// Security events ignore the configured policy, so a hook author cannot
    /// downgrade fail-closed by writing `failure_policy` in `hooks.json`.
    #[tokio::test]
    async fn security_events_ignore_lenient_failure_policies() {
        for policy in [
            HookFailurePolicy::Fail,
            HookFailurePolicy::Skip,
            HookFailurePolicy::Default,
        ] {
            for event in [HookEvent::PreToolUse, HookEvent::PermissionRequest] {
                let mut reg = HookRegistry::new();
                reg.register_defined(defined(event, "broken", policy), Box::new(FailingHook));
                let responses = reg.dispatch(request(event)).await;
                assert_eq!(responses.len(), 1, "{event:?}/{policy:?}");
                assert!(
                    HookRegistry::aggregate_allow(&responses).is_err(),
                    "{event:?} with policy {policy:?} must still fail closed"
                );
            }
        }
    }

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
