//! End-to-end MCP protocol surface against a local stdio mock — no network, no
//! real MCP server, no package manager.
//!
//! These tests exist because the interesting failures are all *interleaving*
//! failures, and a unit test on a pure function cannot produce them. A real
//! server pushes `notifications/*` frames whenever it likes and asks
//! `roots/list` in the middle of a tool call. Before this file the daemon read
//! exactly one line and called it the response, which means the first server
//! that spoke out of turn would have desynchronised the session permanently and
//! every later call on that connection would have returned the previous call's
//! answer. That is the class of bug being pinned here.
//!
//! The mock speaks one JSON-RPC frame per line on stdin/stdout, exactly like a
//! real stdio server, and its behaviour is selected by argv so one script can
//! stand in for "full server", "tools only" and "advertises resources but has
//! none".

use agent_core::{McpServerConfig, McpTransport};
use natives_agent_daemon::mcp_runtime::McpRuntime;
use serde_json::json;

const MOCK: &str = r#"
import sys, json

MODE = sys.argv[1] if len(sys.argv) > 1 else "full"

CAPS = {
    "full": {
        "tools": {},
        "resources": {"subscribe": False, "listChanged": True},
        "prompts": {"listChanged": False},
    },
    "toolsonly": {"tools": {}},
    "emptyresources": {"resources": {}},
}[MODE]

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def read():
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return read()
    return json.loads(line)

while True:
    msg = read()
    if msg is None:
        break
    method = msg.get("method")
    if method is None:
        # A response to a request the server itself sent. Ignore here; the
        # tools/call handler reads its own replies inline.
        continue
    rid = msg.get("id")

    if method == "initialize":
        send({"jsonrpc": "2.0", "id": rid, "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": CAPS,
            "serverInfo": {"name": "surface-mock", "version": "1"},
        }})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": rid, "result": {"tools": [
            {"name": "ask_roots", "description": "echo client roots",
             "inputSchema": {"type": "object"}},
            {"name": "ask_sampling", "description": "probe sampling support",
             "inputSchema": {"type": "object"}},
        ]}})
    elif method == "resources/list":
        # Unsolicited notification *before* the response: the demux test.
        send({"jsonrpc": "2.0", "method": "notifications/message",
              "params": {"level": "info", "data": "listing resources"}})
        if MODE == "emptyresources":
            send({"jsonrpc": "2.0", "id": rid, "result": {"resources": []}})
        else:
            send({"jsonrpc": "2.0", "id": rid, "result": {"resources": [
                {"uri": "mem://note/1", "name": "note one", "mimeType": "text/plain"},
                {"uri": "mem://note/2", "name": "note two", "mimeType": "text/plain"},
            ]}})
    elif method == "resources/templates/list":
        send({"jsonrpc": "2.0", "id": rid, "result": {"resourceTemplates": [
            {"uriTemplate": "mem://doc/{slug}", "name": "doc by slug"},
        ]}})
    elif method == "resources/read":
        uri = msg.get("params", {}).get("uri", "")
        body = "X" * 5000 if uri == "mem://note/2" else "BODY:" + uri
        send({"jsonrpc": "2.0", "id": rid, "result": {"contents": [
            {"uri": uri, "mimeType": "text/plain", "text": body},
        ]}})
    elif method == "prompts/list":
        send({"jsonrpc": "2.0", "id": rid, "result": {"prompts": [
            {"name": "review", "description": "review code",
             "arguments": [{"name": "lang", "required": True}]},
        ]}})
    elif method == "prompts/get":
        args = msg.get("params", {}).get("arguments", {}) or {}
        send({"jsonrpc": "2.0", "id": rid, "result": {
            "description": "review",
            "messages": [{"role": "user", "content": {
                "type": "text", "text": "review in " + str(args.get("lang"))}}],
        }})
    elif method == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "ask_roots":
            send({"jsonrpc": "2.0", "id": 9001, "method": "roots/list"})
            reply = read() or {}
            roots = reply.get("result", {}).get("roots", [])
            send({"jsonrpc": "2.0", "id": rid, "result": {"content": [
                {"type": "text", "text": json.dumps(roots)}]}})
        elif name == "ask_sampling":
            send({"jsonrpc": "2.0", "id": 9002,
                  "method": "sampling/createMessage", "params": {}})
            reply = read() or {}
            code = reply.get("error", {}).get("code")
            send({"jsonrpc": "2.0", "id": rid, "result": {"content": [
                {"type": "text", "text": "code=" + str(code)}]}})
        else:
            send({"jsonrpc": "2.0", "id": rid,
                  "error": {"code": -32602, "message": "unknown tool"}})
    else:
        send({"jsonrpc": "2.0", "id": rid,
              "error": {"code": -32601, "message": "Method not found"}})
