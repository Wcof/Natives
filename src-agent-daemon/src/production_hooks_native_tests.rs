//! Native Hook unit tests (extracted from `production_hooks.rs`, task-01
//! structure): MCP input-template substitution and the structured Prompt Hook
//! allow/deny decision.

use super::*;

#[test]
fn mcp_template_substitutes_structured_event_input() {
    let mut value = serde_json::json!({"payload": "${input}", "literal": "keep"});
    substitute_hook_input(&mut value, &serde_json::json!({"command": "cargo test"}));
    assert_eq!(value["payload"]["command"], "cargo test");
    assert_eq!(value["literal"], "keep");
}

#[test]
fn prompt_hook_requires_a_structured_allow_or_deny() {
    assert!(matches!(
        prompt_decision(r#"{"decision":"allow","reason":"safe"}"#),
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Allow
        })
    ));
    assert!(matches!(
        prompt_decision(r#"{"decision":"deny","reason":"unsafe"}"#),
        HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny { .. }
        })
    ));
    assert!(matches!(
        prompt_decision("maybe"),
        HookOutcome::Failed { .. }
    ));
}
