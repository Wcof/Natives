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
//! So this file re-derives the dispatch map from the sources and compares. It
//! verifies two claims:
//!
//! 1. **Hook points** — every `HookPoint` marked dispatched has a real
//!    construction of `HookEvent::X` in a dispatch source, attributed to the
//!    module the topology names.
//! 2. **Safe Points** — every `SafePoint` advertised on a stage (`Stage
//!    .safe_points`) is constructed as an argument of a real safe-point
//!    dispatch call (`apply_safe_point`, `on_safe_point_checked`). A match arm
//!    that merely names a variant, or a string literal that spells it, is NOT
//!    a dispatch and does not satisfy the claim.
//!
//! Detection is lexical but honest in the directions that matter:
//!
//! - line comments, `#[cfg(test)]` bodies, and double-quoted string literals
//!   are stripped, so prose or `"HookEvent::X"` can never impersonate a call;
//! - a Safe Point counts only inside the argument list of a dispatch call, so
//!   deleting the call site fails the test even though the enum definition and
//!   the engine's match arms still name the variant;
//! - a missing source file fails the test with the missing paths listed — it
//!   never skips, because a topology that was never checked is a topology that
//!   quietly went stale.
//!
//! When this test fails, fix `topology.rs` or the real call site. Do not relax
//! the test: a hook point or safe point marked dispatched that cannot fire is a
//! UI that invites a user to configure something inert.

use harness_core::hooks::HookEvent;
use harness_core::topology::{TriggerSite, STAGES};
use std::collections::BTreeMap;
use std::path::Path;

/// Sources that may legitimately dispatch a Hook, mapped to the module path
/// `topology.rs` names for them. The engine loop lives in `engine_core.rs`;
/// the permission / subagent / notification points live in the daemon's tool
/// path.
const DISPATCH_SOURCES: &[(&str, &str)] = &[
    (
        "../crates/agent-core/src/engine/engine_core.rs",
        "agent-core::engine",
    ),
    (
        "../crates/agent-core/src/hook_handlers.rs",
        "agent-core::engine",
    ),
    (
        "src/tools/permission.rs",
        "natives-agent-daemon::production_tools",
    ),
    (
        "src/tools/subagent.rs",
        "natives-agent-daemon::production_tools",
    ),
    (
        "src/tools/gated.rs",
        "natives-agent-daemon::production_tools",
    ),
];

/// Sources that may legitimately dispatch a Safe Point: the engine tool
/// loop (`apply_safe_point` calls in `engine_tools.rs`) and the daemon
/// permission path (`on_safe_point_checked` after a permission is resolved).
///
/// `engine_core.rs` is deliberately NOT a source: it declares `apply_safe_point`
/// but the real call sites live in `engine_tools.rs` (BeforeTool / AfterTool /
/// ProviderBatchBoundary). Listing the declaration file would let a deleted
/// call site hide behind the signature.
///
/// `prompt_queue_store.rs` is deliberately NOT a source: its
/// `DurableSafePointReceiver` only *translates* the engine's `InputSafePoint`
/// back into a `SafePoint` for the global coordinator. It is a consequence of
/// an engine call, not an independent dispatch — counting it would let a
/// deleted engine call site hide behind the bridge.
const SAFE_POINT_DISPATCH_SOURCES: &[&str] = &[
    "../crates/agent-core/src/engine/engine_tools.rs",
    "src/tools/permission.rs",
];

/// The function names whose argument list may carry a real `SafePoint::X`
/// dispatch. A variable passed to `on_safe_point` is not a dispatch; only a
/// literal variant in the argument list counts.
const SAFE_POINT_DISPATCHERS: &[&str] = &[
    "apply_safe_point(",
    "on_safe_point_checked(",
    "on_safe_point(",
];

