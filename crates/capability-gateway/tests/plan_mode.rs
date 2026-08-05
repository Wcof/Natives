//! Plan Mode contract tests over the *registered* tool surface.
//!
//! The unit tests in `plan_mode.rs` pin the decision function. These pin the
//! thing that actually matters in production: that every tool the gateway hands
//! to a model is classified, and that nothing which can change the machine slips
//! through the latch.

use capability_gateway::plan_mode::{self, PlanDecision};
use capability_gateway::tools::builtin_tools;
use capability_gateway::{CapabilityGateway, SideEffect, ToolCallContext};

/// Tools that keep working while planning. Anything not on this list must be
/// denied — if a new tool needs to be here, that is a deliberate decision and
/// this test is where it gets made.
const EXPECTED_ALLOWED: &[&str] = &[
    "read_file",
    "search_files",
    "list_dir",
    "grep",
    "memory_search",
    "memory_get",
    "task_output",
    "skill",
    "web_fetch",
    "web_search",
];

#[test]
fn every_builtin_has_an_explicit_plan_decision() {
    for tool in builtin_tools() {
        let decision = plan_mode::decision(tool.name, tool.side_effect, tool.permission_class);
        match decision {
            PlanDecision::Control => assert!(
                tool.name == plan_mode::ENTER_PLAN_MODE_TOOL
                    || tool.name == plan_mode::EXIT_PLAN_MODE_TOOL,
                "unexpected control tool: {}",
                tool.name
            ),
            PlanDecision::Allow => assert!(
                EXPECTED_ALLOWED.contains(&tool.name),
                "`{}` is allowed in Plan Mode but is not on the reviewed allow list",
                tool.name
            ),
            PlanDecision::Deny => assert!(
                !EXPECTED_ALLOWED.contains(&tool.name),
                "`{}` should be allowed in Plan Mode but is denied",
                tool.name
            ),
        }
    }
}

#[test]
fn nothing_that_writes_runs_or_spawns_survives_the_latch() {
    for tool in builtin_tools() {
        if matches!(
            tool.side_effect,
            SideEffect::Write | SideEffect::Destructive | SideEffect::Process
        ) && tool.name != plan_mode::ENTER_PLAN_MODE_TOOL
            && tool.name != plan_mode::EXIT_PLAN_MODE_TOOL
        {
            assert_eq!(
                plan_mode::decision(tool.name, tool.side_effect, tool.permission_class),
                PlanDecision::Deny,
                "`{}` has side effect {:?} and must be blocked in Plan Mode",
                tool.name,
                tool.side_effect
            );
        }
    }
}

#[test]
fn plan_control_tools_are_registered() {
    let mut gateway = CapabilityGateway::new();
    let _ = gateway.register_builtins();
    assert!(gateway.get_tool(plan_mode::ENTER_PLAN_MODE_TOOL).is_some());
    assert!(gateway.get_tool(plan_mode::EXIT_PLAN_MODE_TOOL).is_some());
}

#[tokio::test]
async fn exit_plan_mode_through_the_gateway_cannot_release_the_latch() {
    let run_id = format!("gw-exit-{}", uuid::Uuid::new_v4());
    let mut gateway = CapabilityGateway::new();
    let _ = gateway.register_builtins();
    let ctx = ToolCallContext::new(
        std::env::temp_dir(),
        run_id.clone(),
        "conv".into(),
        "tc".into(),
        plan_mode::PLAN_PROFILE.into(),
    );

    gateway
        .execute(
            plan_mode::ENTER_PLAN_MODE_TOOL,
            serde_json::json!({"reason": "risky"}),
            &ctx,
        )
        .await
        .expect("enter_plan_mode must succeed without an orchestrator");
    assert!(plan_mode::is_active(&run_id));

    let err = gateway
        .execute(
            plan_mode::EXIT_PLAN_MODE_TOOL,
            serde_json::json!({
                "plan": {"title": "do it", "steps": [{"title": "write", "kind": "edit"}]}
            }),
            &ctx,
        )
        .await
        .expect_err("exit_plan_mode must not self-approve");
    assert_eq!(err.code, "orchestrator_required");
    assert!(
        plan_mode::is_active(&run_id),
        "the latch must survive a direct exit_plan_mode call"
    );
    plan_mode::clear(&run_id);
}

#[test]
fn web_search_is_absent_until_a_backend_is_configured() {
    capability_gateway::tools::web_search::clear_backend();
    let env_configured = std::env::var(capability_gateway::tools::web_search::ENV_PROVIDER).is_ok();
    let has_tool = builtin_tools().iter().any(|t| t.name == "web_search");
    assert_eq!(
        has_tool, env_configured,
        "web_search must be registered exactly when a backend exists"
    );

    capability_gateway::tools::web_search::install_backend(
        capability_gateway::tools::SearchBackend::new(
            capability_gateway::tools::SearchProvider::Brave,
            "test-key",
        )
        .unwrap(),
    );
    assert!(builtin_tools().iter().any(|t| t.name == "web_search"));
    capability_gateway::tools::web_search::clear_backend();
}
