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
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use stdio::stop_stdio_session;

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

/// A live stdio session shared between the request/response exchange and
/// `stop()`. The session lives in the registry as an `Arc`; the lock layout is
/// deliberate:
///
/// - `exchange` serialises exchanges and is held for the *whole* roundtrip.
///   `stop()` never takes it, so cancellation never waits on a slow server.
/// - `child` is locked only by `stop()`/liveness — never by an exchange — so
///   killing the child is always possible even mid-read.
/// - `stdin` is locked only around individual writes, so `stop()` can send a
///   protocol cancel and close it between writes.
/// - `lines` is fed by a dedicated reader thread; `recv_timeout` gives every
///   exchange a real deadline even when the child is silent (a blocking
///   `read_line` on the raw pipe could otherwise hang forever).
struct StdioSession {
    exchange: std::sync::Mutex<()>,
    child: Arc<std::sync::Mutex<Child>>,
    stdin: Arc<std::sync::Mutex<Option<ChildStdin>>>,
    lines: std::sync::Mutex<std::sync::mpsc::Receiver<Result<String, String>>>,
    next_id: AtomicU64,
    stopping: Arc<AtomicBool>,
}

/// Ceiling for one JSON-RPC line from a stdio child. A pathological server must
/// not be able to grow the reader's buffer without bound.
const MAX_MCP_LINE_BYTES: usize = 16 * 1024 * 1024;

pub struct McpRuntime {
    registry: Mutex<McpRegistry>,
    sessions: Mutex<HashMap<String, Arc<StdioSession>>>,
    /// Servers whose handshake is in flight. `stop()` can still signal them
    /// via the shared stopping flag even before the session is registered.
    starting: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Background SSE curl children (long-lived ingest).
    sse_children: Mutex<HashMap<String, Child>>,
    /// Reader-thread alive flags per SSE server. `true` while the supervised
    /// reader thread is still draining the curl stdout (real connection fact,
    /// not an optimistic cache). Set `false` on EOF/error/cancel.
    sse_readers: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Join handles for the SSE reader threads, so `stop()`/`remove_server()`
    /// can bound-join them instead of leaking threads.
    sse_reader_handles: Mutex<HashMap<String, std::thread::JoinHandle<()>>>,
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
            starting: Mutex::new(HashMap::new()),
            sse_children: Mutex::new(HashMap::new()),
            sse_readers: Mutex::new(HashMap::new()),
            sse_reader_handles: Mutex::new(HashMap::new()),
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
        // Stop any running listener so no thread/child outlives the server.
        self.cancel_sse_listener(server_id);
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
}

impl McpRuntime {
    pub fn stop(&self, server_id: &str) -> Result<(), String> {
        // Signal a handshake that is still in flight so it aborts.
        if let Ok(mut starting) = self.starting.lock() {
            if let Some(flag) = starting.remove(server_id) {
                flag.store(true, Ordering::SeqCst);
            }
        }
        // Detach the session record first (a brief lock). An in-flight
        // exchange keeps running on its own `Arc`; the child signal below
        // makes its blocking read return.
        let session = {
            let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
            sessions.remove(server_id)
        };
        if let Some(session) = session {
            stop_stdio_session(session);
        }
        // Kill the SSE child and bound-join its reader thread.
        self.cancel_sse_listener(server_id);
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
}

impl McpRuntime {
    /// Liveness: stdio sessions check try_wait; HTTP/SSE re-probe tools count.
    pub fn liveness(&self, server_id: &str) -> Result<Value, String> {
        let config = self.server_config(server_id)?;
        match config.transport {
            McpTransport::Stdio => {
                let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                let session = sessions
                    .get(server_id)
                    .ok_or_else(|| format!("no live stdio session: {server_id}"))?;
                let mut child = session
                    .child
                    .lock()
                    .map_err(|e| format!("mcp child lock: {e}"))?;
                match child.try_wait() {
                    Ok(None) => Ok(json!({
                        "server_id": server_id,
                        "alive": true,
                        "transport": "stdio",
                    })),
                    Ok(Some(status)) => {
                        drop(child);
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
                // Real connection fact: the supervised reader thread is still
                // draining the stream. `false` once EOF/error/cancel flipped it.
                let reader_alive = self
                    .sse_readers
                    .lock()
                    .ok()
                    .and_then(|m| m.get(server_id).map(|f| f.load(Ordering::SeqCst)))
                    .unwrap_or(false);
                let child_alive = self
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
                // Both must hold: a live child with a dead reader means the
                // stream closed / was never supervised.
                let sse_alive = reader_alive && child_alive;
                Ok(json!({
                    "server_id": server_id,
                    "alive": sse_alive,
                    "sse_listening": sse_alive,
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
}

impl McpRuntime {
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

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn strip_mcp_namespace(server_id: &str, tool_name: &str) -> String {
    let prefix = format!("mcp__{server_id}__");
    if let Some(rest) = tool_name.strip_prefix(&prefix) {
        rest.to_string()
    } else {
        tool_name.to_string()
    }
}
static GLOBAL_MCP: std::sync::OnceLock<McpRuntime> = std::sync::OnceLock::new();

pub fn global_mcp() -> &'static McpRuntime {
    GLOBAL_MCP.get_or_init(McpRuntime::new)
}

mod http;
mod resources;
mod stdio;
#[cfg(test)]
mod tests;
