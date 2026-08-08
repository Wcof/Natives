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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

        let mut cmd = Command::new(&command);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            // Own process group so `stop()` can TERM/KILL the whole tree.
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn mcp stdio failed: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "mcp stdin missing".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "mcp stdout missing".to_string())?;

        let stopping = Arc::new(AtomicBool::new(false));
        // Register as starting so a concurrent `stop()` can signal the
        // handshake even before the session lands in the registry.
        if let Ok(mut starting) = self.starting.lock() {
            starting.insert(server_id.to_string(), stopping.clone());
        }

        let lines_rx = spawn_stdio_reader(stdout);
        let session = Arc::new(StdioSession {
            exchange: std::sync::Mutex::new(()),
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(Some(stdin))),
            lines: Mutex::new(lines_rx),
            next_id: AtomicU64::new(3),
            stopping: stopping.clone(),
        });

        let roots = self.effective_roots();
        let mut pending_notes: Vec<Value> = Vec::new();

        // The handshake happens on the session's own mutexes, so a concurrent
        // stop that signals `stopping` aborts it instead of deadlocking on the
        // registry. Only the exchange/line locks are held across the read.
        let handshake = (|| {
            let _exchange = session
                .exchange
                .lock()
                .map_err(|e| format!("mcp session lock: {e}"))?;
            let mut lines = session
                .lines
                .lock()
                .map_err(|e| format!("mcp lines lock: {e}"))?;

            // Declare only what we actually serve. We answer `roots/list`, so
            // `roots` is advertised; we do not implement sampling or
            // elicitation, so they are absent and a server can adapt instead
            // of failing mid-run.
            let init_resp = stdio_roundtrip(
                &session.stdin,
                &mut lines,
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
                &stopping,
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
            let _ = write_mcp_line(
                &session.stdin,
                &json!({
                    "jsonrpc":"2.0",
                    "method":"notifications/initialized",
                }),
            );

            // Only ask for tools if the handshake said there are tools. Probing a
            // server that never advertised `tools` invites a `-32601` we would then
            // have to paper over as "zero tools" — the exact lie capabilities exist
            // to prevent.
            let tools_resp = if advertises_tools {
                stdio_roundtrip(
                    &session.stdin,
                    &mut lines,
                    &roots,
                    &mut pending_notes,
                    2,
                    "tools/list",
                    json!({}),
                    Duration::from_secs(5),
                    None,
                    &stopping,
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
            Ok::<usize, String>(discovered)
        })();

        // A stopped-while-starting server must not leave a live session behind.
        let registered = if stopping.load(Ordering::SeqCst) {
            Err("mcp stdio session stopped while starting".to_string())
        } else {
            handshake.map(|discovered| {
                if let Ok(mut sessions) = self.sessions.lock() {
                    sessions.insert(server_id.to_string(), session.clone());
                }
                if let Ok(mut st) = self.status.lock() {
                    st.insert(
                        server_id.to_string(),
                        format!("started tools_discovered={discovered}"),
                    );
                }
                self.record_notifications(server_id, pending_notes);
                json!({
                    "server_id": server_id,
                    "tools_discovered": discovered,
                    "status": "started",
                    "transport": "stdio",
                    "session_live": true,
                    "capabilities": self.server_capabilities(server_id),
                    "roots_granted": roots.len(),
                })
            })
        };
        if let Ok(mut starting) = self.starting.lock() {
            starting.remove(server_id);
        }
        if registered.is_err() {
            // Make sure the child is reaped even though the session never
            // entered the registry.
            stop_stdio_session(session);
        }
        registered
    }

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

    /// Start a supervised SSE listener (curl -N). A dedicated reader thread
    /// keeps draining the stream until EOF/error, so `alive` reflects a real,
    /// ongoing connection instead of a short-lived probe. Credentials are
    /// passed via a 0600 temp header file — never in the child argv (P1-031).
    /// `NATIVES_MCP_SSE_MAX_SECS` caps duration (default 60) so tests/CI do
    /// not hang.
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
        // Kill previous listener + reader thread.
        self.cancel_sse_listener(server_id);

        let max_secs = std::env::var("NATIVES_MCP_SSE_MAX_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60)
            .clamp(1, 600);
        let max_secs_arg = max_secs.to_string();

        // Credentials must not appear in the child argv. curl supports
        // `-K -` (config from stdin): headers (Accept + Authorization +
        // custom) are fed through the child's stdin, never as argv args.
        let mut config_lines = String::from("header = \"Accept: text/event-stream\"\n");
        if let Some(auth) = self.resolve_auth_header(&config) {
            config_lines.push_str(&format!("header = \"Authorization: {auth}\"\n"));
        }
        if let Some(headers) = &config.headers {
            for (k, v) in headers {
                if k.eq_ignore_ascii_case("authorization") {
                    continue;
                }
                config_lines.push_str(&format!("header = \"{k}: {v}\"\n"));
            }
        }

        let mut args = vec![
            "-fsS".into(),
            "-N".into(),
            "--max-time".into(),
            max_secs_arg,
            "-K".into(),
            "-".into(),
        ];
        args.push(url.clone());
        let mut child = Command::new("curl")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn sse listener failed: {e}"))?;
        // Feed the header config through stdin, then close it so curl proceeds.
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(config_lines.as_bytes());
            drop(stdin);
        }
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

        // Supervised reader thread: keep draining until EOF (remote closed) or
        // error, then flip the alive flag. `liveness()` derives from this flag
        // plus `child.try_wait()` — never from an optimistic cache.
        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = Arc::clone(&alive);
        let reader_thread = std::thread::spawn(move || {
            let mut drain = BufReader::new(reader);
            let mut buf = String::new();
            let mut drain_line = String::new();
            loop {
                drain_line.clear();
                match drain.read_line(&mut drain_line) {
                    Ok(0) => break, // EOF — connection closed
                    Ok(_) => {
                        buf.push_str(&drain_line);
                        if buf.len() > 256_000 {
                            buf.clear();
                        }
                    }
                    Err(_) => break,
                }
            }
            alive_clone.store(false, Ordering::SeqCst);
        });

        if let Ok(mut kids) = self.sse_children.lock() {
            kids.insert(server_id.to_string(), child);
        }
        if let Ok(mut readers) = self.sse_readers.lock() {
            readers.insert(server_id.to_string(), Arc::clone(&alive));
        }
        if let Ok(mut handles) = self.sse_reader_handles.lock() {
            handles.insert(server_id.to_string(), reader_thread);
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

    /// Kill the SSE child and bound-join its reader thread; remove all
    /// tracking for the server. Safe to call when no listener exists.
    fn cancel_sse_listener(&self, server_id: &str) {
        if let Ok(mut kids) = self.sse_children.lock() {
            if let Some(mut child) = kids.remove(server_id) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Ok(mut readers) = self.sse_readers.lock() {
            readers.remove(server_id);
        }
        if let Ok(mut handles) = self.sse_reader_handles.lock() {
            if let Some(handle) = handles.remove(server_id) {
                let _ = handle.join();
            }
        }
    }

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
        // Resolve roots before taking any session lock: answering `roots/list`
        // mid-exchange must not need a second lock we already hold.
        let roots = self.effective_roots();
        // Lookup-only use of the registry: the mutex is not held across the
        // blocking read/write below, so `stop()` can always detach the session
        // and kill the child without waiting for us (T04).
        let session = {
            let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
            sessions
                .get(server_id)
                .cloned()
                .ok_or_else(|| format!("mcp stdio session not started: {server_id}"))?
        };
        if session.stopping.load(Ordering::SeqCst) {
            return Err(format!("mcp stdio session stopped: {server_id}"));
        }
        // Serialise exchanges; `stop()` never takes this mutex.
        let _exchange = session.exchange.lock().map_err(|e| e.to_string())?;
        if session.stopping.load(Ordering::SeqCst) {
            return Err(format!("mcp stdio session stopped: {server_id}"));
        }
        let id = session.next_id.fetch_add(1, Ordering::SeqCst);
        let mut notes: Vec<Value> = Vec::new();
        let result = {
            let mut lines = session.lines.lock().map_err(|e| e.to_string())?;
            stdio_roundtrip(
                &session.stdin,
                &mut lines,
                &roots,
                &mut notes,
                id,
                method,
                params,
                timeout,
                progress.as_ref(),
                &session.stopping,
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
        // Bounded line channel: a chatty server is backpressured (the reader
        // thread stops draining curl) instead of growing memory unboundedly.
        let (line_tx, line_rx) = std::sync::mpsc::sync_channel::<Result<String, String>>(256);
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
                // Drop the receiver so a reader blocked on a full bounded
                // channel can exit, then join it.
                drop(line_rx);
                let _ = reader_thread.join();
                return Err("mcp call cancelled".into());
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                drop(line_rx);
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
                    drop(line_rx);
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
        drop(line_rx);
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
    stdin: &Mutex<Option<ChildStdin>>,
    lines: &mut std::sync::mpsc::Receiver<Result<String, String>>,
    roots: &[Value],
    notes: &mut Vec<Value>,
    id: u64,
    method: &str,
    params: Value,
    timeout: Duration,
    progress: Option<&McpProgressCallback>,
    stopping: &AtomicBool,
) -> Result<Value, String> {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    write_mcp_line(stdin, &req)?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        if stopping.load(Ordering::SeqCst) {
            return Err("mcp call cancelled".into());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("mcp {method} timeout"));
        }
        // recv_timeout gives a real deadline even when the child is silent —
        // a blocking read on the pipe would hang forever (T04).
        let line = match lines.recv_timeout(remaining) {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => return Err(e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!("mcp {method} timeout"));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("mcp stdout closed".into());
            }
        };
        let frame: Value = serde_json::from_str(&line).map_err(|e| format!("mcp json: {e}"))?;

        let frame_method = frame.get("method").and_then(|v| v.as_str());
        let frame_id = frame.get("id");

        match (frame_method, frame_id) {
            (Some(server_method), Some(request_id)) => {
                let response = serve_server_request(server_method, request_id, roots);
                write_mcp_line(stdin, &response)?;
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

/// Spawn the dedicated reader thread for a stdio child's stdout. Complete lines
/// are pushed over a bounded channel, so a silent child still gives every
/// exchange a real `recv_timeout` deadline and a chatty child is backpressured
/// (the channel fills and the reader stops draining the pipe).
fn spawn_stdio_reader(stdout: ChildStdout) -> std::sync::mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1024);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf: Vec<u8> = Vec::new();
        loop {
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if buf.len() > MAX_MCP_LINE_BYTES {
                        let _ = tx.send(Err("mcp line too large (exceeds 16 MiB cap)".to_string()));
                        break;
                    }
                    if buf.last() == Some(&b'\n') {
                        let line = String::from_utf8_lossy(&buf).trim().to_string();
                        buf.clear();
                        if line.is_empty() {
                            continue;
                        }
                        if tx.send(Ok(line)).is_err() {
                            break; // receiver dropped → session teardown
                        }
                    }
                    // Partial line (no newline yet): keep accumulating.
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("mcp read: {e}")));
                    break;
                }
            }
        }
    });
    rx
}

