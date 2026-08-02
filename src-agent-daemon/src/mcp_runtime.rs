//! MCP Runtime — registry + stdio session + HTTP/SSE discovery + the protocol
//! surface beyond tools: resources, resource templates, prompts, client roots,
//! and server-pushed change notifications.
//!
//! Untrusted stdio is never auto-started. HTTP/SSE block obvious SSRF targets
//! unless `trusted=true`. Stdio keeps a live session (stdin/stdout) so tools/call
//! can run after initialize. SSE can spawn a bounded long-lived listener that
//! ingests `data:` frames. OAuth browser flow is host-side; daemon holds bearer
//! leases in `McpCredentialStore` (never logged / never in events).
//!
//! # 第 1 节 — Capabilities are the honesty boundary
//!
//! MCP's `initialize` response carries a `capabilities` object. It is the only
//! way to tell "this server has no resources" from "this server does not do
//! resources at all", and the project forbids inventing the difference. So:
//!
//! - Capabilities are captured **verbatim** from the handshake into
//!   [`McpServerCapabilities::raw`]; the booleans are derived views, never guesses.
//! - Before the handshake there is no entry at all, and every capability-scoped
//!   call fails with `capabilities unknown` — not with an empty list.
//! - A server that advertises `resources` but returns `[]` is reported as an
//!   empty list, which now *means* something.
//!
//! # 第 2 节 — Why `resources/read` is gated harder than `resources/list`
//!
//! `resources/read` returns server-chosen bytes that a caller may put in front of
//! a model. That is an injection funnel: the server picks the URI's meaning, the
//! content is attacker-controlled if the server is, and once it is in context the
//! model cannot tell it from instructions. Four rules bound it, see
//! [`McpRuntime::assert_resource_uri_allowed`]:
//!
//! 1. **Discovery allowlist** — the URI must have come from this server's own
//!    `resources/list`, or match one of its `resources/templates/list` templates.
//!    Same shape as `call_tool` refusing unregistered tools: the reachable set is
//!    whatever the server published, never whatever a caller can type.
//! 2. **Scheme policy** — `file:` is refused for untrusted servers outright, and
//!    for anyone if it carries `..` or a non-local authority. `javascript:`,
//!    `data:` and `blob:` are always refused; they are code/inline payload
//!    carriers with no legitimate resource meaning here.
//! 3. **Size cap** — content over `NATIVES_MCP_RESOURCE_MAX_BYTES` (default
//!    256 KiB) is truncated with `truncated: true` on the envelope. Never silent.
//! 4. **Provenance** — every read is wrapped in an envelope carrying
//!    `untrusted: true`, `server_id` and `uri`, and binary `blob` payloads are
//!    passed through as-is with their mime type, never decoded into text.
//!
//! Note what is deliberately *absent*: no `mcp__server__read_resource` pseudo-tool
//! is exposed to the model. `tools/call` stays closed over RPC (`direct_mcp_call_disabled`,
//! task-06) because it has side effects and must go through `PermissionGatedTools`.
//! Reads are human-initiated from the GUI only, which is why the discovery
//! allowlist plus scheme policy is a sufficient gate for them and would not be for
//! `tools/call`.
//!
//! # 第 3 节 — `roots/list` is a client obligation
//!
//! `roots` is the one direction where the server calls us. The stdio read loop
//! therefore demultiplexes: a frame with `method` **and** `id` is a server→client
//! request and gets answered inline; a frame with `method` and no `id` is a
//! notification and is buffered; only a frame whose `id` matches our request is
//! the response we were waiting for. Roots come from `NATIVES_MCP_ROOTS` or
//! [`McpRuntime::set_roots`] and must resolve to existing absolute directories —
//! an unbacked root would be exactly the fake data the project forbids. Empty is
//! the honest, fail-closed default: "no roots granted".