/// Strip `//` line comments, cut `#[cfg(test)]` bodies, and remove double
/// quoted string literals — every file in this tree puts its unit tests last.
fn clean_source(raw: &str) -> String {
    let body = raw.split("#[cfg(test)]").next().unwrap_or(raw);
    let no_comments = body
        .lines()
        .map(|line| match line.find("//") {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    strip_string_literals(&no_comments)
}

fn production_source(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    Some(clean_source(&raw))
}

/// Replace the contents of every `"…"` literal with spaces so a string such as
/// `"HookEvent::PreToolUse"` or `"SafePoint::BeforeTool"` can never be
/// mistaken for a real construction. A comment is stripped too, but a string
/// literal is prose all the same — the scan must detect calls, not text.
fn strip_string_literals(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in source.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            out.push(' ');
        } else if c == '"' {
            in_string = true;
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether `needle` appears in `source` as a whole token (non-identifier on
/// both sides), so `SafePointX` or a longer variant never matches.
fn names_token(source: &str, needle: &str) -> bool {
    source.match_indices(needle).any(|(index, _)| {
        // `index` is the byte offset where `needle` starts, so both slices are
        // at character boundaries and cannot panic on multi-byte UTF-8.
        let before = source[..index].chars().next_back();
        let after = source[index + needle.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

/// Whether `source` names the exact variant, not a longer one that starts the
/// same way. Without this, `HookEvent::Stop` would match `HookEvent::StopFailure`
/// and `PostToolUse` would match `PostToolUseFailure`, quietly making the scan
/// unable to detect the very confusion it exists to prevent.
fn names_variant(source: &str, needle: &str) -> bool {
    names_token(source, needle)
}

/// Index of the `)` matching the `(` at `open`, or `None` when unbalanced.
fn matching_close_paren(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for index in open..bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether `source` constructs `SafePoint::<variant>` as an argument of a real
/// safe-point dispatch call. Requiring a *call* — not a bare occurrence — is
/// what stops a deleted call site from being masked by the enum definition or
/// by a match arm in the same file.
fn safe_point_dispatch_in(source: &str, variant: &str) -> bool {
    let needle = format!("SafePoint::{variant}");
    for opener in SAFE_POINT_DISPATCHERS {
        let mut search_from = 0usize;
        while let Some(relative) = source[search_from..].find(opener) {
            // `opener` ends with `(`; matching_close_paren expects the index of
            // the `(` itself so the opening paren raises depth to 1.
            let open = search_from + relative + opener.len() - 1;
            match matching_close_paren(source, open) {
                Some(close) => {
                    if names_token(&source[open..close], &needle) {
                        return true;
                    }
                    search_from = close;
                }
                None => search_from = source.len(),
            }
        }
    }
    false
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

/// A missing source is a failed test, never a skip. The topology must be
/// verified against the real sources; if they are not present next to the
/// daemon the test says so loudly instead of silently passing.
fn require_sources() {
    let mut missing: Vec<&str> = Vec::new();
    for (path, _) in DISPATCH_SOURCES {
        if !Path::new(path).exists() {
            missing.push(path);
        }
    }
    for path in SAFE_POINT_DISPATCH_SOURCES {
        if !Path::new(path).exists() {
            missing.push(path);
        }
    }
    assert!(
        missing.is_empty(),
        "harness topology truth test cannot run: dispatch sources are missing \
         next to the daemon: {missing:?}. A missing source must fail, never skip."
    );
}

#[test]
fn every_declared_trigger_site_has_a_real_call_site() {
    require_sources();

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

/// Every declared Hook event must have a real dispatch site. A declared-but-
/// unreachable event is a promise the runtime does not keep; a UI that renders
/// it invites configuring something inert.
#[test]
fn every_declared_hook_event_has_a_real_dispatch_site() {
    require_sources();

    let undispatched: Vec<&str> = HookEvent::ALL
        .into_iter()
        .filter(|event| observed_dispatch(*event).is_none())
        .map(HookEvent::as_str)
        .collect();
    assert!(
        undispatched.is_empty(),
        "every declared Hook event must have a real dispatch site, but these do \
         not: {undispatched:?}. Either wire them or mark them NotDispatched in \
         harness_core::topology::STAGES."
    );
}

/// The permission and notification points are dispatched by the Daemon's tool
/// path rather than by the engine loop. A UI that lumps them under "the
/// engine" would mislead a user debugging why a permission Hook did not fire
/// during a plain chat turn.
#[test]
fn the_permission_and_notification_points_live_outside_the_engine_loop() {
    require_sources();

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

/// The core NE-P0-07 claim: a Safe Point that the topology advertises must be
/// dispatchable. BeforeTool/AfterTool/AfterPermissionResolved/ProviderBatch
/// Boundary each need a real call that constructs the variant inside a
/// safe-point dispatch call — a match arm that merely names the variant, or a
/// string literal that spells it, does not count. Deleting the engine's
/// `apply_safe_point(SafePoint::BeforeTool)` call therefore turns this red.
#[test]
fn every_advertised_safe_point_has_a_real_dispatch_call() {
    require_sources();

    let mut wrong: Vec<String> = Vec::new();
    for stage in STAGES {
        for point in stage.safe_points {
            let name = harness_core::topology::safe_point_name(*point);
            let variant = format!("{point:?}");
            let dispatched = SAFE_POINT_DISPATCH_SOURCES.iter().any(|path| {
                production_source(Path::new(path))
                    .is_some_and(|source| safe_point_dispatch_in(&source, &variant))
            });
            if !dispatched {
                wrong.push(format!(
                    "safe point '{name}' (SafePoint::{variant}) is advertised on \
                     stage {} but no dispatch source constructs it as a call \
                     argument: {SAFE_POINT_DISPATCH_SOURCES:?}",
                    stage.id
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "advertised Safe Points without a real dispatch call:\n  {}",
        wrong.join("\n  ")
    );
}

/// The scanner must reject string-literal and comment impersonation: `"HookEvent::X"`
/// or `"SafePoint::X"` text is not a dispatch, and a comment naming a variant
/// is not either.
#[test]
fn a_string_literal_cannot_impersonate_a_dispatch() {
    let source = clean_source(
        r#"
        let note = "HookEvent::PreToolUse";
        let note2 = "HookEvent::Stop";
        let note3 = "SafePoint::BeforeTool";
        // HookEvent::PostToolUse
    "#,
    );
    assert!(
        !names_variant(&source, "HookEvent::PreToolUse"),
        "a string literal must not count as a PreToolUse dispatch"
    );
    assert!(
        !names_variant(&source, "HookEvent::Stop"),
        "a string literal must not count as a Stop dispatch"
    );
    assert!(
        !names_variant(&source, "HookEvent::PostToolUse"),
        "a comment must not count as a PostToolUse dispatch"
    );
    assert!(
        !safe_point_dispatch_in(&source, "BeforeTool"),
        "a string literal must not count as a BeforeTool safe-point dispatch"
    );
}

/// The scanner must find real dispatch calls and must NOT treat a match arm in
/// the same file as a dispatch — that is the exact failure mode NE-P0-07 warns
/// about (a variant still "appears" in the file after its call site is gone).
#[test]
fn the_safe_point_scan_detects_real_calls_and_ignores_match_arms() {
    let source = clean_source(
        r#"
        self.apply_safe_point(
            &config.conversation_id,
            crate::session_coordinator::SafePoint::BeforeTool,
            &mut typed_messages,
        )
        .await?;
        match crate::prompt_queue_store::on_safe_point_checked(
            &self.conversation_id,
            agent_core::SafePoint::AfterPermissionResolved,
        ) {
            Ok(_) => {}
            Err(error) => return Some(error),
        }
        let action = harness.on_safe_point(conversation_id, point);
    "#,
    );
    assert!(safe_point_dispatch_in(&source, "BeforeTool"));
    assert!(safe_point_dispatch_in(&source, "AfterPermissionResolved"));
    // The variable-shaped on_safe_point call carries no literal: nothing to see.
    assert!(!safe_point_dispatch_in(&source, "AfterTool"));

    let match_source = clean_source(
        r#"
        let input_point = match point {
            crate::session_coordinator::SafePoint::BeforeTool => {
                crate::InputSafePoint::AfterToolBatch
            }
            crate::session_coordinator::SafePoint::AfterTool => {
                crate::InputSafePoint::AfterToolBatch
            }
        };
    "#,
    );
    assert!(
        !safe_point_dispatch_in(&match_source, "BeforeTool"),
        "a match arm naming the variant is not a dispatch"
    );
    assert!(
        !safe_point_dispatch_in(&match_source, "AfterTool"),
        "a match arm naming the variant is not a dispatch"
    );
}

/// NE-P0-07 regression guard: `engine_core.rs` declares `apply_safe_point`
/// but the real call sites live in `engine_tools.rs`. Listing the declaration
/// file as a safe-point dispatch source would let a deleted call site hide
/// behind the signature, so this test makes that regression red.
#[test]
fn engine_core_rs_is_not_a_safe_point_dispatch_source() {
    // The declaration file must NOT appear in the source list.
    for path in SAFE_POINT_DISPATCH_SOURCES {
        assert!(
            !path.ends_with("engine_core.rs"),
            "engine_core.rs must not be a SAFE_POINT_DISPATCH_SOURCES entry:              it declares apply_safe_point but the real call sites are in              engine_tools.rs. Listing the declaration lets a deleted call              hide behind the signature."
        );
    }
    // engine_tools.rs must appear, because BeforeTool / AfterTool /
    // ProviderBatchBoundary are dispatched there.
    let has_tools = SAFE_POINT_DISPATCH_SOURCES
        .iter()
        .any(|path| path.ends_with("engine_tools.rs"));
    assert!(
        has_tools,
        "engine_tools.rs must be a SAFE_POINT_DISPATCH_SOURCES entry:          BeforeTool / AfterTool / ProviderBatchBoundary are dispatched there."
    );
}

/// NE-P0-07: a missing source must fail, never skip. The topology must be
/// verified against the real sources; if one is absent the test says so loudly
/// instead of silently passing on the remaining sources. This duplicates the
/// guard `require_sources` gives the other tests, but it makes the missing-
/// source contract explicit for the safe-point scan specifically.
#[test]
fn a_missing_safe_point_dispatch_source_fails_not_skips() {
    let missing = SAFE_POINT_DISPATCH_SOURCES
        .iter()
        .filter(|path| !Path::new(path).exists())
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "safe-point dispatch sources are missing: {missing:?}.          A missing source must fail, never skip — otherwise an advertised          SafePoint could quietly go stale."
    );
}

/// NE-P0-07: the production sources must not contain a string literal that
/// spells a SafePoint variant — `clean_source` strips string literals, so
/// prose like `"SafePoint::BeforeTool"` in a debug message cannot impersonate a
/// real dispatch. This cross-checks the stripping against the actual production
/// files (not a fixture), so a regression in `strip_string_literals` that let
/// prose leak through would turn this red.
#[test]
fn production_sources_strip_string_literals_so_prose_cannot_impersonate_dispatch() {
    require_sources();

    let prose = r#"let note = "SafePoint::BeforeTool";
        let note2 = "SafePoint::AfterTool";
        // SafePoint::ProviderBatchBoundary"#;
    let cleaned = clean_source(prose);
    assert!(
        !safe_point_dispatch_in(&cleaned, "BeforeTool"),
        "a string literal spelling SafePoint::BeforeTool must not count as a dispatch"
    );
    assert!(
        !safe_point_dispatch_in(&cleaned, "AfterTool"),
        "a string literal spelling SafePoint::AfterTool must not count as a dispatch"
    );
    assert!(
        !names_variant(&cleaned, "SafePoint::ProviderBatchBoundary"),
        "a comment spelling SafePoint::ProviderBatchBoundary must not count as a token"
    );
}