/// Write one JSON-RPC frame, locking the shared stdin only for the duration of
/// the write. `stop()` can therefore close the pipe between writes (and never
/// waits on an in-flight exchange).
fn write_mcp_line(stdin: &Mutex<Option<ChildStdin>>, value: &Value) -> Result<(), String> {
    let mut guard = stdin.lock().map_err(|e| e.to_string())?;
    let writer = guard
        .as_mut()
        .ok_or_else(|| "mcp stdin closed".to_string())?;
    writeln!(writer, "{value}").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

/// Detach-and-kill a stdio session with a bounded escalation:
/// protocol cancel → close stdin → SIGTERM → grace → SIGKILL → wait.
///
/// The sessions registry mutex is never held here, and `exchange`/`lines`
/// (held by an in-flight exchange) are never taken, so cancellation can
/// always reach the child even while a request is stuck (T04).
fn stop_stdio_session(session: Arc<StdioSession>) {
    session.stopping.store(true, Ordering::SeqCst);
    // Best-effort protocol cancel + stdin close. Use try_lock: the in-flight
    // exchange only holds stdin while writing, but if the child stopped
    // reading a full pipe the writer could be blocked — escalate to the child
    // signal instead of waiting on the pipe.
    if let Ok(mut guard) = session.stdin.try_lock() {
        if let Some(writer) = guard.as_mut() {
            let _ = writeln!(
                writer,
                "{}",
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/cancelled",
                    "params": { "requestId": Value::Null, "reason": "client cancelled" },
                })
            );
            let _ = writer.flush();
        }
        guard.take(); // drop the write end → child sees EOF on its stdin
    }
    let mut child = match session.child.lock() {
        Ok(child) => child,
        Err(_) => return,
    };
    if child.try_wait().ok().flatten().is_some() {
        return; // already exited
    }
    #[cfg(unix)]
    {
        let pid = child.id();
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(_) => break,
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Always SIGKILL the group: a grandchild that ignored TERM must not
        // survive a server stop; an empty group makes this a no-op.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait();
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
            call_rt2.call_tool_with_progress_and_cancel(
                "flood",
                "t",
                json!({}),
                None,
                Some(cancel_cb),
            )
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
}