"#;

struct Mock {
    dir: std::path::PathBuf,
    script: std::path::PathBuf,
}

impl Mock {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mcp-surface-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("surface_mock.py");
        std::fs::write(&script, MOCK).expect("write mock");
        Self { dir, script }
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Register + start a mock in the given mode. Returns `None` when python3 is
/// unavailable, so CI without a python toolchain skips instead of failing.
fn start(rt: &McpRuntime, mock: &Mock, id: &str, mode: &str) -> Option<serde_json::Value> {
    rt.register_server(McpServerConfig {
        id: id.into(),
        transport: McpTransport::Stdio,
        command: Some("python3".into()),
        args: Some(vec![
            mock.script.to_string_lossy().to_string(),
            mode.to_string(),
        ]),
        url: None,
        trusted: true,
        auth_token: None,
        headers: None,
    })
    .expect("register");
    match rt.start(id) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("mcp surface test skipped ({id}): {e}");
            None
        }
    }
}

#[test]
fn handshake_captures_capabilities_verbatim() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    let Some(started) = start(&rt, &mock, "full", "full") else {
        return;
    };
    assert_eq!(started["tools_discovered"], 2);

    let caps = rt
        .server_capabilities("full")
        .expect("capabilities captured");
    assert!(caps.tools);
    assert!(caps.resources);
    assert!(caps.resources_list_changed);
    assert!(!caps.resources_subscribe);
    assert!(caps.prompts);
    // Never advertised, so never claimed.
    assert!(!caps.logging);
    assert!(!caps.completions);
    assert_eq!(caps.protocol_version, "2024-11-05");
    assert_eq!(caps.server_info["name"], "surface-mock");
    // `raw` must be the server's own object, not our projection of it.
    assert_eq!(caps.raw["resources"]["listChanged"], true);
    assert!(caps.raw.get("logging").is_none());

    let _ = rt.stop("full");
}

#[test]
fn capabilities_are_dropped_when_the_session_stops() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    assert!(rt.server_capabilities("full").is_some());
    rt.stop("full").unwrap();
    // A dead server must not keep vouching for what it could do.
    assert!(rt.server_capabilities("full").is_none());
    assert_eq!(
        rt.list_resources("full", None).unwrap_err().kind(),
        "unsupported"
    );
}

#[test]
fn resources_list_read_and_the_discovery_allowlist() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }

    // Before listing, nothing is readable — the allowlist starts empty.
    let cold = rt.read_resource("full", "mem://note/1").unwrap_err();
    assert_eq!(cold.kind(), "denied");

    let listed = rt.list_resources("full", None).expect("resources/list");
    assert_eq!(listed["resources"].as_array().unwrap().len(), 2);
    assert_eq!(listed["resources"][0]["uri"], "mem://note/1");

    let read = rt.read_resource("full", "mem://note/1").expect("read");
    assert_eq!(read["allowlist_match"], "listed");
    // Provenance is what lets a caller label this before it reaches a model.
    assert_eq!(read["untrusted"], true);
    assert_eq!(read["origin"], "mcp_resource");
    assert_eq!(read["truncated"], false);
    assert_eq!(read["contents"][0]["text"], "BODY:mem://note/1");

    // Still denied for anything the server did not publish.
    let denied = rt.read_resource("full", "mem://note/999").unwrap_err();
    assert_eq!(denied.kind(), "denied");
    assert!(denied.message().contains("not published"));

    let _ = rt.stop("full");
}

#[test]
fn oversized_resource_is_truncated_and_says_so() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    std::env::set_var("NATIVES_MCP_RESOURCE_MAX_BYTES", "2048");
    rt.list_resources("full", None).unwrap();
    // note/2 is 5000 bytes from the mock.
    let read = rt.read_resource("full", "mem://note/2").expect("read");
    std::env::remove_var("NATIVES_MCP_RESOURCE_MAX_BYTES");

    assert_eq!(read["truncated"], true, "oversized read must be flagged");
    assert_eq!(read["contents"][0]["truncated"], true);
    let text = read["contents"][0]["text"].as_str().unwrap();
    assert!(text.len() <= 2048, "text not capped: {}", text.len());

    let _ = rt.stop("full");
}

