//! Hook Runtime — Session/Prompt/Tool/Permission/Subagent/Compact/Stop.
//!
//! Security-sensitive events (PreToolUse, PermissionRequest) use fail-closed
//! aggregation: any Deny wins; empty registry for those events denies by default
//! when `fail_closed` is set on the registry.

use crate::event_seq::EventSequencer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

// The Hook data model — event identity, provenance, matching — lives in
// `harness-core`. This module owns only the executable runtime around it.
pub use harness_core::hooks::{
    tool_pattern_matches, Condition, ConditionOperator, HookDefinition, HookEvent,
    HookFailurePolicy, HookId, HookKind, HookScope, HookSource,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDecision {
    Allow,
    Deny { reason: String },
    Modify { payload: Value },
    Inject { messages: Vec<String> },
}

/// The only result an observation-only (post-event) hook may produce.
///
/// `PostToolUse`, `PostToolUseFailure` and `PostCompact` fire AFTER the outcome
/// they observe is already committed (the tool executed, the context
/// compacted). The engine has no channel to rewrite that outcome, so the
/// contract is frozen: a post hook observes, and any decision that claims to
/// alter the outcome (`Deny`/`Modify`/`Inject`) is refused at the dispatch
/// boundary instead of silently ignored (T03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserveResult {
    /// The hook observed the committed outcome and raises no objection.
    Observe,
    /// The hook refused to observe (infrastructure failure) or returned a
    /// decision the post-event contract cannot honour; the caller fails the
    /// run loudly rather than dropping the objection.
    Failed { reason: String },
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
pub struct HookRegistry {
    entries: HashMap<HookEvent, Vec<Registered>>,
    /// When true, security-sensitive events with zero matching handlers deny.
    pub fail_closed_security: bool,
    events: Option<EventSequencer>,
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    /// Empty registry. Security fail-closed is **off** by default so bare
    /// `AgentEngine::new()` tests keep working; call
    /// [`HookRegistry::enable_security_fail_closed`] after registering defaults.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            fail_closed_security: false,
            events: None,
        }
    }

    pub fn with_events(mut self, events: EventSequencer) -> Self {
        self.events = Some(events);
        self
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
            for (ordinal, entry) in entries.iter().enumerate() {
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
                let invocation_id = Uuid::new_v4().to_string();
                let input_json =
                    serde_json::to_string(&request.input).unwrap_or_else(|_| "{}".into());
                let input_truncated = input_json.chars().count() > 512;
                let input_summary = assistant_protocol::v2::redact_secrets(
                    &input_json.chars().take(512).collect::<String>(),
                );
                if let Some(events) = &self.events {
                    if events
                        .append_checked(
                            &request.run_id,
                            assistant_protocol::v2::RunEventKind::HookInvocationStarted {
                                invocation_id: invocation_id.clone(),
                                hook_id: entry.definition.id.to_string(),
                                hook_event: request.event.to_string(),
                                source: entry.definition.source.origin.clone(),
                                ordinal: ordinal as u32,
                                input_summary,
                                input_truncated,
                            },
                        )
                        .is_err()
                    {
                        out.push(HookOutcome::Failed {
                            reason: "hook telemetry persistence failed before handler".into(),
                        });
                        break;
                    }
                }
                let started = Instant::now();
                let outcome = entry.handler.handle_outcome(request.clone()).await;
                let (status, decision, error_category, output_summary) = match &outcome {
                    HookOutcome::Decided(response) => (
                        "completed",
                        Some(format!("{:?}", response.decision)),
                        None,
                        "decision".into(),
                    ),
                    HookOutcome::Permission(verdict) => (
                        "completed",
                        Some(format!("{:?}", verdict)),
                        None,
                        "permission".into(),
                    ),
                    HookOutcome::Failed { reason } => (
                        "failed",
                        None,
                        Some("handler_failure".into()),
                        assistant_protocol::v2::redact_secrets(
                            &reason.chars().take(512).collect::<String>(),
                        ),
                    ),
                };
                let mut telemetry_ok = true;
                if let Some(events) = &self.events {
                    if events
                        .append_checked(
                            &request.run_id,
                            assistant_protocol::v2::RunEventKind::HookInvocationCompleted {
                                invocation_id,
                                hook_id: entry.definition.id.to_string(),
                                hook_event: request.event.to_string(),
                                source: entry.definition.source.origin.clone(),
                                ordinal: ordinal as u32,
                                status: status.into(),
                                effective_decision: decision,
                                error_category,
                                duration_ms: started.elapsed().as_millis() as u64,
                                output_truncated: output_summary.chars().count() > 512,
                                output_summary: output_summary.chars().take(512).collect(),
                            },
                        )
                        .is_err()
                    {
                        out.push(HookOutcome::Failed {
                            reason: "hook telemetry persistence failed after handler".into(),
                        });
                        telemetry_ok = false;
                    }
                }
                match outcome {
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
                if !telemetry_ok {
                    break;
                }
            }
        }
        // Fail-closed for security events with no matching handler. A hook that
        // was dropped by `Skip` counts as "no handler" here, so skipping cannot
        // quietly turn a security event into an allow.
        if out.is_empty() && self.fail_closed_security && request.event.is_security_sensitive() {
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

    /// Dispatch an observation-only post event (PostToolUse, PostToolUseFailure,
    /// PostCompact). The observed outcome is immutable, so the only accepted
    /// decision is `Allow`; a hook that returns anything else is refused loudly
    /// (T03) — the caller fails the run rather than silently ignoring the hook's
    /// objection. Infrastructure failures were already resolved through each
    /// hook's failure policy by [`Self::dispatch_outcomes`].
    pub async fn observe(&self, request: HookRequest) -> Result<(), ObserveResult> {
        for outcome in self.dispatch_outcomes(request).await {
            match outcome {
                HookOutcome::Decided(HookResponse {
                    decision: HookDecision::Allow,
                })
                | HookOutcome::Permission(_) => {}
                HookOutcome::Decided(HookResponse { decision }) => {
                    return Err(ObserveResult::Failed {
                        reason: format!(
                            "post-event hook returned {decision:?}, which cannot be honoured \
                             after the observed outcome is committed; refusing loudly instead \
                             of silently ignoring"
                        ),
                    });
                }
                HookOutcome::Failed { reason } => {
                    return Err(ObserveResult::Failed {
                        reason: format!("post-event hook observation failed: {reason}"),
                    });
                }
            }
        }
        Ok(())
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
                HookOutcome::Failed { reason } => return PermissionAggregate::Deny(reason.clone()),
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
#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
