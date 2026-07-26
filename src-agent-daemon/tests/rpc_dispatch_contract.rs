//! Dispatch-coverage contract: **advertised ⊆ callable**.
//!
//! `daemon.getCapabilities` advertises `IMPLEMENTED_METHODS ∪ HOST_IMPLEMENTED_METHODS`,
//! and the whole frontend capability gate (`src/lib/assistant-workspace/capability-gate.ts`)
//! is built on the promise that an advertised method is really callable. When a method is
//! advertised but has no dispatch arm it falls into the fail-closed arm at the bottom of
//! `handle_rpc`, which maps `MethodStatus::Implemented` to the error code `internal_error`.
//! The caller then sees "the daemon is broken" instead of an honest "unsupported", and the
//! UI gate has already been opened by the lie. This file makes that state unreachable.
//!
//! Why drive the real `handle_rpc` instead of parsing `rpc.rs` for `names::X =>` arms:
//! a source scan cannot see *routing*. `conversation.listPage` had a handler arm inside
//! `conversation_store::request` and was advertised, yet no top-level arm routed to it —
//! a source scan counts it as covered while a live call still returns `internal_error`.
//! Only real dispatch settles the question.
//!
//! Probe protocol: call each method with deliberately empty params.
//! - A validation / not-found / permission error is a PASS — reaching an argument check
//!   proves the arm exists. This test asserts *reachability*, never behaviour.
//! - Hitting the fail-closed arm is the FAILURE we are pinning.
//! - A handler that blocks past the timeout is a PASS: the fail-closed arm is a pure,
//!   immediate error write, so anything that takes time is definitionally not it.

use assistant_protocol::v1::daemon::RpcRequest;
use assistant_protocol::version::ProtocolVersion;
use assistant_protocol::v2::{HOST_IMPLEMENTED_METHODS, IMPLEMENTED_METHODS};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Methods the **host** intercepts before the daemon ever sees them, so the daemon is
/// correct to have no arm. Every entry needs a reason, and the protocol crate pins this
/// set to exactly `artifact.reveal` (see `host_only_surface_is_os_bound_and_minimal`).
///
/// `artifact.reveal` = "show this file in the OS file manager". A daemon that may run as
/// a sidecar, over a remote socket, or headless has no desktop shell to talk to, so this
/// is genuinely not a daemon capability; `src-tauri`'s `is_host_owned_method` owns it.
const HOST_INTERCEPTED: &[&str] = &["artifact.reveal"];

/// Per-method budget. Generous: a slow handler passing is fine, a missing arm is instant.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Point every store at a throwaway directory **before** the first probe.
///
/// This is load-bearing, not hygiene. Integration tests compile the lib without
/// `cfg(test)`, so `conversation_store::store()`'s "refuse the default path" guard is
/// compiled out and the stores fall back to the developer's real `~/.natives` database.
/// The probe set includes `scheduler.tick`, which fires due jobs — against a real DB that
/// would start real runs from a test. It also must run before the first `run_manager()`
/// call, because the global RunManager caches its DataStore in a `OnceLock`.
fn isolate_env() {
    static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INIT.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("natives-dispatch-{}", uuid::Uuid::new_v4()));
        let runtime = root.join("runtime");
        std::fs::create_dir_all(&runtime).expect("create temp runtime dir");
        let db = root.join("assistant.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", &runtime);
        // Keep probes off any real provider / network path.
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        std::env::set_var("NATIVES_ALLOW_FIXTURE_FALLBACK", "1");
    });
}

enum Probe {
    /// Handler was reached and produced a response.
    Responded { code: Option<String>, body: String },
    /// Handler was reached and is still working. Cannot be the fail-closed arm.
    Blocked,
}

/// Drive one method through the real `handle_rpc` over a live socket pair.
async fn probe(method: &str) -> Probe {
    let (client, server) = tokio::net::UnixStream::pair().expect("socket pair");
    let (_server_read, mut server_write) = server.into_split();

    let request = RpcRequest {
        protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
        request_id: format!("probe-{method}"),
        client_id: "dispatch-contract".to_string(),
        session_token: "probe-session".to_string(),
        method: method.to_string(),
        params: serde_json::json!({}),
    };

    let protocol_version = ProtocolVersion::new(2, 0, 0);
    let started_at = std::time::Instant::now();
    let dispatch = natives_agent_daemon::rpc::handle_rpc(
        &mut server_write,
        &request,
        &protocol_version,
        "0.0.0-test",
        &started_at,
    );

    if tokio::time::timeout(PROBE_TIMEOUT, dispatch).await.is_err() {
        return Probe::Blocked;
    }
    // Drop the write half so the reader sees EOF instead of hanging on a silent handler.
    drop(server_write);

    let mut line = String::new();
    let mut reader = BufReader::new(client);
    let _ = tokio::time::timeout(PROBE_TIMEOUT, reader.read_line(&mut line)).await;

    let code = serde_json::from_str::<serde_json::Value>(line.trim())
        .ok()
        .and_then(|v| {
            v.get("code")
                .or_else(|| v.get("error").and_then(|e| e.get("code")))
                .and_then(|c| c.as_str())
                .map(str::to_string)
        });
    Probe::Responded { code, body: line }
}

