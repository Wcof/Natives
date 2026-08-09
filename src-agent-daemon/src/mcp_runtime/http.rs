//! HTTP/SSE MCP transport (ARCH-002).
//!
//! Discovery (`tools/list`, `/tools` legacy shape, SSE `data:` frame ingest),
//! capability handshake, and the request/response RPC path with SSRF gating and
//! bounded flood-cancellation. Extracted from `mcp_runtime.rs`.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::*;

impl McpRuntime {
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
    pub(crate) fn cancel_sse_listener(&self, server_id: &str) {
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
    pub(crate) fn call_http_tool(
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
    pub(crate) fn ingest_tools_payload(
        &self,
        server_id: &str,
        body: &str,
    ) -> Result<usize, String> {
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

    pub(crate) fn server_config(&self, server_id: &str) -> Result<McpServerConfig, String> {
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
    pub(crate) fn http_handshake(&self, config: &McpServerConfig) {
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
    pub(crate) fn require_capability(&self, server_id: &str, key: &str) -> Result<(), McpError> {
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
    pub(crate) fn request(
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
}

/// Map a JSON-RPC error frame onto our error kinds.
///
/// `-32601` (method not found) is the case that matters: a server advertised a
/// capability and then refused the call. That is the server's inconsistency, and
/// it must read as `unsupported`, not as a daemon fault.
pub(crate) fn map_jsonrpc_error(server_id: &str, method: &str, err: &Value) -> McpError {
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
