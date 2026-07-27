//! The `NATIVES_PROMPT_CACHE` operator kill switch.
//!
//! This lives in its own test binary on purpose: the switch is read from the
//! process environment, so a test that sets it would otherwise change the
//! answer for every other test running in parallel in the same process.
//! **Keep this file to a single test** for the same reason.

use provider_adapters::capabilities::{
    ProviderMessage, ProviderRequest, RequestControls, PROMPT_CACHE_ENV,
};
use provider_adapters::providers::anthropic::build_messages_body;

fn request() -> ProviderRequest {
    ProviderRequest {
        model: "claude-sonnet-4-5".into(),
        messages: vec![
            ProviderMessage {
                role: "user".into(),
                content: vec![
                    provider_adapters::capabilities::ProviderContentBlock::Text {
                        text: "hi".into(),
                    },
                ],
            },
            ProviderMessage {
                role: "assistant".into(),
                content: vec![
                    provider_adapters::capabilities::ProviderContentBlock::Text {
                        text: "hello".into(),
                    },
                ],
            },
            ProviderMessage {
                role: "user".into(),
                content: vec![
                    provider_adapters::capabilities::ProviderContentBlock::Text {
                        text: "again".into(),
                    },
                ],
            },
        ],
        system_prompt: Some("s".repeat(20_000)),
        tools: None,
        max_tokens: Some(1024),
        temperature: None,
        stream: true,
        structured_output: None,
        controls: Default::default(),
    }
}

fn has_breakpoint(controls: RequestControls) -> bool {
    let mut req = request();
    req.controls = controls;
    serde_json::to_string(&build_messages_body(&req))
        .unwrap()
        .contains("cache_control")
}

#[test]
fn env_kill_switch_disables_prompt_cache_for_every_caller() {
    assert!(
        std::env::var(PROMPT_CACHE_ENV).is_err(),
        "the ambient environment must not preset {PROMPT_CACHE_ENV}"
    );
    assert!(has_breakpoint(RequestControls::default()));
    assert!(has_breakpoint(RequestControls {
        prompt_cache: Some(true),
        ..Default::default()
    }));

    std::env::set_var(PROMPT_CACHE_ENV, "0");
    // The switch outranks an explicit per-request opt-in — that is what makes
    // it usable during an incident without touching any caller.
    assert!(!has_breakpoint(RequestControls::default()));
    assert!(!has_breakpoint(RequestControls {
        prompt_cache: Some(true),
        ..Default::default()
    }));

    // A value that is not a recognised flag must not silently disable caching.
    std::env::set_var(PROMPT_CACHE_ENV, "sure");
    assert!(has_breakpoint(RequestControls::default()));

    std::env::set_var(PROMPT_CACHE_ENV, "on");
    assert!(has_breakpoint(RequestControls::default()));

    std::env::remove_var(PROMPT_CACHE_ENV);
    assert!(has_breakpoint(RequestControls::default()));
}