/// The signature of the fail-closed arm: an advertised method that no arm handled.
fn is_fail_closed_miss(code: Option<&str>, body: &str) -> bool {
    code == Some("internal_error") && body.contains("method not implemented")
}

#[tokio::test(flavor = "multi_thread")]
async fn every_advertised_daemon_method_has_real_dispatch() {
    isolate_env();

    let mut offenders: Vec<String> = Vec::new();
    for method in IMPLEMENTED_METHODS {
        match probe(method).await {
            Probe::Blocked => {}
            Probe::Responded { code, body } => {
                if is_fail_closed_miss(code.as_deref(), &body) {
                    offenders.push((*method).to_string());
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "IMPLEMENTED_METHODS advertises {} method(s) with no dispatch arm in rpc.rs — \
         callers get `internal_error` instead of an honest `unsupported`, and the frontend \
         capability gate has already been opened by the advertisement: {offenders:?}\n\
         Fix by adding a dispatch arm, or remove the method from IMPLEMENTED_METHODS \
         (keeping it in ALL_METHODS) so the advertisement shrinks honestly.",
        offenders.len(),
    );
}

/// The advertised set is the *union* with the host list, so the host half needs the same
/// guarantee: anything advertised there but not intercepted by the host reaches the daemon
/// and must be dispatchable. Only `HOST_INTERCEPTED` is exempt, with a stated reason.
#[tokio::test(flavor = "multi_thread")]
async fn host_advertised_methods_are_dispatchable_unless_host_intercepted() {
    isolate_env();

    let mut offenders: Vec<String> = Vec::new();
    for method in HOST_IMPLEMENTED_METHODS {
        if HOST_INTERCEPTED.contains(method) {
            continue;
        }
        match probe(method).await {
            Probe::Blocked => {}
            Probe::Responded { code, body } => {
                if is_fail_closed_miss(code.as_deref(), &body) {
                    offenders.push((*method).to_string());
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "HOST_IMPLEMENTED_METHODS advertises {} method(s) that neither the host intercepts \
         nor the daemon dispatches: {offenders:?}\n\
         An entry in HOST_IMPLEMENTED_METHODS is a promise that \
         src-tauri's `is_host_owned_method` catches the call first. If it does not, the call \
         is routed to the daemon and dies in the fail-closed arm. Either add a daemon arm, \
         or add the method to HOST_INTERCEPTED here *and* to `is_host_owned_method`.",
        offenders.len(),
    );
}

/// Guard the exemption list itself: an exemption is only legitimate while the protocol
/// crate still declares the method host-only. This stops `HOST_INTERCEPTED` from decaying
/// into a place where inconvenient failures get parked.
#[test]
fn host_intercepted_exemptions_are_still_host_only() {
    for method in HOST_INTERCEPTED {
        assert!(
            HOST_IMPLEMENTED_METHODS.contains(method),
            "{method} is exempted as host-intercepted but is no longer in \
             HOST_IMPLEMENTED_METHODS — drop the exemption"
        );
        assert!(
            !IMPLEMENTED_METHODS.contains(method),
            "{method} is now daemon-implemented — drop it from HOST_INTERCEPTED so the \
             daemon arm is actually covered"
        );
    }
}

/// The root cause of this bug class: a method is listed as host-owned, the host does not
/// actually intercept it, so the call is routed to the daemon that has no arm for it.
/// Neither list is wrong on its own — they disagree. Pin them against each other.
///
/// Note this is deliberately *not* a "HOST and IMPLEMENTED must be disjoint" assertion.
/// Overlap is real and correct: `run.start` / `run.subscribe` are host-preflighted and
/// then genuinely executed by the daemon, and `artifact.open` is served by both. Demanding
/// disjointness would force removing true daemon arms from the advertisement — a lie in the
/// opposite direction. What must hold is narrower and actually load-bearing: the methods
/// only the host can serve (`HOST \ IMPLEMENTED`) are exactly the ones the host intercepts.
#[test]
fn host_only_methods_are_actually_intercepted_by_the_tauri_host() {
    // cwd for an integration test is the package root (`src-agent-daemon/`).
    let service = std::path::Path::new("../src-tauri/src/assistant_service.rs");
    let Ok(src) = std::fs::read_to_string(service) else {
        // The daemon must stay buildable without the Tauri host checked out.
        eprintln!("skipping: {} not present", service.display());
        return;
    };
    let body = src
        .split("fn is_host_owned_method")
        .nth(1)
        .and_then(|rest| rest.split("}\n").next())
        .unwrap_or_default();

    for method in HOST_IMPLEMENTED_METHODS {
        if IMPLEMENTED_METHODS.contains(method) {
            continue; // daemon can serve it too — interception is an optimisation, not a need.
        }
        assert!(
            body.contains(&format!("\"{method}\"")),
            "{method} is advertised as host-only but src-tauri's `is_host_owned_method` does \
             not intercept it. The call will be routed to the daemon, which has no dispatch \
             arm for it, and the caller gets `internal_error`. Either intercept it in \
             src-tauri, or implement + advertise it on the daemon."
        );
    }
}

/// Deliberately-unsupported methods must stay *out* of the advertised set, so that a real
/// call returns the honest `unsupported` code. This is the other half of the invariant:
/// the first test stops us over-advertising, this one stops the red lines from drifting in.
#[tokio::test(flavor = "multi_thread")]
async fn deliberately_unsupported_methods_report_unsupported() {
    isolate_env();

    // OAuth browser/redirect is an explicit product red line, not an oversight:
    // the daemon has no browser and no redirect listener.
    for method in ["mcp.auth.oauthStart", "mcp.auth.oauthCallback"] {
        assert!(
            !IMPLEMENTED_METHODS.contains(&method) && !HOST_IMPLEMENTED_METHODS.contains(&method),
            "{method} must not be advertised — no OAuth browser flow exists"
        );
        match probe(method).await {
            Probe::Blocked => panic!("{method} unexpectedly has a working handler"),
            Probe::Responded { code, body } => {
                assert_eq!(
                    code.as_deref(),
                    Some("unsupported"),
                    "{method} should fail closed as `unsupported`, got: {body}"
                );
            }
        }
    }
}

/// The MCP surface beyond `tools` is the newest advertisement, so pin it by name
/// rather than relying only on the blanket sweep above. If one of these is ever
/// dropped from `IMPLEMENTED_METHODS` the sweep goes quiet — it only checks what
/// is advertised — and the frontend loses the capability with no test turning red.
/// Naming them here makes removal a deliberate edit to this list.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_protocol_surface_is_advertised_and_dispatchable() {
    isolate_env();

    const MCP_SURFACE: &[&str] = &[
        "mcp.resources.list",
        "mcp.resources.read",
        "mcp.resources.templates.list",
        "mcp.prompts.list",
        "mcp.prompts.get",
        "mcp.roots.list",
        "mcp.notifications.list",
    ];

    for method in MCP_SURFACE {
        assert!(
            IMPLEMENTED_METHODS.contains(method),
            "{method} dropped out of IMPLEMENTED_METHODS — the daemon still \
             dispatches it, so the advertisement is now under-honest"
        );
        match probe(method).await {
            Probe::Blocked => {}
            Probe::Responded { code, body } => {
                assert!(
                    !is_fail_closed_miss(code.as_deref(), &body),
                    "{method} is advertised but has no dispatch arm: {body}"
                );
            }
        }
    }
}

/// `sampling/createMessage` and `elicitation/create` let an MCP **server** drive
/// the client: spend local inference on the server's prompt, or put the server's
/// question in front of the user. Both invert the trust direction the rest of the
/// MCP surface assumes, and neither has a consent surface here. They are out of
/// scope by decision, not by oversight, so they must fail closed as `unsupported`
/// exactly like the OAuth pair above — never `internal_error`, and never quietly
/// advertised by a future edit.
#[tokio::test(flavor = "multi_thread")]
async fn server_driven_mcp_methods_are_not_advertised() {
    isolate_env();

    for method in ["mcp.sampling.createMessage", "mcp.elicitation.create"] {
        assert!(
            !IMPLEMENTED_METHODS.contains(&method) && !HOST_IMPLEMENTED_METHODS.contains(&method),
            "{method} must not be advertised — server-driven MCP calls need a \
             permission design before they are implemented"
        );
        match probe(method).await {
            Probe::Blocked => panic!("{method} unexpectedly has a working handler"),
            Probe::Responded { code, body } => {
                assert_eq!(
                    code.as_deref(),
                    Some("unsupported"),
                    "{method} should fail closed as `unsupported`, got: {body}"
                );
            }
        }
    }
}

/// Unknown names must also fail closed as `unsupported`, never `internal_error`.
#[tokio::test(flavor = "multi_thread")]
async fn unknown_methods_fail_closed_as_unsupported() {
    isolate_env();
    match probe("totally.notAMethod").await {
        Probe::Blocked => panic!("unknown method should not reach a handler"),
        Probe::Responded { code, body } => {
            assert_eq!(code.as_deref(), Some("unsupported"), "body: {body}");
        }
    }
}