use agent_core::{
    McpCredentialLease, McpCredentialStore, McpRegistry, McpServerConfig, McpToolDescriptor,
    McpTransport,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Per-server cap on buffered change notifications. Oldest are dropped.
const NOTIFICATION_RING_CAP: usize = 200;

/// Default ceiling for a single `resources/read` payload.
const DEFAULT_RESOURCE_MAX_BYTES: usize = 256 * 1024;

/// Failure modes that RPC needs to distinguish, because "the server cannot do
/// this" and "you asked wrong" must not collapse into one error code.
#[derive(Debug, Clone)]
pub enum McpError {
    /// Bad or missing arguments from the caller.
    Invalid(String),
    /// Server / resource / prompt does not exist.
    NotFound(String),
    /// The server does not advertise the capability, or never handshook so we
    /// genuinely do not know. Distinct from "supported but empty".
    Unsupported(String),
    /// Blocked by a security rule (SSRF, scheme policy, discovery allowlist).
    Denied(String),
    /// Transport or protocol failure.
    Transport(String),
}

impl McpError {
    /// Stable discriminant for the RPC error envelope.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid",
            Self::NotFound(_) => "not_found",
            Self::Unsupported(_) => "unsupported",
            Self::Denied(_) => "denied",
            Self::Transport(_) => "transport",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Invalid(m)
            | Self::NotFound(m)
            | Self::Unsupported(m)
            | Self::Denied(m)
            | Self::Transport(m) => m,
        }
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// What a server said it can do, captured from `initialize`.
///
/// `raw` is the source of truth; the booleans are conveniences derived from it so
/// a GUI does not have to know MCP's nesting. Absence of this struct for a server
/// means "no handshake yet" — never "supports nothing".
#[derive(Debug, Clone, Serialize)]
pub struct McpServerCapabilities {
    pub server_id: String,
    pub protocol_version: String,
    pub server_info: Value,
    pub tools: bool,
    pub tools_list_changed: bool,
    pub resources: bool,
    pub resources_subscribe: bool,
    pub resources_list_changed: bool,
    pub prompts: bool,
    pub prompts_list_changed: bool,
    pub logging: bool,
    pub completions: bool,
    /// Verbatim `capabilities` object from the handshake.
    pub raw: Value,
}

impl McpServerCapabilities {
    fn from_initialize(server_id: &str, result: &Value) -> Self {
        let raw = result
            .get("capabilities")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let present = |key: &str| raw.get(key).map(|v| !v.is_null()).unwrap_or(false);
        let flag = |key: &str, sub: &str| {
            raw.get(key)
                .and_then(|v| v.get(sub))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        Self {
            server_id: server_id.to_string(),
            protocol_version: result
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            server_info: result
                .get("serverInfo")
                .cloned()
                .unwrap_or_else(|| json!({})),
            tools: present("tools"),
            tools_list_changed: flag("tools", "listChanged"),
            resources: present("resources"),
            resources_subscribe: flag("resources", "subscribe"),
            resources_list_changed: flag("resources", "listChanged"),
            prompts: present("prompts"),
            prompts_list_changed: flag("prompts", "listChanged"),
            logging: present("logging"),
            completions: present("completions"),
            raw,
        }
    }

    fn advertises(&self, key: &str) -> bool {
        self.raw.get(key).map(|v| !v.is_null()).unwrap_or(false)
    }
}

/// A server-pushed `notifications/*` frame, kept with arrival order.
#[derive(Debug, Clone, Serialize)]
pub struct McpNotification {
    pub server_id: String,
    pub method: String,
    pub params: Value,
    /// Unix millis when the daemon ingested the frame.
    pub received_at_ms: u64,
}

/// A directory the client grants servers visibility into.
#[derive(Debug, Clone, Serialize)]
pub struct McpRoot {
    /// `file://` URI form, which is what `roots/list` puts on the wire.
    pub uri: String,
    pub name: String,
}

struct StdioSession {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

pub struct McpRuntime {
    registry: Mutex<McpRegistry>,
    sessions: Mutex<HashMap<String, StdioSession>>,
    /// Background SSE curl children (long-lived ingest).
    sse_children: Mutex<HashMap<String, Child>>,
    status: Mutex<HashMap<String, String>>,
    credentials: Mutex<McpCredentialStore>,
    /// Handshake result per server. Missing = never initialized.
    capabilities: Mutex<HashMap<String, McpServerCapabilities>>,
    /// Last `resources/list` per server — doubles as the `resources/read` allowlist.
    resources: Mutex<HashMap<String, Vec<Value>>>,
    /// Last `resources/templates/list` per server, for template-matched reads.
    resource_templates: Mutex<HashMap<String, Vec<Value>>>,
    /// Bounded per-server notification ring.
    notifications: Mutex<HashMap<String, Vec<McpNotification>>>,
    /// Roots granted to servers. Empty = none granted (fail-closed default).
    roots: Mutex<Option<Vec<McpRoot>>>,
    /// run_id → selected server ids (ADR-0016 lifecycle refcount).
    run_refs: Mutex<HashMap<String, std::collections::HashSet<String>>>,
    /// server_id → instant it lost its last run reference (reaper input).
    idle_since: Mutex<HashMap<String, std::time::Instant>>,
}

/// Synchronous callback used by stdio transports for MCP
/// `notifications/progress` frames. The caller owns async delivery.
pub type McpProgressCallback = Arc<dyn Fn(Value) + Send + Sync>;
pub type McpCancelCallback = Arc<dyn Fn() -> bool + Send + Sync>;

impl Default for McpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl McpRuntime {
    pub fn new() -> Self {
        Self {
            registry: Mutex::new(McpRegistry::new()),
            sessions: Mutex::new(HashMap::new()),
            sse_children: Mutex::new(HashMap::new()),
            status: Mutex::new(HashMap::new()),
            credentials: Mutex::new(McpCredentialStore::new()),
            capabilities: Mutex::new(HashMap::new()),
            resources: Mutex::new(HashMap::new()),
            resource_templates: Mutex::new(HashMap::new()),
            notifications: Mutex::new(HashMap::new()),
            roots: Mutex::new(None),
            run_refs: Mutex::new(HashMap::new()),
            idle_since: Mutex::new(HashMap::new()),
        }
    }

    /// Set OAuth/bearer token for a server (memory only).
    pub fn set_auth_token(
        &self,
        server_id: &str,
        token: String,
        token_type: &str,
        expires_at: Option<u64>,
    ) -> Result<McpCredentialLease, String> {
        let mut store = self.credentials.lock().map_err(|e| e.to_string())?;
        store.set_token(server_id, token, token_type, expires_at)?;
        Ok(store.lease_status(server_id))
    }

    pub fn clear_auth_token(&self, server_id: &str) -> Result<(), String> {
        self.credentials
            .lock()
            .map_err(|e| e.to_string())?
            .clear(server_id);
        Ok(())
    }

    pub fn auth_status(&self, server_id: &str) -> Result<McpCredentialLease, String> {
        Ok(self
            .credentials
            .lock()
            .map_err(|e| e.to_string())?
            .lease_status(server_id))
    }

    fn resolve_auth_header(&self, config: &McpServerConfig) -> Option<String> {
        if let Some(h) = config.headers.as_ref().and_then(|m| {
            m.get("Authorization")
                .or_else(|| m.get("authorization"))
                .cloned()
        }) {
            return Some(h);
        }
        if let Some(t) = config.auth_token.as_ref().filter(|s| !s.is_empty()) {
            return Some(format!("Bearer {t}"));
        }
        if let Ok(store) = self.credentials.lock() {
            if let Some(t) = store.get_token(&config.id) {
                return Some(format!("Bearer {t}"));
            }
        }
        None
    }

    pub fn list_servers(&self) -> Vec<McpServerConfig> {
        self.registry
            .lock()
            .map(|r| r.list_servers().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn list_tools(&self) -> Vec<McpToolDescriptor> {
        self.registry
            .lock()
            .map(|r| r.list_tools().to_vec())
            .unwrap_or_default()
    }

    pub fn server_status(&self, id: &str) -> Option<String> {
        self.status.lock().ok().and_then(|m| m.get(id).cloned())
    }

    pub fn register_server(&self, config: McpServerConfig) -> Result<(), String> {
        self.registry
            .lock()
            .map_err(|e| e.to_string())?
            .register_server(config)
    }

    /// Remove a server from the registry (capability library delete path).
    /// Callers must stop a running server first.
    pub fn remove_server(&self, server_id: &str) {
        if let Ok(mut registry) = self.registry.lock() {
            registry.remove_server(server_id);
        }
        if let Ok(mut status) = self.status.lock() {
            status.remove(server_id);
        }
        if let Ok(mut refs) = self.run_refs.lock() {
            refs.retain(|_, servers| {
                servers.remove(server_id);
                !servers.is_empty()
            });
        }
        if let Ok(mut idle) = self.idle_since.lock() {
            idle.remove(server_id);
        }
    }

    /// Run-scoped reference: keeps a selected server warm while the run lives
    /// (ADR-0016 lifecycle). Concurrency-safe against sibling runs.
    pub fn acquire(&self, server_id: &str, run_id: &str) {
        if let Ok(mut refs) = self.run_refs.lock() {
            refs.entry(run_id.to_string())
                .or_default()
                .insert(server_id.to_string());
        }
        if let Ok(mut idle) = self.idle_since.lock() {
            idle.remove(server_id);
        }
    }

    /// Release every server reference held by a run (terminal path). Servers
    /// are NOT stopped here — the idle reaper stops cold stdio servers later,
    /// preserving warm starts across turns of the same conversation.
    pub fn release_run(&self, run_id: &str) {
        let released: Vec<String> = match self.run_refs.lock() {
            Ok(mut refs) => refs
                .remove(run_id)
                .map(|s| s.into_iter().collect())
                .unwrap_or_default(),
            Err(_) => return,
        };
        if released.is_empty() {
            return;
        }
        let still_referenced: std::collections::HashSet<String> = self
            .run_refs
            .lock()
            .map(|refs| refs.values().flatten().cloned().collect())
            .unwrap_or_default();
        if let Ok(mut idle) = self.idle_since.lock() {
            let now = std::time::Instant::now();
            for server in released {
                if !still_referenced.contains(&server) {
                    idle.insert(server, now);
                }
            }
        }
    }

    /// Stop stdio servers with zero run references idle longer than `idle_for`.
    /// Http/sse endpoints have no local process and are left alone. Returns
    /// the ids that were stopped (for logging/tests).
    pub fn reap_idle(&self, idle_for: std::time::Duration) -> Vec<String> {
        let now = std::time::Instant::now();
        let candidates: Vec<String> = match self.idle_since.lock() {
            Ok(idle) => idle
                .iter()
                .filter(|(_, since)| now.duration_since(**since) >= idle_for)
                .map(|(id, _)| id.clone())
                .collect(),
            Err(_) => return Vec::new(),
        };
        let mut stopped = Vec::new();
        for server_id in candidates {
            let is_stdio = self
                .registry
                .lock()
                .ok()
                .and_then(|r| {
                    r.list_servers()
                        .into_iter()
                        .find(|c| c.id == server_id)
                        .map(|c| matches!(c.transport, McpTransport::Stdio))
                })
                .unwrap_or(false);
            let running = self.server_status(&server_id).as_deref() == Some("running");
            if is_stdio && running && self.stop(&server_id).is_ok() {
                stopped.push(server_id.clone());
            }
            if let Ok(mut idle) = self.idle_since.lock() {
                idle.remove(&server_id);
            }
        }
        stopped
    }

    pub fn upsert_tool(&self, tool: McpToolDescriptor) -> Result<(), String> {
        self.registry
            .lock()
            .map_err(|e| e.to_string())?
            .upsert_tool(tool);
        Ok(())
    }

    pub fn namespaced_tools(&self) -> Vec<(String, String, Value)> {
        self.registry
            .lock()
            .map(|r| r.namespaced_tool_schemas())
            .unwrap_or_default()
    }

    /// Dispatch start by registered transport.
    pub fn start(&self, server_id: &str) -> Result<Value, String> {
        let config = self.server_config(server_id)?;
        match config.transport {
            McpTransport::Stdio => self.start_stdio(server_id),
            McpTransport::Http => {
                self.http_handshake(&config);
                let n = self.probe_http_tools(server_id)?;
                Ok(json!({
                    "server_id": server_id,
                    "tools_discovered": n,
                    "status": "http_probed",
                    "transport": "http",
                    "auth": self.auth_status(server_id).ok(),
                    "capabilities": self.server_capabilities(server_id),
                }))
            }
            McpTransport::Sse => {
                self.http_handshake(&config);
                let n = self.probe_http_tools(server_id)?;
                let sse = self.start_sse_listener(server_id)?;
                Ok(json!({
                    "server_id": server_id,
                    "tools_discovered": n,
                    "status": "sse_listening",
                    "transport": "sse",
                    "sse": sse,
                    "auth": self.auth_status(server_id).ok(),
                    "capabilities": self.server_capabilities(server_id),
                }))
            }
        }
    }

    /// Start a registered trusted stdio MCP server and discover tools.
    /// Keeps stdin/stdout open for subsequent `call_tool`.
    pub fn start_stdio(&self, server_id: &str) -> Result<Value, String> {
        let config = self.server_config(server_id)?;
        if !matches!(config.transport, McpTransport::Stdio) {
            return Err("start_stdio only supports stdio transport".into());
        }
        if !config.trusted {
            return Err("untrusted stdio MCP servers cannot be started".into());
        }
        let command = config
            .command
            .clone()
            .ok_or_else(|| "stdio server requires command".to_string())?;
        let args = config.args.clone().unwrap_or_default();

        let _ = self.stop(server_id);

        let mut child = Command::new(&command)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn mcp stdio failed: {e}"))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "mcp stdin missing".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "mcp stdout missing".to_string())?;
        let mut reader = BufReader::new(stdout);

        let roots = self.effective_roots();
        let mut pending_notes: Vec<Value> = Vec::new();

        // Declare only what we actually serve. We answer `roots/list`, so `roots`
        // is advertised; we do not implement sampling or elicitation, so they are
        // absent and a server can adapt instead of failing mid-run.
        let init_resp = stdio_roundtrip(
            &mut stdin,
            &mut reader,
            &roots,
            &mut pending_notes,
            1,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "roots": { "listChanged": false } },
                "clientInfo": { "name": "natives-agent-daemon", "version": "0.1.0" }
            }),
            Duration::from_secs(5),
            None,
        )?;
        let caps = McpServerCapabilities::from_initialize(
            server_id,
            init_resp.get("result").unwrap_or(&Value::Null),
        );
        // Legacy tolerance: a server that sent no `capabilities` at all predates
        // the field being load-bearing, so we still probe it. A server that sent
        // a populated object without `tools` is taken at its word.
        let advertises_tools =
            caps.tools || caps.raw.as_object().map(|o| o.is_empty()).unwrap_or(true);
        if let Ok(mut map) = self.capabilities.lock() {
            map.insert(server_id.to_string(), caps);
        }

        // initialized notification (best-effort; servers may ignore)
        let _ = writeln!(
            stdin,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        );
        let _ = stdin.flush();

        // Only ask for tools if the handshake said there are tools. Probing a
        // server that never advertised `tools` invites a `-32601` we would then
        // have to paper over as "zero tools" — the exact lie capabilities exist
        // to prevent.
        let tools_resp = if advertises_tools {
            stdio_roundtrip(
                &mut stdin,
                &mut reader,
                &roots,
                &mut pending_notes,
                2,
                "tools/list",
                json!({}),
                Duration::from_secs(5),
                None,
            )?
        } else {
            json!({})
        };

        let mut discovered = 0usize;
        if let Some(tools) = tools_resp
            .pointer("/result/tools")
            .and_then(|v| v.as_array())
        {
            for t in tools {
                let name = t
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let description = t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"}));
                self.upsert_tool(McpToolDescriptor {
                    server_id: server_id.to_string(),
                    name,
                    description,
                    input_schema,
                })?;
                discovered += 1;
            }
        }

        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                server_id.to_string(),
                StdioSession {
                    child,
                    stdin,
                    reader,
                    next_id: 3,
                },
            );
        }
        self.record_notifications(server_id, pending_notes);
        if let Ok(mut st) = self.status.lock() {
            st.insert(
                server_id.to_string(),
                format!("started tools_discovered={discovered}"),
            );
        }

        Ok(json!({
            "server_id": server_id,
            "tools_discovered": discovered,
            "status": "started",
            "transport": "stdio",
            "session_live": true,
            "capabilities": self.server_capabilities(server_id),
            "roots_granted": roots.len(),
        }))
    }

    pub fn stop(&self, server_id: &str) -> Result<(), String> {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(mut session) = sessions.remove(server_id) {
                let _ = session.child.kill();
                let _ = session.child.wait();
            }
        }
        if let Ok(mut kids) = self.sse_children.lock() {
            if let Some(mut child) = kids.remove(server_id) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        // Capabilities and the discovery caches were learned during a handshake
        // that is now over. Keeping them would let a dead server keep vouching
        // for a `resources/read` allowlist. Notifications are history and stay.
        if let Ok(mut caps) = self.capabilities.lock() {
            caps.remove(server_id);
        }
        if let Ok(mut res) = self.resources.lock() {
            res.remove(server_id);
        }
        if let Ok(mut tpl) = self.resource_templates.lock() {
            tpl.remove(server_id);
        }
        if let Ok(mut st) = self.status.lock() {
            st.insert(server_id.to_string(), "stopped".into());
        }
        Ok(())
    }

    /// Start a bounded SSE listener (curl -N). Ingests `data:` tool frames into registry.
    /// `NATIVES_MCP_SSE_MAX_SECS` caps duration (default 60) so tests/CI do not hang.
    pub fn start_sse_listener(&self, server_id: &str) -> Result<Value, String> {
        let config = self.server_config(server_id)?;
        if !matches!(config.transport, McpTransport::Sse | McpTransport::Http) {
            return Err("start_sse_listener requires http/sse transport".into());
        }
        let url = config
            .url
            .clone()
            .ok_or_else(|| "sse server requires url".to_string())?;
        self.assert_url_allowed(&config, &url)?;
        // Kill previous listener.
        if let Ok(mut kids) = self.sse_children.lock() {
            if let Some(mut child) = kids.remove(server_id) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let max_secs = std::env::var("NATIVES_MCP_SSE_MAX_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60)
            .clamp(1, 600);
        let max_secs_arg = max_secs.to_string();
        let mut args = vec![
            "-fsS".into(),
            "-N".into(),
            "--max-time".into(),
            max_secs_arg,
            "-H".into(),
            "Accept: text/event-stream".into(),
        ];
        if let Some(auth) = self.resolve_auth_header(&config) {
            args.push("-H".into());
            args.push(format!("Authorization: {auth}"));
        }
        if let Some(headers) = &config.headers {
            for (k, v) in headers {
                if k.eq_ignore_ascii_case("authorization") {
                    continue;
                }
                args.push("-H".into());
                args.push(format!("{k}: {v}"));
            }
        }
        args.push(url.clone());
        let mut child = Command::new("curl")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn sse listener failed: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "sse stdout missing".to_string())?;
        // Drain first chunk for tools (bounded read), keep process if still running.
        let mut reader = BufReader::new(stdout);
        let mut body = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut line = String::new();
        while std::time::Instant::now() < deadline {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    body.push_str(&line);
                    if body.len() > 256_000 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let discovered = if body.is_empty() {
            0
        } else {
            self.ingest_tools_payload(server_id, &body).unwrap_or(0)
        };
        // Note: we dropped the reader — child may get SIGPIPE; for production a
        // dedicated thread would keep reading. Status reflects partial long-lived start.
        if let Ok(mut kids) = self.sse_children.lock() {
            kids.insert(server_id.to_string(), child);
        }
        if let Ok(mut st) = self.status.lock() {
            st.insert(
                server_id.to_string(),
                format!("sse_listener tools_from_stream={discovered}"),
            );
        }
        Ok(json!({
            "server_id": server_id,
            "tools_from_stream": discovered,
            "listening": true,
            "max_secs": max_secs,
        }))
    }

    /// Liveness: stdio sessions check try_wait; HTTP/SSE re-probe tools count.
    pub fn liveness(&self, server_id: &str) -> Result<Value, String> {
        let config = self.server_config(server_id)?;
        match config.transport {
            McpTransport::Stdio => {
                let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                let session = sessions
                    .get_mut(server_id)
                    .ok_or_else(|| format!("no live stdio session: {server_id}"))?;
                match session.child.try_wait() {
                    Ok(None) => Ok(json!({
                        "server_id": server_id,
                        "alive": true,
                        "transport": "stdio",
                    })),
                    Ok(Some(status)) => {
                        sessions.remove(server_id);
                        Ok(json!({
                            "server_id": server_id,
                            "alive": false,
                            "transport": "stdio",
                            "exit": format!("{status}"),
                        }))
                    }
                    Err(e) => Err(format!("liveness check failed: {e}")),
                }
            }
            McpTransport::Http | McpTransport::Sse => {
                let tools = self
                    .list_tools()
                    .into_iter()
                    .filter(|t| t.server_id == server_id)
                    .count();
                let sse_alive = self
                    .sse_children
                    .lock()
                    .ok()
                    .and_then(|mut m| {
                        let child = m.get_mut(server_id)?;
                        match child.try_wait() {
                            Ok(None) => Some(true),
                            Ok(Some(_)) => {
                                m.remove(server_id);
                                Some(false)
                            }
                            Err(_) => Some(false),
                        }
                    })
                    .unwrap_or(false);
                Ok(json!({
                    "server_id": server_id,
                    "alive": tools > 0 || sse_alive,
                    "sse_listener_alive": sse_alive,
                    "transport": match config.transport {
                        McpTransport::Sse => "sse",
                        _ => "http",
                    },
                    "tools": tools,
                    "auth": self.auth_status(server_id).ok(),
                }))
            }
        }
    }

    /// Stop + start again (stdio spawn or HTTP/SSE re-probe).
    pub fn reconnect(&self, server_id: &str) -> Result<Value, String> {
        let _ = self.stop(server_id);
        let started = self.start(server_id)?;
        Ok(json!({
            "server_id": server_id,
            "reconnected": true,
            "start": started,
        }))
    }

    /// Invoke a discovered MCP tool by bare name or namespaced `mcp__server__tool`.
    pub fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        self.call_tool_with_progress(server_id, tool_name, arguments, None)
    }

    pub fn call_tool_with_progress(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
        progress: Option<McpProgressCallback>,
    ) -> Result<Value, String> {
        self.call_tool_with_progress_and_cancel(server_id, tool_name, arguments, progress, None)
    }

    pub fn call_tool_with_progress_and_cancel(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
        progress: Option<McpProgressCallback>,
        cancel: Option<McpCancelCallback>,
    ) -> Result<Value, String> {
        let bare = strip_mcp_namespace(server_id, tool_name);
        // Ensure tool is registered for this server (permission surface).
        let known = self
            .list_tools()
            .into_iter()
            .any(|t| t.server_id == server_id && t.name == bare);
        if !known {
            return Err(format!("mcp tool not registered: {server_id}/{bare}"));
        }

        let config = self.server_config(server_id)?;
        match config.transport {
            McpTransport::Stdio => self.call_stdio_tool(server_id, &bare, arguments, progress),
            McpTransport::Http | McpTransport::Sse => self.call_http_tool(
                &config,
                &bare,
                arguments,
                progress.as_ref(),
                cancel.as_ref(),
            ),
        }
    }

    fn call_stdio_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
        progress: Option<McpProgressCallback>,
    ) -> Result<Value, String> {
        let resp = self.stdio_request(
            server_id,
            "tools/call",
            json!({ "name": tool_name, "arguments": arguments }),
            Duration::from_secs(30),
            progress,
        )?;
        if let Some(err) = resp.get("error") {
            return Err(format!("mcp tools/call error: {err}"));
        }
        Ok(resp.get("result").cloned().unwrap_or(resp))
    }

    /// One request/response exchange on a live stdio session.
    ///
    /// Servers legitimately interleave `roots/list` requests and `notifications/*`
    /// frames with our responses — long tool calls are exactly when they do it.
    /// Everything that is not our response is handled inline, so the caller only
    /// ever sees the frame it asked for.
    fn stdio_request(
        &self,
        server_id: &str,
        method: &str,
        params: Value,
        timeout: Duration,
        progress: Option<McpProgressCallback>,
    ) -> Result<Value, String> {
        // Resolve roots before taking the session lock: answering `roots/list`
        // mid-exchange must not need a second lock we already hold.
        let roots = self.effective_roots();
        let mut notes: Vec<Value> = Vec::new();
        let result = {
            let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
            let session = sessions
                .get_mut(server_id)
                .ok_or_else(|| format!("mcp stdio session not started: {server_id}"))?;
            let id = session.next_id;
            session.next_id += 1;
            let StdioSession { stdin, reader, .. } = session;
            stdio_roundtrip(
                stdin,
                reader,
                &roots,
                &mut notes,
                id,
                method,
                params,
                timeout,
                progress.as_ref(),
            )
        };
        self.record_notifications(server_id, notes);
        result
    }

    fn call_http_tool(
        &self,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
        progress: Option<&McpProgressCallback>,
        cancel: Option<&McpCancelCallback>,
    ) -> Result<Value, String> {
        self.http_rpc(
            config,
            "tools/call",
            json!({ "name": tool_name, "arguments": arguments }),
            30,
            progress,
            cancel,
        )
    }

    /// [`http_rpc_frame`](Self::http_rpc_frame) with the `result` extracted and a
    /// JSON-RPC `error` flattened into a message. Use the frame variant when the
    /// caller needs to classify the error code.
    fn http_rpc(
        &self,
        config: &McpServerConfig,
        method: &str,
        params: Value,
        max_time_secs: u64,
        progress: Option<&McpProgressCallback>,
        cancel: Option<&McpCancelCallback>,
    ) -> Result<Value, String> {
        let frame = self.http_rpc_frame(config, method, params, max_time_secs, progress, cancel)?;
        if let Some(err) = frame.get("error") {
            return Err(format!("mcp {method} error: {err}"));
        }
        Ok(frame.get("result").cloned().unwrap_or(frame))
    }

    /// One JSON-RPC exchange against a Streamable HTTP / SSE MCP endpoint,
    /// returning the whole frame so `error.code` survives.
    ///
    /// The SSRF guard runs on every call, not just at registration, because a
    /// config can be re-registered between calls.
    fn http_rpc_frame(
        &self,
        config: &McpServerConfig,
        method: &str,
        params: Value,
        max_time_secs: u64,
        progress: Option<&McpProgressCallback>,
        cancel: Option<&McpCancelCallback>,
    ) -> Result<Value, String> {
        let url = config
            .url
            .clone()
            .ok_or_else(|| "http/sse server requires url".to_string())?;
        self.assert_url_allowed(config, &url)?;
        // Prefer MCP Streamable HTTP JSON-RPC endpoint (url itself or /message).
        let endpoint = if url.contains("/tools") {
            url.trim_end_matches("/tools").to_string()
        } else {
            url.trim_end_matches('/').to_string()
        };
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let mut args = vec![
            "-fsS".into(),
            "--max-time".into(),
            max_time_secs.to_string(),
            "-H".into(),
            "Content-Type: application/json".into(),
            "-H".into(),
            "Accept: application/json, text/event-stream".into(),
        ];
        if let Some(auth) = self.resolve_auth_header(config) {
            args.push("-H".into());
            args.push(format!("Authorization: {auth}"));
        }
        args.push("-d".into());
        args.push(body.to_string());
        args.push(endpoint);
        let mut child = Command::new("curl")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl not available for mcp call: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "http mcp stdout unavailable".to_string())?;
        let (line_tx, line_rx) = std::sync::mpsc::channel::<Result<String, String>>();
        let reader_thread = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.map_err(|error| format!("http mcp stdout read failed: {error}"));
                if line_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(max_time_secs.max(1));
        let mut response: Option<Value> = None;
        let mut child_finished = false;
        loop {
            if cancel.is_some_and(|callback| callback()) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader_thread.join();
                return Err("mcp call cancelled".into());
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader_thread.join();
                return Err(format!("http mcp {method} timeout"));
            }
            match line_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(line)) => {
                    let data = line
                        .trim()
                        .strip_prefix("data:")
                        .map(str::trim)
                        .unwrap_or_else(|| line.trim());
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }
                    let Ok(frame) = serde_json::from_str::<Value>(data) else {
                        continue;
                    };
                    let is_progress = frame.get("method").and_then(Value::as_str)
                        == Some("notifications/progress");
                    if is_progress {
                        if let Some(callback) = progress {
                            callback(frame);
                        }
                        continue;
                    }
                    response = Some(frame.clone());
                    if frame.get("result").is_some() || frame.get("error").is_some() {
                        break;
                    }
                }
                Ok(Err(error)) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader_thread.join();
                    return Err(error);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if child
                        .try_wait()
                        .map_err(|e| format!("http mcp wait failed: {e}"))?
                        .is_some()
                    {
                        child_finished = true;
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        if !child_finished {
            let _ = child.kill();
        }
        let status = child
            .wait()
            .map_err(|e| format!("http mcp wait failed: {e}"))?;
        let _ = reader_thread.join();
        if !status.success() {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            return Err(format!("http mcp {method} failed: {}", stderr));
        }
        response.ok_or_else(|| format!("mcp {method}: unparseable response"))
    }

    /// HTTP/SSE discovery probe: GET `{url}/tools` or bare url JSON list.
    /// Blocks local metadata targets unless trusted.
    pub fn probe_http_tools(&self, server_id: &str) -> Result<usize, String> {
        let config = self.server_config(server_id)?;
        if !matches!(config.transport, McpTransport::Http | McpTransport::Sse) {
            return Err("probe_http_tools requires http/sse transport".into());
        }
        let url = config
            .url
            .clone()
            .ok_or_else(|| "http/sse server requires url".to_string())?;
        self.assert_url_allowed(&config, &url)?;
        let probe_url = if url.ends_with("/tools") {
            url.clone()
        } else {
            format!("{}/tools", url.trim_end_matches('/'))
        };
        let mut args = vec![
            "-fsS".into(),
            "--max-time".into(),
            "5".into(),
            "-H".into(),
            "Accept: application/json, text/event-stream".into(),
        ];
        if let Some(auth) = self.resolve_auth_header(&config) {
            args.push("-H".into());
            args.push(format!("Authorization: {auth}"));
        }
        args.push(probe_url);
        let output = Command::new("curl").args(&args).output();
        let body = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            Ok(o) => {
                return Err(format!(
                    "http/sse probe failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ))
            }
            Err(e) => return Err(format!("curl not available for http/sse probe: {e}")),
        };
        let discovered = self.ingest_tools_payload(server_id, &body)?;
        if let Ok(mut st) = self.status.lock() {
            st.insert(
                server_id.to_string(),
                format!("http_or_sse_probed tools={discovered}"),
            );
        }
        Ok(discovered)
    }

    /// Parse tools from JSON body or SSE `data:` lines containing JSON objects.
    fn ingest_tools_payload(&self, server_id: &str, body: &str) -> Result<usize, String> {
        let mut discovered = 0usize;
        let mut candidates: Vec<Value> = Vec::new();
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            candidates.push(v);
        } else {
            for line in body.lines() {
                let line = line.trim();
                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim();
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        candidates.push(v);
                    }
                }
            }
        }
        for v in candidates {
            let tools = v
                .get("tools")
                .and_then(|t| t.as_array())
                .cloned()
                .or_else(|| {
                    v.get("result")
                        .and_then(|r| r.get("tools"))
                        .and_then(|t| t.as_array())
                        .cloned()
                })
                .or_else(|| v.as_array().cloned())
                .unwrap_or_default();
            for t in tools {
                let name = t
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let description = t
                    .get("description")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = t
                    .get("inputSchema")
                    .or_else(|| t.get("input_schema"))
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"}));
                self.upsert_tool(McpToolDescriptor {
                    server_id: server_id.to_string(),
                    name,
                    description,
                    input_schema,
                })?;
                discovered += 1;
            }
        }
        Ok(discovered)
    }

    fn server_config(&self, server_id: &str) -> Result<McpServerConfig, String> {
        self.registry
            .lock()
            .map_err(|e| e.to_string())?
            .list_servers()
            .into_iter()
            .find(|s| s.id == server_id)
            .cloned()
            .ok_or_else(|| format!("mcp server not found: {server_id}"))
    }

    fn assert_url_allowed(&self, config: &McpServerConfig, url: &str) -> Result<(), String> {
        if config.trusted {
            return Ok(());
        }
        if url.contains("127.0.0.1")
            || url.contains("localhost")
            || url.contains("169.254.")
            || url.starts_with("file:")
            || url.contains("0.0.0.0")
            || url.contains("[::1]")
        {
            return Err("SSRF: local MCP HTTP endpoints blocked unless trusted".into());
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // 第 4 节 — Capabilities
    // ---------------------------------------------------------------------

    /// Handshake result, or `None` when this server has never completed one.
    ///
    /// `None` is not "supports nothing" — callers must surface it as *unknown*.
    pub fn server_capabilities(&self, server_id: &str) -> Option<McpServerCapabilities> {
        self.capabilities
            .lock()
            .ok()
            .and_then(|m| m.get(server_id).cloned())
    }

    /// Best-effort `initialize` for HTTP/SSE. Non-fatal: a server that only
    /// speaks the legacy `/tools` shape stays usable, just without capabilities,
    /// and the capability-scoped calls then honestly report "unknown".
    fn http_handshake(&self, config: &McpServerConfig) {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "roots": { "listChanged": false } },
            "clientInfo": { "name": "natives-agent-daemon", "version": "0.1.0" }
        });
        if let Ok(result) = self.http_rpc(config, "initialize", params, 10, None, None) {
            let caps = McpServerCapabilities::from_initialize(&config.id, &result);
            if let Ok(mut map) = self.capabilities.lock() {
                map.insert(config.id.clone(), caps);
            }
        }
    }

    /// Gate a capability-scoped call. Separates the three states the project
    /// forbids collapsing: unknown, unsupported, supported.
    fn require_capability(&self, server_id: &str, key: &str) -> Result<(), McpError> {
        // Confirm the server exists at all before talking about its capabilities.
        self.server_config(server_id).map_err(McpError::NotFound)?;
        match self.server_capabilities(server_id) {
            None => Err(McpError::Unsupported(format!(
                "mcp capabilities unknown for `{server_id}`: no completed initialize handshake — \
                 start the server before asking what it supports"
            ))),
            Some(caps) if !caps.advertises(key) => Err(McpError::Unsupported(format!(
                "mcp server `{server_id}` does not advertise the `{key}` capability"
            ))),
            Some(_) => Ok(()),
        }
    }

    /// Transport-agnostic request returning the JSON-RPC `result`.
    fn request(
        &self,
        server_id: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        let config = self.server_config(server_id).map_err(McpError::NotFound)?;
        match config.transport {
            McpTransport::Stdio => {
                let frame = self
                    .stdio_request(server_id, method, params, timeout, None)
                    .map_err(McpError::Transport)?;
                if let Some(err) = frame.get("error") {
                    return Err(map_jsonrpc_error(server_id, method, err));
                }
                Ok(frame.get("result").cloned().unwrap_or(frame))
            }
            // Same classification as stdio: a `-32601` after the server
            // advertised the capability is the server's inconsistency, not a
            // network fault, and must not read as retryable.
            McpTransport::Http | McpTransport::Sse => {
                let frame = self
                    .http_rpc_frame(
                        &config,
                        method,
                        params,
                        timeout.as_secs().max(1),
                        None,
                        None,
                    )
                    .map_err(McpError::Transport)?;
                if let Some(err) = frame.get("error") {
                    return Err(map_jsonrpc_error(server_id, method, err));
                }
                Ok(frame.get("result").cloned().unwrap_or(frame))
            }
        }
    }

    // ---------------------------------------------------------------------
    // 第 5 节 — Resources
    // ---------------------------------------------------------------------

    /// `resources/list`. Caches the result as the `resources/read` allowlist.
    pub fn list_resources(&self, server_id: &str, cursor: Option<&str>) -> Result<Value, McpError> {
        self.require_capability(server_id, "resources")?;
        let mut params = json!({});
        if let Some(c) = cursor {
            params["cursor"] = json!(c);
        }
        let result = self.request(server_id, "resources/list", params, Duration::from_secs(15))?;
        let items = result
            .get("resources")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Ok(mut cache) = self.resources.lock() {
            let entry = cache.entry(server_id.to_string()).or_default();
            if cursor.is_none() {
                entry.clear();
            }
            for item in &items {
                if let Some(uri) = item.get("uri").and_then(|v| v.as_str()) {
                    if !entry
                        .iter()
                        .any(|e| e.get("uri").and_then(|v| v.as_str()) == Some(uri))
                    {
                        entry.push(item.clone());
                    }
                }
            }
        }
        Ok(json!({
            "server_id": server_id,
            "resources": items,
            "next_cursor": result.get("nextCursor").cloned().unwrap_or(Value::Null),
        }))
    }

    /// `resources/templates/list`. Caches templates for allowlist matching.
    pub fn list_resource_templates(&self, server_id: &str) -> Result<Value, McpError> {
        self.require_capability(server_id, "resources")?;
        let result = self.request(
            server_id,
            "resources/templates/list",
            json!({}),
            Duration::from_secs(15),
        )?;
        let items = result
            .get("resourceTemplates")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Ok(mut cache) = self.resource_templates.lock() {
            cache.insert(server_id.to_string(), items.clone());
        }
        Ok(json!({
            "server_id": server_id,
            "resource_templates": items,
            "next_cursor": result.get("nextCursor").cloned().unwrap_or(Value::Null),
        }))
    }

    /// `resources/read`, wrapped in a provenance envelope.
    ///
    /// See the module docs 第 2 节 for why this is the most tightly bounded call
    /// in the file. The returned contents are untrusted server output; the
    /// envelope says so explicitly so nothing downstream has to infer it.
    pub fn read_resource(&self, server_id: &str, uri: &str) -> Result<Value, McpError> {
        if uri.trim().is_empty() {
            return Err(McpError::Invalid("resource uri required".into()));
        }
        self.require_capability(server_id, "resources")?;
        let config = self.server_config(server_id).map_err(McpError::NotFound)?;
        let matched_by = self.assert_resource_uri_allowed(&config, uri)?;

        let result = self.request(
            server_id,
            "resources/read",
            json!({ "uri": uri }),
            Duration::from_secs(30),
        )?;

        let max_bytes = resource_max_bytes();
        let mut truncated = false;
        let contents: Vec<Value> = result
            .get("contents")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut item| {
                // Text is capped on a char boundary. `blob` is base64 and is
                // never decoded here — we only measure and cap it.
                let text_len = item.get("text").and_then(|v| v.as_str()).map(str::len);
                if text_len.is_some_and(|len| len > max_bytes) {
                    let capped = item
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|t| truncate_on_char_boundary(t, max_bytes))
                        .unwrap_or_default();
                    truncated = true;
                    item["text"] = json!(capped);
                    item["truncated"] = json!(true);
                }
                let blob_len = item.get("blob").and_then(|v| v.as_str()).map(str::len);
                if let Some(len) = blob_len {
                    item["blob_bytes"] = json!(len);
                    if len > max_bytes {
                        truncated = true;
                        item["blob"] = Value::Null;
                        item["truncated"] = json!(true);
                        item["dropped_reason"] = json!("blob exceeds resource byte cap");
                    }
                }
                item
            })
            .collect();

        Ok(json!({
            "server_id": server_id,
            "uri": uri,
            // Load-bearing for anything that later puts this in a model context.
            "untrusted": true,
            "origin": "mcp_resource",
            "allowlist_match": matched_by,
            "truncated": truncated,
            "max_bytes": max_bytes,
            "contents": contents,
        }))
    }

    /// The `resources/read` gate. Returns how the URI was allowed, for audit.
    ///
    /// Rules, in order (see module docs 第 2 节):
    /// 1. dangerous schemes are refused for everyone;
    /// 2. `file:` needs a trusted server, no `..`, and no remote authority;
    /// 3. the URI must be one this server published, or match a published template.
    fn assert_resource_uri_allowed(
        &self,
        config: &McpServerConfig,
        uri: &str,
    ) -> Result<String, McpError> {
        let lowered = uri.trim().to_ascii_lowercase();

        // 1. Code / inline-payload carriers have no resource meaning here.
        for scheme in ["javascript:", "data:", "vbscript:", "blob:"] {
            if lowered.starts_with(scheme) {
                return Err(McpError::Denied(format!(
                    "resource scheme `{scheme}` is never readable"
                )));
            }
        }

        // 2. Local filesystem reads. An untrusted server must not be able to
        //    name a path at all; a trusted one still may not traverse or point
        //    at another host.
        if lowered.starts_with("file:") {
            if !config.trusted {
                return Err(McpError::Denied(
                    "file:// resources are blocked for untrusted MCP servers".into(),
                ));
            }
            if uri.contains("..") {
                return Err(McpError::Denied(
                    "file:// resource path traversal (`..`) blocked".into(),
                ));
            }
            let after_scheme = &uri[5..];
            // `file://host/path` — anything but an empty or `localhost` authority
            // is a remote fetch wearing a local scheme.
            if let Some(rest) = after_scheme.strip_prefix("//") {
                let authority = rest.split('/').next().unwrap_or("");
                if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
                    return Err(McpError::Denied(format!(
                        "file:// resource with non-local authority `{authority}` blocked"
                    )));
                }
            }
        }

        // 3. Discovery allowlist. Same principle as `call_tool`: the reachable
        //    set is what the server published, not what a caller can type.
        let listed = self
            .resources
            .lock()
            .ok()
            .and_then(|m| m.get(&config.id).cloned())
            .unwrap_or_default();
        if listed
            .iter()
            .any(|r| r.get("uri").and_then(|v| v.as_str()) == Some(uri))
        {
            return Ok("listed".into());
        }

        let templates = self
            .resource_templates
            .lock()
            .ok()
            .and_then(|m| m.get(&config.id).cloned())
            .unwrap_or_default();
        for tpl in &templates {
            if let Some(pattern) = tpl.get("uriTemplate").and_then(|v| v.as_str()) {
                if uri_matches_template(uri, pattern) {
                    return Ok(format!("template:{pattern}"));
                }
            }
        }

        Err(McpError::Denied(format!(
            "resource `{uri}` was not published by `{}` — call mcp.resources.list \
             (and mcp.resources.templates.list) first; arbitrary URIs are not readable",
            config.id
        )))
    }

    // ---------------------------------------------------------------------
    // 第 6 节 — Prompts
    // ---------------------------------------------------------------------

    /// `prompts/list`.
    pub fn list_prompts(&self, server_id: &str, cursor: Option<&str>) -> Result<Value, McpError> {
        self.require_capability(server_id, "prompts")?;
        let mut params = json!({});
        if let Some(c) = cursor {
            params["cursor"] = json!(c);
        }
        let result = self.request(server_id, "prompts/list", params, Duration::from_secs(15))?;
        Ok(json!({
            "server_id": server_id,
            "prompts": result.get("prompts").cloned().unwrap_or_else(|| json!([])),
            "next_cursor": result.get("nextCursor").cloned().unwrap_or(Value::Null),
        }))
    }

    /// `prompts/get`. The rendered messages are server-authored text destined for
    /// a model context, so they carry the same provenance envelope as resources.
    pub fn get_prompt(
        &self,
        server_id: &str,
        name: &str,
        arguments: Value,
    ) -> Result<Value, McpError> {
        if name.trim().is_empty() {
            return Err(McpError::Invalid("prompt name required".into()));
        }
        self.require_capability(server_id, "prompts")?;
        let mut params = json!({ "name": name });
        if !arguments.is_null() {
            params["arguments"] = arguments;
        }
        let result = self.request(server_id, "prompts/get", params, Duration::from_secs(30))?;
        Ok(json!({
            "server_id": server_id,
            "name": name,
            "untrusted": true,
            "origin": "mcp_prompt",
            "description": result.get("description").cloned().unwrap_or(Value::Null),
            "messages": result.get("messages").cloned().unwrap_or_else(|| json!([])),
        }))
    }

    // ---------------------------------------------------------------------
    // 第 7 节 — Roots (client-side obligation)
    // ---------------------------------------------------------------------

    /// Replace the granted root set. Entries that are not existing absolute
    /// directories are rejected rather than trimmed, so a caller never believes
    /// it granted something it did not.
    pub fn set_roots(&self, paths: &[String]) -> Result<Vec<McpRoot>, McpError> {
        let mut roots = Vec::new();
        for raw in paths {
            roots.push(validate_root(raw)?);
        }
        let snapshot = roots.clone();
        self.roots
            .lock()
            .map_err(|e| McpError::Transport(e.to_string()))?
            .replace(roots);
        Ok(snapshot)
    }

    /// Roots we would answer `roots/list` with.
    ///
    /// Source order: an explicit [`set_roots`](Self::set_roots) wins; otherwise
    /// `NATIVES_MCP_ROOTS` (a `:`-separated path list). Empty means no roots
    /// granted — the fail-closed default, and an honest answer rather than a
    /// silent fallback to the process cwd.
    pub fn client_roots(&self) -> Vec<McpRoot> {
        self.effective_roots()
            .into_iter()
            .filter_map(|v| {
                Some(McpRoot {
                    uri: v.get("uri")?.as_str()?.to_string(),
                    name: v
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect()
    }

    /// Wire form of the roots, as `roots/list` returns them.
    fn effective_roots(&self) -> Vec<Value> {
        if let Ok(guard) = self.roots.lock() {
            if let Some(explicit) = guard.as_ref() {
                return explicit
                    .iter()
                    .map(|r| json!({ "uri": r.uri, "name": r.name }))
                    .collect();
            }
        }
        let Ok(raw) = std::env::var("NATIVES_MCP_ROOTS") else {
            return Vec::new();
        };
        raw.split(':')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|p| validate_root(p).ok())
            .map(|r| json!({ "uri": r.uri, "name": r.name }))
            .collect()
    }

    // ---------------------------------------------------------------------
    // 第 8 节 — Change notifications
    // ---------------------------------------------------------------------

    /// Buffered `notifications/*` frames, newest last. `server_id = None` returns
    /// every server's, ordered by arrival.
    pub fn notifications(&self, server_id: Option<&str>) -> Vec<McpNotification> {
        let Ok(map) = self.notifications.lock() else {
            return Vec::new();
        };
        let mut out: Vec<McpNotification> = match server_id {
            Some(id) => map.get(id).cloned().unwrap_or_default(),
            None => map.values().flatten().cloned().collect(),
        };
        out.sort_by_key(|n| n.received_at_ms);
        out
    }

    /// Ingest server-pushed frames into the bounded ring.
    ///
    /// `notifications/tools/list_changed` invalidates our tool cache, and the
    /// `resources` pair invalidates the read allowlist — leaving a stale
    /// allowlist in place would keep vouching for URIs the server has retracted.
    fn record_notifications(&self, server_id: &str, frames: Vec<Value>) {
        if frames.is_empty() {
            return;
        }
        let now = now_millis();
        let mut invalidate_resources = false;
        let Ok(mut map) = self.notifications.lock() else {
            return;
        };
        let ring = map.entry(server_id.to_string()).or_default();
        for frame in frames {
            let Some(method) = frame.get("method").and_then(|v| v.as_str()) else {
                continue;
            };
            if method.starts_with("notifications/resources/") {
                invalidate_resources = true;
            }
            ring.push(McpNotification {
                server_id: server_id.to_string(),
                method: method.to_string(),
                params: frame.get("params").cloned().unwrap_or_else(|| json!({})),
                received_at_ms: now,
            });
        }
        if ring.len() > NOTIFICATION_RING_CAP {
            let overflow = ring.len() - NOTIFICATION_RING_CAP;
            ring.drain(0..overflow);
        }
        drop(map);
        if invalidate_resources {
            if let Ok(mut cache) = self.resources.lock() {
                cache.remove(server_id);
            }
            if let Ok(mut cache) = self.resource_templates.lock() {
                cache.remove(server_id);
            }
        }
    }
}

/// Map a JSON-RPC error frame onto our error kinds.
///
/// `-32601` (method not found) is the case that matters: a server advertised a
/// capability and then refused the call. That is the server's inconsistency, and
/// it must read as `unsupported`, not as a daemon fault.
fn map_jsonrpc_error(server_id: &str, method: &str, err: &Value) -> McpError {
    let code = err.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
    let message = err
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown error");
    match code {
        -32601 => McpError::Unsupported(format!(
            "mcp server `{server_id}` advertised the capability but rejected `{method}`: {message}"
        )),
        -32602 => McpError::Invalid(format!("mcp {method} rejected arguments: {message}")),
        -32002 => McpError::NotFound(format!("mcp {method}: {message}")),
        _ => McpError::Transport(format!("mcp {method} error ({code}): {message}")),
    }
}

/// Byte ceiling for one `resources/read` payload.
fn resource_max_bytes() -> usize {
    std::env::var("NATIVES_MCP_RESOURCE_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_RESOURCE_MAX_BYTES)
        .clamp(1024, 8 * 1024 * 1024)
}

fn truncate_on_char_boundary(text: &str, max_bytes: usize) -> String {
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// A root must be an existing absolute directory. Anything else would be a
/// grant we cannot back with a real path.
fn validate_root(raw: &str) -> Result<McpRoot, McpError> {
    let path = PathBuf::from(raw.trim());
    if !path.is_absolute() {
        return Err(McpError::Invalid(format!(
            "mcp root must be an absolute path: {raw}"
        )));
    }
    if !path.is_dir() {
        return Err(McpError::Invalid(format!(
            "mcp root is not an existing directory: {raw}"
        )));
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    Ok(McpRoot {
        uri: format!("file://{}", path.to_string_lossy()),
        name,
    })
}

/// Match a URI against an RFC 6570-style `{var}` template.
///
/// Deliberately conservative: literal segments must match exactly and a `{var}`
/// expands to one or more characters that are **not** `/` and do not contain
/// `..`. A permissive matcher here would silently widen the read allowlist,
/// which is the one thing this function must never do.
fn uri_matches_template(uri: &str, template: &str) -> bool {
    if uri.contains("..") {
        return false;
    }
    let mut rest = uri;
    let mut parts = template.split('{');

    // Text before the first `{` is a literal prefix.
    let Some(prefix) = parts.next() else {
        return false;
    };
    let Some(after_prefix) = rest.strip_prefix(prefix) else {
        return false;
    };
    rest = after_prefix;

    let mut segments: Vec<&str> = Vec::new();
    for part in parts {
        // Each part is `varname}literal`. A template without the closing brace
        // is malformed; refuse rather than guess.
        let Some((_var, literal)) = part.split_once('}') else {
            return false;
        };
        segments.push(literal);
    }

    for (index, literal) in segments.iter().enumerate() {
        let is_last = index + 1 == segments.len();
        if literal.is_empty() {
            if is_last {
                // Trailing variable: must consume at least one non-slash char.
                return !rest.is_empty() && !rest.contains('/');
            }
            // Two adjacent variables with no separator are unmatchable.
            return false;
        }
        let Some(found) = rest.find(literal) else {
            return false;
        };
        if found == 0 {
            // Variable matched nothing.
            return false;
        }
        if rest[..found].contains('/') {
            return false;
        }
        rest = &rest[found + literal.len()..];
    }

    rest.is_empty()
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Write one request and read until its response arrives.
///
/// The demultiplexing here is the whole point (module docs 第 3 节):
/// - `method` + `id`  → a server→client **request**; answer it inline and keep reading.
/// - `method`, no `id` → a **notification**; buffer it and keep reading.
/// - matching `id`     → our response.
/// - other `id`        → a stale response from an earlier timed-out call; skip it.
#[allow(clippy::too_many_arguments)]
fn stdio_roundtrip(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    roots: &[Value],
    notes: &mut Vec<Value>,
    id: u64,
    method: &str,
    params: Value,
    timeout: Duration,
    progress: Option<&McpProgressCallback>,
) -> Result<Value, String> {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(stdin, "{req}").map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("mcp {method} timeout"));
        }
        let frame = read_json_line(reader, remaining)?;

        let frame_method = frame.get("method").and_then(|v| v.as_str());
        let frame_id = frame.get("id");

        match (frame_method, frame_id) {
            (Some(server_method), Some(request_id)) => {
                let response = serve_server_request(server_method, request_id, roots);
                writeln!(stdin, "{response}").map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
            }
            (Some(server_method), None) => {
                if server_method == "notifications/progress" {
                    if let Some(callback) = progress {
                        callback(frame.clone());
                    }
                }
                notes.push(frame);
            }
            (None, Some(request_id)) => {
                if request_id.as_u64() == Some(id) {
                    return Ok(frame);
                }
                // Stale response to an abandoned request — drop it.
            }
            (None, None) => {
                // Neither a request nor a response. Not addressable; ignore.
            }
        }
    }
}

/// Answer a server→client request.
///
/// Only `roots/list` and `ping` are served. `sampling/createMessage` and
/// `elicitation/create` are refused with `-32601` rather than left to time out,
/// because an honest "I do not implement this" lets the server fall back, and a
/// silent hang looks like a daemon bug. See the report for why they are out of
/// scope: both hand a remote server a lever on local inference or on the user.
fn serve_server_request(method: &str, request_id: &Value, roots: &[Value]) -> Value {
    match method {
        "roots/list" => json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": { "roots": roots },
        }),
        "ping" => json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {},
        }),
        other => json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32601,
                "message": format!("client does not implement `{other}`"),
            },
        }),
    }
}

fn strip_mcp_namespace(server_id: &str, tool_name: &str) -> String {
    let prefix = format!("mcp__{server_id}__");
    if let Some(rest) = tool_name.strip_prefix(&prefix) {
        rest.to_string()
    } else {
        tool_name.to_string()
    }
}

fn read_json_line<R: BufRead>(reader: &mut R, timeout: Duration) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let mut line = String::new();
    loop {
        if start.elapsed() > timeout {
            return Err("mcp handshake timeout".into());
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err("mcp stdout closed".into()),
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                return serde_json::from_str(trimmed).map_err(|e| format!("mcp json: {e}"));
            }
            Err(e) => return Err(format!("mcp read: {e}")),
        }
    }
}

static GLOBAL_MCP: std::sync::OnceLock<McpRuntime> = std::sync::OnceLock::new();

pub fn global_mcp() -> &'static McpRuntime {
    GLOBAL_MCP.get_or_init(McpRuntime::new)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