#[test]
fn resource_templates_widen_the_allowlist_only_by_one_segment() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    let tpl = rt
        .list_resource_templates("full")
        .expect("resources/templates/list");
    assert_eq!(
        tpl["resource_templates"][0]["uriTemplate"],
        "mem://doc/{slug}"
    );

    let read = rt
        .read_resource("full", "mem://doc/intro")
        .expect("template read");
    assert_eq!(read["allowlist_match"], "template:mem://doc/{slug}");

    // The template must not become a subtree pass.
    assert_eq!(
        rt.read_resource("full", "mem://doc/intro/secret")
            .unwrap_err()
            .kind(),
        "denied"
    );
    let _ = rt.stop("full");
}

#[test]
fn prompts_list_and_get_carry_provenance() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    let listed = rt.list_prompts("full", None).expect("prompts/list");
    assert_eq!(listed["prompts"][0]["name"], "review");
    assert_eq!(listed["prompts"][0]["arguments"][0]["name"], "lang");

    let got = rt
        .get_prompt("full", "review", json!({ "lang": "rust" }))
        .expect("prompts/get");
    assert_eq!(got["untrusted"], true);
    assert_eq!(got["origin"], "mcp_prompt");
    assert_eq!(got["messages"][0]["content"]["text"], "review in rust");

    assert_eq!(
        rt.get_prompt("full", "", json!({})).unwrap_err().kind(),
        "invalid"
    );
    let _ = rt.stop("full");
}

#[test]
fn a_server_that_advertises_no_prompts_reports_unsupported_not_empty() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "toolsonly", "toolsonly").is_none() {
        return;
    }
    // The whole point of reading `capabilities`: this is not an empty list.
    let err = rt.list_prompts("toolsonly", None).unwrap_err();
    assert_eq!(err.kind(), "unsupported");
    assert!(err.message().contains("does not advertise"), "{err}");
    assert_eq!(
        rt.list_resources("toolsonly", None).unwrap_err().kind(),
        "unsupported"
    );
    let _ = rt.stop("toolsonly");
}

#[test]
fn a_server_that_advertises_resources_but_has_none_reports_an_empty_list() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "empty", "emptyresources").is_none() {
        return;
    }
    // The other side of the same coin: supported-but-empty is a real answer,
    // and it is only distinguishable from unsupported because of `capabilities`.
    let caps = rt.server_capabilities("empty").expect("caps");
    assert!(caps.resources);
    assert!(!caps.tools);
    let listed = rt.list_resources("empty", None).expect("resources/list");
    assert_eq!(listed["resources"].as_array().unwrap().len(), 0);
    let _ = rt.stop("empty");
}

#[test]
fn server_initiated_roots_list_is_answered_with_the_granted_roots() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    let granted = std::env::temp_dir().join(format!("mcp-granted-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&granted).unwrap();
    rt.set_roots(&[granted.to_string_lossy().to_string()])
        .expect("set_roots");

    // The mock asks `roots/list` mid `tools/call` and echoes what it received.
    let out = rt
        .call_tool("full", "ask_roots", json!({}))
        .expect("tools/call ask_roots");
    let echoed = out["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        echoed.contains(&granted.to_string_lossy().to_string()),
        "server did not receive the granted root; got: {echoed}"
    );

    let _ = rt.stop("full");
    let _ = std::fs::remove_dir_all(&granted);
}

#[test]
fn sampling_request_from_the_server_is_refused_with_method_not_found() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    // We do not implement `sampling/createMessage`. Refusing explicitly lets the
    // server degrade; hanging would look like a daemon fault and stall the call.
    let out = rt
        .call_tool("full", "ask_sampling", json!({}))
        .expect("tools/call ask_sampling");
    assert_eq!(out["content"][0]["text"], "code=-32601");
    let _ = rt.stop("full");
}

#[test]
fn an_interleaved_notification_does_not_desynchronise_the_session() {
    let mock = Mock::new();
    let rt = McpRuntime::new();
    if start(&rt, &mock, "full", "full").is_none() {
        return;
    }
    // The mock emits `notifications/message` immediately before every
    // `resources/list` response. A reader that took the first line as the answer
    // would return the notification here and then be one frame behind forever.
    let first = rt.list_resources("full", None).expect("first list");
    assert_eq!(first["resources"][0]["uri"], "mem://note/1");
    let second = rt.list_resources("full", None).expect("second list");
    assert_eq!(second["resources"][0]["uri"], "mem://note/1");

    // Prove the next unrelated call is still aligned.
    let prompts = rt.list_prompts("full", None).expect("prompts after notes");
    assert_eq!(prompts["prompts"][0]["name"], "review");

    let notes = rt.notifications(Some("full"));
    assert!(
        notes.len() >= 2,
        "notifications should be buffered, got {}",
        notes.len()
    );
    assert!(notes.iter().all(|n| n.method == "notifications/message"));
    assert_eq!(notes[0].params["data"], "listing resources");

    let _ = rt.stop("full");
}
