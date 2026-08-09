//! HookRuntime tests (extracted from `hooks.rs` so the module stays focused on
//! the executable runtime; tests are a separate change-reason).

use super::*;

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
        described
            .iter()
            .map(|d| d.id.to_string())
            .collect::<Vec<_>>(),
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
        vec![
            HookEvent::SessionStart,
            HookEvent::PreToolUse,
            HookEvent::Error
        ]
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
    assert!(matches!(responses[0].decision, HookDecision::Deny { .. }));
}

// ---- post-event observation-only contract (T03) ------------------------

/// A post hook that only observes (Allow) must not disturb the run.
#[tokio::test]
async fn observe_accepts_allow_and_permission() {
    let mut reg = HookRegistry::new();
    reg.register(HookEvent::PostToolUse, Box::new(AllowAllHook));
    reg.register_defined(
        defined(HookEvent::PostToolUse, "verdict", HookFailurePolicy::Fail),
        Box::new(VerdictHook(PermissionVerdict::Allow {
            reason: "observed".into(),
        })),
    );
    assert!(reg.observe(request(HookEvent::PostToolUse)).await.is_ok());
}

/// A post hook that tries to deny an already-executed outcome must be
/// refused loudly, not silently dropped.
#[tokio::test]
async fn observe_refuses_deny_loudly() {
    let mut reg = HookRegistry::new();
    reg.register_defined(
        defined(HookEvent::PostToolUse, "blocker", HookFailurePolicy::Fail),
        Box::new(MatcherDenyHook {
            tool_pattern: "*".into(),
            reason: "too late to block".into(),
        }),
    );
    let result = reg.observe(request(HookEvent::PostToolUse)).await;
    match result {
        Err(ObserveResult::Failed { reason }) => {
            assert!(
                reason.contains("cannot be honoured"),
                "refusal must explain the contract, got: {reason}"
            );
        }
        Err(ObserveResult::Observe) => {
            unreachable!("observe() only errors with Failed")
        }
        Ok(()) => panic!("a Deny at a post event must not be silently observed"),
    }
}

/// A post hook that tries to modify the input at a post point is refused the
/// same way — the outcome is immutable after the tool executed.
#[tokio::test]
async fn observe_refuses_modify_loudly() {
    struct Modifier;
    #[async_trait::async_trait]
    impl HookHandler for Modifier {
        async fn handle(&self, _: HookRequest) -> HookResponse {
            HookResponse {
                decision: HookDecision::Modify {
                    payload: serde_json::json!({"rewritten": true}),
                },
            }
        }
    }
    let mut reg = HookRegistry::new();
    reg.register(HookEvent::PostToolUse, Box::new(Modifier));
    assert!(reg.observe(request(HookEvent::PostToolUse)).await.is_err());
}

/// An infrastructure failure is resolved by the failure policy; a `Skip`
/// policy drops the hook entirely, so observation sees no objection.
#[tokio::test]
async fn observe_resolves_failure_through_policy() {
    let mut reg = HookRegistry::new();
    reg.register_defined(
        defined(HookEvent::PostToolUse, "broken", HookFailurePolicy::Skip),
        Box::new(FailingHook),
    );
    assert!(reg.observe(request(HookEvent::PostToolUse)).await.is_ok());
    // With Fail policy the resolved failure is a Deny -> refused loudly.
    let mut reg = HookRegistry::new();
    reg.register_defined(
        defined(HookEvent::PostToolUse, "broken", HookFailurePolicy::Fail),
        Box::new(FailingHook),
    );
    assert!(reg.observe(request(HookEvent::PostToolUse)).await.is_err());
}
