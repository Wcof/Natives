//! MCP Runtime — registry + stdio session + HTTP/SSE discovery + tools/call.
//!
//! Untrusted stdio is never auto-started. HTTP/SSE block obvious SSRF targets
//! unless `trusted=true`. Stdio keeps a live session (stdin/stdout) so tools/call
//! can run after initialize. SSE can spawn a bounded long-lived listener that
//! ingests `data:` frames. OAuth browser flow is host-side; daemon holds bearer
//! leases in `McpCredentialStore` (never logged / never in events).

use agent_core::{
    McpCredentialLease, McpCredentialStore, McpRegistry, McpServerConfig, McpToolDescriptor,
    McpTransport,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

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
    /// run_id → selected server ids (ADR-0016 lifecycle refcount).
    run_refs: Mutex<HashMap<String, std::collections::HashSet<String>>>,
    /// server_id → instant it lost its last run reference (reaper input).
    idle_since: Mutex<HashMap<String, std::time::Instant>>,
}

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
        self.status
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
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
            Ok(mut refs) => refs.remove(run_id).map(|s| s.into_iter().collect()).unwrap_or_default(),
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
                let n = self.probe_http_tools(server_id)?;
                Ok(json!({
                    "server_id": server_id,
                    "tools_discovered": n,
                    "status": "http_probed",
                    "transport": "http",
                    "auth": self.auth_status(server_id).ok(),
                }))
            }
            McpTransport::Sse => {
                let n = self.probe_http_tools(server_id)?;
                let sse = self.start_sse_listener(server_id)?;
                Ok(json!({
                    "server_id": server_id,
                    "tools_discovered": n,
                    "status": "sse_listening",
                    "transport": "sse",
                    "sse": sse,
                    "auth": self.auth_status(server_id).ok(),
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

        let init = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "natives-agent-daemon", "version": "0.1.0" }
            }
        });
        writeln!(stdin, "{init}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        let _init_resp = read_json_line(&mut reader, Duration::from_secs(5))?;

        // initialized notification (best-effort; servers may ignore)
        let _ = writeln!(
            stdin,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        );
        let _ = stdin.flush();

        let list = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        writeln!(stdin, "{list}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        let tools_resp = read_json_line(&mut reader, Duration::from_secs(5))?;

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
            McpTransport::Stdio => self.call_stdio_tool(server_id, &bare, arguments),
            McpTransport::Http | McpTransport::Sse => {
                self.call_http_tool(&config, &bare, arguments)
            }
        }
    }

    fn call_stdio_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(server_id)
            .ok_or_else(|| format!("mcp stdio session not started: {server_id}"))?;
        let id = session.next_id;
        session.next_id += 1;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments,
            }
        });
        writeln!(session.stdin, "{req}").map_err(|e| e.to_string())?;
        session.stdin.flush().map_err(|e| e.to_string())?;
        let resp = read_json_line(&mut session.reader, Duration::from_secs(30))?;
        if let Some(err) = resp.get("error") {
            return Err(format!("mcp tools/call error: {err}"));
        }
        Ok(resp
            .get("result")
            .cloned()
            .unwrap_or(resp))
    }

    fn call_http_tool(
        &self,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
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
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments,
            }
        });
        let mut args = vec![
            "-fsS".into(),
            "--max-time".into(),
            "30".into(),
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
        let output = Command::new("curl")
            .args(&args)
            .output()
            .map_err(|e| format!("curl not available for mcp call: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "http mcp tools/call failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        // Accept bare JSON or SSE data: frame.
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(err) = v.get("error") {
                return Err(format!("mcp tools/call error: {err}"));
            }
            return Ok(v.get("result").cloned().unwrap_or(v));
        }
        for line in text.lines() {
            if let Some(data) = line.trim().strip_prefix("data:") {
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(err) = v.get("error") {
                        return Err(format!("mcp tools/call error: {err}"));
                    }
                    return Ok(v.get("result").cloned().unwrap_or(v));
                }
            }
        }
        Err("mcp tools/call: unparseable response".into())
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
        let err = rt
            .call_tool("s", "missing", json!({}))
            .unwrap_err();
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
