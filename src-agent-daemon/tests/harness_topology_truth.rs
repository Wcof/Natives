//! The topology must describe the engine that exists, not the one on the slide.
//!
//! `harness_core::topology::STAGES` is a hand-written constant. Every other
//! fact the Harness control plane serves is derived from a live source — Hook
//! discovery reads real files, the catalog is `HookRegistry::describe()` — so
//! the topology is the one place a claim could quietly become false. A stage
//! table that says `PermissionRequest` fires while the only call site has been
//! deleted is exactly the "fake data" the project forbids, and it would be
//! invisible in every other test.
//!
//! So this file re-derives the dispatch map from the sources and compares.
//!
//! Detection is on `event: HookEvent::X` and `HookEvent::X,` in a
//! `HookRequest`-shaped construction, which is how every real dispatch site in
//! the tree is written. It is a lexical scan, so it can be fooled by a
//! sufficiently creative refactor — but not by the failure mode that matters:
//! a call site being deleted, moved to another crate, or never written.
//!
//! When this test fails, fix `topology.rs`. Do not relax the test: a hook point
//! marked dispatched that is not is a UI that invites a user to configure
//! something inert.

use harness_core::hooks::HookEvent;
use harness_core::topology::{TriggerSite, STAGES};
use std::collections::BTreeMap;
use std::path::Path;

/// Sources that may legitimately dispatch a Hook, mapped to the module path
/// `topology.rs` names for them.
const DISPATCH_SOURCES: &[(&str, &str)] = &[
    ("../crates/agent-core/src/engine.rs", "agent-core::engine"),
    (
        "../crates/agent-core/src/hook_handlers.rs",
        "agent-core::engine",
    ),
    (
        "src/production_tools.rs",
        "natives-agent-daemon::production_tools",
    ),
];

/// Strip `//` line comments and `#[cfg(test)]` bodies would be nicer, but a
/// dispatch inside a test module is still a real construction of the event and
/// would make the scan over-report. Instead, cut everything from the first
/// `#[cfg(test)]` marker — every file in this tree puts its unit tests last.
fn production_source(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let body = raw
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(&raw)
        .to_string();
    Some(
        body.lines()
            .map(|line| match line.find("//") {
                Some(index) => &line[..index],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Whether `source` names the exact variant, not a longer one that starts the
/// same way. Without this, `HookEvent::Stop` would match `HookEvent::StopFailure`
/// and `PostToolUse` would match `PostToolUseFailure`, quietly making the scan
/// unable to detect the very confusion it exists to prevent.
fn names_variant(source: &str, needle: &str) -> bool {
    source.match_indices(needle).any(|(index, _)| {
        source[index + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
    })
}

/// Which module, if any, constructs a dispatch for `event`.
///
/// Order matters: `DISPATCH_SOURCES` lists the engine loop first, so an event
/// referenced from both the loop and a helper is attributed to the loop.
fn observed_dispatch(event: HookEvent) -> Option<&'static str> {
    let needle = format!("HookEvent::{}", event.as_str());
    for (path, module) in DISPATCH_SOURCES {
        let Some(source) = production_source(Path::new(path)) else {
            continue;
        };
        if names_variant(&source, &needle) {
            return Some(module);
        }
    }
    None
}

/// Skip the whole file when the sources are not checked out beside the daemon,
/// matching how `rpc_dispatch_contract.rs` treats a missing `src-tauri`.
fn sources_present() -> bool {
    DISPATCH_SOURCES
        .iter()
        .all(|(path, _)| Path::new(path).exists())
}

#[test]
fn every_declared_trigger_site_has_a_real_call_site() {
    if !sources_present() {
        eprintln!("skipping: engine sources not present next to the daemon");
        return;
    }

    let mut wrong: Vec<String> = Vec::new();
    for stage in STAGES {
        for point in stage.hook_points {
            let observed = observed_dispatch(point.event);
            match (point.trigger, observed) {
                (TriggerSite::NotDispatched, None) => {}
                (TriggerSite::NotDispatched, Some(module)) => wrong.push(format!(
                    "{} is marked NotDispatched but {module} dispatches it — the \
                     catalog is understating the engine",
                    point.event
                )),
                (declared, None) => wrong.push(format!(
                    "{} claims {:?} but no call site constructs it — mark it \
                     NotDispatched rather than promising a hook point that \
                     cannot fire",
                    point.event,
                    declared.module()
                )),
                (declared, Some(module)) => {
                    if declared.module() != Some(module) {
                        wrong.push(format!(
                            "{} claims {:?} but is dispatched from {module}",
                            point.event,
                            declared.module()
                        ));
                    }
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "harness_core::topology::STAGES disagrees with the engine sources:\n  {}",
        wrong.join("\n  ")
    );
}

/// The inert pair is the headline finding this scan exists to keep honest: two
/// of the sixteen contract events have no producer anywhere. Pinning the exact
/// set means implementing subagent hooks turns this test red, which is the
/// prompt to update the topology in the same change.
#[test]
fn exactly_two_events_have_no_dispatch_site() {
    if !sources_present() {
        eprintln!("skipping: engine sources not present next to the daemon");
        return;
    }

    let undispatched: Vec<&str> = HookEvent::ALL
        .into_iter()
        .filter(|event| observed_dispatch(*event).is_none())
        .map(HookEvent::as_str)
        .collect();
    assert!(
        undispatched.is_empty(),
        "every declared Hook event must have a real dispatch site, but these do \
         not: {undispatched:?}. Either wire them or mark them NotDispatched in \
         harness_core::topology::STAGES — a declared-but-unreachable event is a \
         promise the runtime does not keep."
    );
}

/// Three events are dispatched by the Daemon's tool path rather than by the
/// engine loop. A UI that lumps them under "the engine" would mislead a user
/// debugging why a permission Hook did not fire during a plain chat turn.
#[test]
fn the_permission_and_notification_points_live_outside_the_engine_loop() {
    if !sources_present() {
        eprintln!("skipping: engine sources not present next to the daemon");
        return;
    }

    let outside: BTreeMap<&str, &str> = HookEvent::ALL
        .into_iter()
        .filter_map(|event| {
            observed_dispatch(event)
                .filter(|module| *module == "natives-agent-daemon::production_tools")
                .map(|module| (event.as_str(), module))
        })
        .collect();
    assert_eq!(
        outside.keys().copied().collect::<Vec<_>>(),
        vec![
            "Notification",
            "PermissionDenied",
            "PermissionRequest",
            "SubagentStart",
            "SubagentStop",
        ]
    );
}
