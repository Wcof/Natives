//! MCP runtime test suite (ARCH-002). Moved verbatim from `mcp_runtime.rs`.

use super::http::map_jsonrpc_error;
use super::resources::{resource_max_bytes, truncate_on_char_boundary, uri_matches_template};
use super::stdio::{serve_server_request, spawn_stdio_reader, stdio_roundtrip};
use super::*;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn register_trusted_http_and_list() {
    let rt = McpRuntime::new();
    rt.register_server(McpServerConfig {
        id: "docs".into(),
        transport: McpTransport::Http,
        command: None,
        args: None,
        url: Some("https://mcp.example.com/v1".into()),
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    rt.upsert_tool(McpToolDescriptor {
        server_id: "docs".into(),
        name: "search".into(),
        description: "search docs".into(),
        input_schema: json!({"type":"object"}),
    })
    .unwrap();
    assert_eq!(rt.list_servers().len(), 1);
    assert_eq!(rt.namespaced_tools()[0].0, "mcp__docs__search");
}

#[test]
fn rejects_ssrf_localhost() {
    let rt = McpRuntime::new();
    let err = rt
        .register_server(McpServerConfig {
            id: "bad".into(),
            transport: McpTransport::Http,
            command: None,
            args: None,
            url: Some("http://127.0.0.1:9".into()),
            trusted: false,
            auth_token: None,
            headers: None,
        })
        .unwrap_err();
    assert!(err.to_ascii_lowercase().contains("ssrf") || err.contains("local"));
}

#[test]
fn trusted_localhost_http_allowed_at_register() {
    let rt = McpRuntime::new();
    rt.register_server(McpServerConfig {
        id: "local".into(),
        transport: McpTransport::Http,
        command: None,
        args: None,
        url: Some("http://127.0.0.1:9999/v1".into()),
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    assert_eq!(rt.list_servers().len(), 1);
}

#[test]
fn untrusted_stdio_start_denied() {
    let rt = McpRuntime::new();
    let err = rt.start_stdio("nope").unwrap_err();
    assert!(err.contains("not found"));
}

#[test]
fn ingest_tools_from_sse_data_lines() {
    let rt = McpRuntime::new();
    let body = r#"
event: tools
data: {"tools":[{"name":"sse_tool","description":"from sse","inputSchema":{"type":"object"}}]}

data: [DONE]
"#;
    let n = rt.ingest_tools_payload("sse1", body).unwrap();
    assert_eq!(n, 1);
    assert!(rt
        .namespaced_tools()
        .iter()
        .any(|(n, _, _)| n == "mcp__sse1__sse_tool"));
}

#[test]
fn call_tool_requires_registered_tool() {
    let rt = McpRuntime::new();
    rt.register_server(McpServerConfig {
        id: "s".into(),
        transport: McpTransport::Stdio,
        command: Some("true".into()),
        args: None,
        url: None,
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    let err = rt.call_tool("s", "missing", json!({})).unwrap_err();
    assert!(err.contains("not registered"));
}

#[test]
fn oauth_lease_status_hides_token() {
    let rt = McpRuntime::new();
    let lease = rt
        .set_auth_token("svc", "super-secret-token-abc".into(), "bearer", None)
        .unwrap();
    assert!(lease.has_token);
    let json = serde_json::to_string(&lease).unwrap();
    assert!(!json.contains("super-secret"));
    let status = rt.auth_status("svc").unwrap();
    assert!(status.has_token);
    rt.clear_auth_token("svc").unwrap();
    assert!(!rt.auth_status("svc").unwrap().has_token);
}

#[test]
fn resolve_auth_prefers_store_over_missing_config() {
    let rt = McpRuntime::new();
    rt.register_server(McpServerConfig {
        id: "authd".into(),
        transport: McpTransport::Http,
        command: None,
        args: None,
        url: Some("https://mcp.example.com".into()),
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    rt.set_auth_token("authd", "tok-1".into(), "bearer", None)
        .unwrap();
    let cfg = rt.server_config("authd").unwrap();
    let h = rt.resolve_auth_header(&cfg).unwrap();
    assert!(h.contains("tok-1"));
    // header must not appear in list_servers serialization to UI events — only status bool
    let status = rt.auth_status("authd").unwrap();
    let s = serde_json::to_string(&status).unwrap();
    assert!(!s.contains("tok-1"));
}

// -----------------------------------------------------------------
// Capability honesty
// -----------------------------------------------------------------

fn http_server(rt: &McpRuntime, id: &str) {
    rt.register_server(McpServerConfig {
        id: id.into(),
        transport: McpTransport::Http,
        command: None,
        args: None,
        url: Some("https://mcp.example.com".into()),
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
}

#[test]
fn capability_scoped_calls_report_unknown_before_any_handshake() {
    let rt = McpRuntime::new();
    http_server(&rt, "nohandshake");
    // The distinction the project's no-fake-data rule exists for: we must
    // not answer "no resources", because we have not asked yet.
    let err = rt.list_resources("nohandshake", None).unwrap_err();
    assert_eq!(err.kind(), "unsupported");
    assert!(
        err.message().contains("capabilities unknown"),
        "expected unknown-capabilities wording, got: {err}"
    );
    assert!(rt.server_capabilities("nohandshake").is_none());
}

#[test]
fn missing_server_is_not_found_not_unsupported() {
    let rt = McpRuntime::new();
    let err = rt.list_prompts("ghost", None).unwrap_err();
    assert_eq!(err.kind(), "not_found");
}

#[test]
fn capabilities_distinguish_unsupported_from_supported_but_empty() {
    let caps = McpServerCapabilities::from_initialize(
        "s",
        &json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "resources": { "listChanged": true } },
            "serverInfo": { "name": "x", "version": "1" }
        }),
    );
    assert!(caps.resources, "declared resources must read as supported");
    assert!(caps.resources_list_changed);
    assert!(!caps.resources_subscribe);
    // Never declared => not advertised. An empty `resources/list` from a
    // server with `resources: true` is a different fact entirely.
    assert!(!caps.prompts);
    assert!(!caps.tools);
    assert_eq!(caps.protocol_version, "2024-11-05");
    // `raw` keeps the verbatim object so nothing is lost in the projection.
    assert!(caps.raw.get("resources").is_some());
    assert!(caps.raw.get("prompts").is_none());
}

#[test]
fn declared_capability_that_server_then_rejects_reads_as_unsupported() {
    // -32601 after advertising is the server contradicting itself; it must
    // not surface as a daemon transport fault.
    let err = map_jsonrpc_error(
        "s",
        "resources/list",
        &json!({"code": -32601, "message": "Method not found"}),
    );
    assert_eq!(err.kind(), "unsupported");
    assert_eq!(
        map_jsonrpc_error(
            "s",
            "prompts/get",
            &json!({"code": -32602, "message": "bad"})
        )
        .kind(),
        "invalid"
    );
}

// -----------------------------------------------------------------
// resources/read security boundary
// -----------------------------------------------------------------

fn caps_with(rt: &McpRuntime, id: &str, caps: Value) {
    rt.capabilities.lock().unwrap().insert(
        id.to_string(),
        McpServerCapabilities::from_initialize(id, &json!({ "capabilities": caps })),
    );
}

fn seed_resources(rt: &McpRuntime, id: &str, uris: &[&str]) {
    rt.resources.lock().unwrap().insert(
        id.to_string(),
        uris.iter().map(|u| json!({ "uri": u })).collect(),
    );
}

#[test]
fn read_resource_refuses_uri_the_server_never_published() {
    let rt = McpRuntime::new();
    http_server(&rt, "res");
    caps_with(&rt, "res", json!({ "resources": {} }));
    seed_resources(&rt, "res", &["mem://note/1"]);

    let err = rt.read_resource("res", "mem://note/2").unwrap_err();
    assert_eq!(err.kind(), "denied");
    assert!(err.message().contains("not published"), "{err}");
}

#[test]
fn read_resource_allows_a_published_uri() {
    let rt = McpRuntime::new();
    http_server(&rt, "res");
    caps_with(&rt, "res", json!({ "resources": {} }));
    seed_resources(&rt, "res", &["mem://note/1"]);
    let cfg = rt.server_config("res").unwrap();
    assert_eq!(
        rt.assert_resource_uri_allowed(&cfg, "mem://note/1")
            .unwrap(),
        "listed"
    );
}

#[test]
fn file_uri_blocked_for_untrusted_server_even_when_published() {
    let rt = McpRuntime::new();
    // Untrusted remote HTTP server, allowed to register (no SSRF target).
    rt.register_server(McpServerConfig {
        id: "sketchy".into(),
        transport: McpTransport::Http,
        command: None,
        args: None,
        url: Some("https://sketchy.example.com".into()),
        trusted: false,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    caps_with(&rt, "sketchy", json!({ "resources": {} }));
    // Even publishing it does not buy the right to name a local path.
    seed_resources(&rt, "sketchy", &["file:///etc/passwd"]);

    let err = rt
        .read_resource("sketchy", "file:///etc/passwd")
        .unwrap_err();
    assert_eq!(err.kind(), "denied");
    assert!(err.message().contains("untrusted"), "{err}");
}

#[test]
fn file_uri_traversal_and_remote_authority_blocked_even_when_trusted() {
    let rt = McpRuntime::new();
    http_server(&rt, "t");
    caps_with(&rt, "t", json!({ "resources": {} }));
    seed_resources(
        &rt,
        "t",
        &[
            "file:///srv/data/../../etc/shadow",
            "file://evil.example.com/share/x",
        ],
    );
    let cfg = rt.server_config("t").unwrap();

    let traversal = rt
        .assert_resource_uri_allowed(&cfg, "file:///srv/data/../../etc/shadow")
        .unwrap_err();
    assert_eq!(traversal.kind(), "denied");
    assert!(traversal.message().contains("traversal"), "{traversal}");

    let remote = rt
        .assert_resource_uri_allowed(&cfg, "file://evil.example.com/share/x")
        .unwrap_err();
    assert_eq!(remote.kind(), "denied");
    assert!(remote.message().contains("non-local authority"), "{remote}");
}

#[test]
fn code_bearing_schemes_are_never_readable() {
    let rt = McpRuntime::new();
    http_server(&rt, "t");
    caps_with(&rt, "t", json!({ "resources": {} }));
    let cfg = rt.server_config("t").unwrap();
    for uri in [
        "javascript:alert(1)",
        "data:text/html;base64,PHNjcmlwdD4=",
        "vbscript:x",
        "blob:https://x/y",
    ] {
        // Published or not is irrelevant — the scheme check runs first.
        seed_resources(&rt, "t", &[uri]);
        let err = rt.assert_resource_uri_allowed(&cfg, uri).unwrap_err();
        assert_eq!(err.kind(), "denied", "{uri} should be denied");
        assert!(err.message().contains("never readable"), "{uri}: {err}");
    }
}

#[test]
fn empty_uri_is_a_validation_error_not_a_denial() {
    let rt = McpRuntime::new();
    http_server(&rt, "t");
    caps_with(&rt, "t", json!({ "resources": {} }));
    assert_eq!(rt.read_resource("t", "  ").unwrap_err().kind(), "invalid");
}

// -----------------------------------------------------------------
// Template matching — widening this silently widens the read allowlist
// -----------------------------------------------------------------

#[test]
fn template_matches_only_single_segment_expansions() {
    assert!(uri_matches_template(
        "db://table/users",
        "db://table/{name}"
    ));
    assert!(uri_matches_template(
        "repo://natives/file/main.rs",
        "repo://{project}/file/{path}"
    ));
    // A `/` in the expansion would let one template cover a whole subtree.
    assert!(!uri_matches_template(
        "db://table/users/secret",
        "db://table/{name}"
    ));
    // Variable must consume something.
    assert!(!uri_matches_template("db://table/", "db://table/{name}"));
    // Literal prefix mismatch.
    assert!(!uri_matches_template(
        "other://table/users",
        "db://table/{name}"
    ));
    // Trailing literal must be consumed exactly.
    assert!(uri_matches_template(
        "db://x/rows.json",
        "db://{t}/rows.json"
    ));
    assert!(!uri_matches_template(
        "db://x/rows.json.bak",
        "db://{t}/rows.json"
    ));
}

#[test]
fn template_never_matches_traversal_or_malformed_patterns() {
    assert!(!uri_matches_template(
        "file:///srv/../etc/passwd",
        "file:///srv/{name}"
    ));
    // Unclosed brace is malformed: refuse rather than guess.
    assert!(!uri_matches_template("db://x", "db://{name"));
    // Adjacent variables have no separator to anchor on.
    assert!(!uri_matches_template("db://ab", "db://{a}{b}"));
}

#[test]
fn template_published_uri_is_allowed_but_a_sibling_subtree_is_not() {
    let rt = McpRuntime::new();
    http_server(&rt, "t");
    caps_with(&rt, "t", json!({ "resources": {} }));
    rt.resource_templates.lock().unwrap().insert(
        "t".into(),
        vec![json!({"uriTemplate": "db://table/{name}"})],
    );
    let cfg = rt.server_config("t").unwrap();
    assert_eq!(
        rt.assert_resource_uri_allowed(&cfg, "db://table/users")
            .unwrap(),
        "template:db://table/{name}"
    );
    assert!(rt
        .assert_resource_uri_allowed(&cfg, "db://table/users/private")
        .is_err());
}

// -----------------------------------------------------------------
// Size cap
// -----------------------------------------------------------------

#[test]
fn truncation_respects_utf8_boundaries() {
    let text = "日本語テキスト";
    let out = truncate_on_char_boundary(text, 5);
    assert!(text.starts_with(&out));
    assert!(out.len() <= 5);
    // Would panic on a byte slice; must not.
    assert!(std::str::from_utf8(out.as_bytes()).is_ok());
}

#[test]
fn resource_byte_cap_is_clamped_to_a_sane_band() {
    // Env is process-global; assert the clamp arithmetic via the public band
    // rather than mutating env and racing other tests.
    assert!(resource_max_bytes() >= 1024);
    assert!(resource_max_bytes() <= 8 * 1024 * 1024);
}

// -----------------------------------------------------------------
// Roots
// -----------------------------------------------------------------

#[test]
fn roots_default_to_empty_rather_than_the_process_cwd() {
    let rt = McpRuntime::new();
    // No explicit grant. Absent NATIVES_MCP_ROOTS this must be empty; if the
    // env happens to be set, every entry must still be a real directory.
    for root in rt.client_roots() {
        assert!(root.uri.starts_with("file://"));
    }
    if std::env::var("NATIVES_MCP_ROOTS").is_err() {
        assert!(rt.client_roots().is_empty());
    }
}

#[test]
fn roots_must_be_existing_absolute_directories() {
    let rt = McpRuntime::new();
    assert_eq!(
        rt.set_roots(&["relative/path".into()]).unwrap_err().kind(),
        "invalid"
    );
    assert_eq!(
        rt.set_roots(&["/definitely/not/here/xyzzy".into()])
            .unwrap_err()
            .kind(),
        "invalid"
    );
    let dir = std::env::temp_dir().join(format!("mcp-root-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let granted = rt.set_roots(&[dir.to_string_lossy().to_string()]).unwrap();
    assert_eq!(granted.len(), 1);
    assert!(granted[0].uri.starts_with("file://"));
    assert_eq!(rt.client_roots().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn server_requests_we_do_not_implement_are_refused_not_ignored() {
    // A silent hang would look like a daemon bug; -32601 lets the server adapt.
    let sampling = serve_server_request("sampling/createMessage", &json!(7), &[]);
    assert_eq!(sampling["error"]["code"], -32601);
    assert_eq!(sampling["id"], json!(7));
    let elicit = serve_server_request("elicitation/create", &json!(8), &[]);
    assert_eq!(elicit["error"]["code"], -32601);

    let roots = serve_server_request("roots/list", &json!(9), &[json!({"uri":"file:///w"})]);
    assert_eq!(roots["result"]["roots"][0]["uri"], "file:///w");
    assert!(serve_server_request("ping", &json!(1), &[])["result"].is_object());
}

// -----------------------------------------------------------------
// Notifications
// -----------------------------------------------------------------

#[test]
fn notification_ring_is_bounded_and_keeps_the_newest() {
    let rt = McpRuntime::new();
    let frames: Vec<Value> = (0..NOTIFICATION_RING_CAP + 50)
        .map(|i| json!({"method": "notifications/message", "params": {"seq": i}}))
        .collect();
    rt.record_notifications("s", frames);
    let kept = rt.notifications(Some("s"));
    assert_eq!(kept.len(), NOTIFICATION_RING_CAP);
    assert_eq!(
        kept.last().unwrap().params["seq"],
        NOTIFICATION_RING_CAP + 49
    );
}

#[test]
fn resource_change_notification_invalidates_the_read_allowlist() {
    let rt = McpRuntime::new();
    http_server(&rt, "r");
    caps_with(&rt, "r", json!({ "resources": { "listChanged": true } }));
    seed_resources(&rt, "r", &["mem://a"]);
    let cfg = rt.server_config("r").unwrap();
    assert!(rt.assert_resource_uri_allowed(&cfg, "mem://a").is_ok());

    rt.record_notifications(
        "r",
        vec![json!({"method": "notifications/resources/list_changed"})],
    );

    // A retracted list must stop vouching for its URIs.
    let err = rt.assert_resource_uri_allowed(&cfg, "mem://a").unwrap_err();
    assert_eq!(err.kind(), "denied");
    assert_eq!(rt.notifications(Some("r")).len(), 1);
}

#[test]
fn notifications_are_scoped_per_server() {
    let rt = McpRuntime::new();
    rt.record_notifications(
        "a",
        vec![json!({"method": "notifications/tools/list_changed"})],
    );
    rt.record_notifications("b", vec![json!({"method": "notifications/message"})]);
    assert_eq!(rt.notifications(Some("a")).len(), 1);
    assert_eq!(
        rt.notifications(Some("a"))[0].method,
        "notifications/tools/list_changed"
    );
    assert_eq!(rt.notifications(None).len(), 2);
    assert_eq!(rt.notifications(Some("missing")).len(), 0);
}

#[test]
fn stdio_mock_echo_handshake_and_call() {
    let rt = McpRuntime::new();
    let script = r#"
import sys, json
for line in sys.stdin:
    line=line.strip()
    if not line:
        continue
    req = json.loads(line)
    method = req.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":req["id"],"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"mock","version":"0"}}}))
        sys.stdout.flush()
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        print(json.dumps({"jsonrpc":"2.0","id":req["id"],"result":{"tools":[{"name":"echo","description":"echo","inputSchema":{"type":"object"}}]}}))
        sys.stdout.flush()
    elif method == "tools/call":
        args = req.get("params",{}).get("arguments",{})
        print(json.dumps({"jsonrpc":"2.0","id":req["id"],"result":{"content":[{"type":"text","text":json.dumps(args)}]}}))
        sys.stdout.flush()
"#;
    let dir = std::env::temp_dir().join(format!("mcp-mock-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("mock_mcp.py");
    std::fs::write(&path, script).unwrap();
    rt.register_server(McpServerConfig {
        id: "mock".into(),
        transport: McpTransport::Stdio,
        command: Some("python3".into()),
        args: Some(vec![path.to_string_lossy().to_string()]),
        url: None,
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    let res = rt.start("mock");
    if res.is_err() {
        eprintln!("stdio mock skipped: {:?}", res.err());
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let v = res.unwrap();
    assert_eq!(v["tools_discovered"], 1);
    assert_eq!(v["session_live"], true);
    assert!(rt
        .namespaced_tools()
        .iter()
        .any(|(n, _, _)| n == "mcp__mock__echo"));
    let called = rt
        .call_tool("mock", "mcp__mock__echo", json!({"msg": "hi"}))
        .expect("call_tool");
    assert!(
        called.to_string().contains("hi") || called.get("content").is_some(),
        "unexpected call result: {called}"
    );
    let _ = rt.stop("mock");
    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// T04: a `stop()` while a stdio request is blocked in the transport must
// not deadlock. Before the fix the sessions mutex was held across the
// whole blocking read, so `stop()` waited for the (30s) transport timeout.
// -----------------------------------------------------------------------

const HUNG_MOCK: &str = r#"
import sys, json, time
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    method = req.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":req["id"],"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"hung","version":"0"}}}), flush=True)
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        print(json.dumps({"jsonrpc":"2.0","id":req["id"],"result":{"tools":[{"name":"hang","description":"hang forever","inputSchema":{"type":"object"}}]}}), flush=True)
    elif method == "tools/call":
        while True:
            time.sleep(3600)
"#;

/// Register + start a stdio server that never answers `tools/call`.
/// Returns the temp dir (cleaned by the caller).
fn start_hung(rt: &McpRuntime, id: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp-hung-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = dir.join("hung_mock.py");
    std::fs::write(&script, HUNG_MOCK).expect("write hung mock");
    rt.register_server(McpServerConfig {
        id: id.into(),
        transport: McpTransport::Stdio,
        command: Some("python3".into()),
        args: Some(vec![script.to_string_lossy().to_string()]),
        url: None,
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .expect("register hung server");
    let started = rt.start(id);
    if let Err(e) = started {
        let _ = std::fs::remove_dir_all(&dir);
        panic!("hung stdio mock could not start (python3 required): {e}");
    }
    dir
}

#[test]
fn stop_does_not_wait_for_inflight_stdio_read() {
    let rt = Arc::new(McpRuntime::new());
    let dir = start_hung(&rt, "hung");
    let call_rt = rt.clone();
    let caller = std::thread::spawn(move || call_rt.call_tool("hung", "hang", json!({})));
    // Let the call block on the server's never-answering tools/call.
    std::thread::sleep(Duration::from_millis(400));

    let stop_rt = rt.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        let res = stop_rt.stop("hung");
        let _ = tx.send(res);
    });
    let stop_result = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("stop must not wait on the in-flight blocking read (deadlock)");
    assert!(stop_result.is_ok(), "stop failed: {:?}", stop_result);

    // The blocked call must now return: the child was killed, so the read
    // hit EOF instead of the 30s transport timeout.
    let call_result = caller.join().expect("call thread must exit");
    assert!(call_result.is_err(), "hung call must not succeed");
    // The session must be detached (child + registry quiet).
    assert!(rt.sessions.lock().unwrap().get("hung").is_none());
    let _ = stopper.join();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn call_tool_after_stop_fails_fast() {
    let rt = McpRuntime::new();
    let dir = start_hung(&rt, "gone");
    rt.stop("gone").expect("stop");
    let t0 = std::time::Instant::now();
    let err = rt.call_tool("gone", "hang", json!({})).unwrap_err();
    assert!(err.contains("session not started"), "got: {err}");
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "call after stop must fail fast"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn stop_escalates_term_then_kill_for_stuck_stdio() {
    // A child that records SIGTERM and keeps running (ignores it) must be
    // TERM'd first and then SIGKILL'd, and the session must be reaped.
    let dir = std::env::temp_dir().join(format!("mcp-term-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("term.txt");
    let script = dir.join("term_trap.py");
    std::fs::write(
            &script,
            format!(
                r#"import signal, time, sys, json
def handler(signum, frame):
    open({marker:?}, "w").write("term")
signal.signal(signal.SIGTERM, handler)
def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n"); sys.stdout.flush()
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    req = json.loads(line)
    method = req.get("method")
    if method == "initialize":
        send({{"jsonrpc":"2.0","id":req["id"],"result":{{"protocolVersion":"2024-11-05","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"termtrap","version":"0"}}}}}})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        send({{"jsonrpc":"2.0","id":req["id"],"result":{{"tools":[{{"name":"hang","description":"hang","inputSchema":{{"type":"object"}}}}]}}}})
    elif method == "tools/call":
        while True:
            time.sleep(3600)
"#
            ),
        )
        .unwrap();
    let rt = McpRuntime::new();
    rt.register_server(McpServerConfig {
        id: "termtrap".into(),
        transport: McpTransport::Stdio,
        command: Some("python3".into()),
        args: Some(vec![script.to_string_lossy().to_string()]),
        url: None,
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    if rt.start("termtrap").is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return; // python3 unavailable in CI — skip
    }
    // Give the handler a moment to install.
    std::thread::sleep(Duration::from_millis(300));
    let t0 = std::time::Instant::now();
    rt.stop("termtrap").unwrap();
    assert!(t0.elapsed() < Duration::from_secs(5), "escalation too slow");
    assert!(
        marker.exists(),
        "SIGTERM must have been attempted before escalation to KILL"
    );
    assert!(rt.sessions.lock().unwrap().get("termtrap").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn http_flood_cancel_does_not_hang_or_grow_unbounded() {
    // A server that floods SSE lines must (a) be bounded by the bounded
    // reader channel and (b) cancel without the reader join hanging on a
    // full channel.
    let dir = std::env::temp_dir().join(format!("mcp-http-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("http_flood.py");
    let portfile = dir.join("port");
    std::fs::write(
        &script,
        r#"
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            while True:
                self.wfile.write(b"data: " + b"x" * 1000 + b"\n\n")
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass
    def log_message(self, *a):
        pass
srv = HTTPServer(("127.0.0.1", 0), H)
with open(sys.argv[1], "w") as f:
    f.write(str(srv.server_address[1]))
srv.serve_forever()
"#,
    )
    .unwrap();
    let mut srv = Command::new("python3")
        .arg(&script)
        .arg(&portfile)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("python3 required");
    let port = {
        let mut deadline = 0;
        loop {
            if portfile.exists() {
                if let Ok(raw) = std::fs::read_to_string(&portfile) {
                    if let Ok(p) = raw.trim().parse::<u16>() {
                        break p;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
            deadline += 1;
            assert!(deadline < 200, "http server never reported a port");
        }
    };

    let rt = McpRuntime::new();
    rt.register_server(McpServerConfig {
        id: "flood".into(),
        transport: McpTransport::Http,
        command: None,
        args: None,
        url: Some(format!("http://127.0.0.1:{port}/mcp")),
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .unwrap();
    rt.upsert_tool(McpToolDescriptor {
        server_id: "flood".into(),
        name: "t".into(),
        description: "flood".into(),
        input_schema: json!({"type": "object"}),
    })
    .unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancelled.clone();
    let cancel_cb: McpCancelCallback = Arc::new(move || cancel_flag.load(Ordering::SeqCst));
    let call_rt = Arc::new(rt);
    let call_rt2 = call_rt.clone();
    let caller = std::thread::spawn(move || {
        call_rt2.call_tool_with_progress_and_cancel("flood", "t", json!({}), None, Some(cancel_cb))
    });
    // Let curl/reader settle into the flood, then cancel.
    std::thread::sleep(Duration::from_millis(300));
    cancelled.store(true, Ordering::SeqCst);
    let t0 = std::time::Instant::now();
    let res = caller
        .join()
        .expect("caller thread must exit (reader join must not hang)");
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "cancel took too long: {:?}",
        t0.elapsed()
    );
    assert!(res.is_err(), "flooded call must be cancelled");

    let _ = srv.kill();
    let _ = srv.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn roundtrip_timeout_is_distinct_from_stop() {
    // A never-responding child must surface a transport timeout, not a
    // cancel, and a stopped session must surface a cancel, not a timeout —
    // the audit/ledger distinction depends on it.
    let dir = std::env::temp_dir().join(format!("mcp-timeout-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("sleep_forever.py");
    std::fs::write(&script, "import time\nwhile True: time.sleep(3600)\n").unwrap();

    let mut child = Command::new("python3")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("python3 required");
    let stdin = Arc::new(Mutex::new(Some(child.stdin.take().expect("stdin"))));
    let mut lines = spawn_stdio_reader(child.stdout.take().expect("stdout"));
    let stopping = Arc::new(AtomicBool::new(false));
    let roots: Vec<Value> = Vec::new();
    let mut notes = Vec::new();

    let timeout_err = stdio_roundtrip(
        &stdin,
        &mut lines,
        &roots,
        &mut notes,
        1,
        "tools/call",
        json!({}),
        Duration::from_millis(300),
        None,
        &stopping,
    )
    .unwrap_err();
    assert!(
        timeout_err.contains("timeout"),
        "timeout must be explicit, got: {timeout_err}"
    );
    assert!(
        !timeout_err.contains("cancelled"),
        "timeout must not masquerade as cancel: {timeout_err}"
    );

    stopping.store(true, Ordering::SeqCst);
    let stop_err = stdio_roundtrip(
        &stdin,
        &mut lines,
        &roots,
        &mut notes,
        2,
        "tools/call",
        json!({}),
        Duration::from_millis(300),
        None,
        &stopping,
    )
    .unwrap_err();
    assert!(
        stop_err.contains("cancelled"),
        "stopped session must surface cancel, got: {stop_err}"
    );

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
