//! Stdio MCP session transport (ARCH-002).
//!
//! Owns one live stdio server: spawn, handshake, line-based JSON-RPC
//! demultiplexing (server requests answered inline, notifications buffered,
//! responses matched by id), and TERM-then-KILL shutdown. Extracted from
//! `mcp_runtime.rs` so each transport seam lives with its own protocol, and the
//! facade keeps only registry/lifecycle orchestration.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::*;

impl McpRuntime {
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

    pub(crate) fn call_stdio_tool(
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
    pub(crate) fn stdio_request(
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
}

/// Write one request and read until its response arrives.
///
/// The demultiplexing here is the whole point (module docs 第 3 节):
/// - `method` + `id`  → a server→client **request**; answer it inline and keep reading.
/// - `method`, no `id` → a **notification**; buffer it and keep reading.
/// - matching `id`     → our response.
/// - other `id`        → a stale response from an earlier timed-out call; skip it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stdio_roundtrip(
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
pub(crate) fn spawn_stdio_reader(
    stdout: ChildStdout,
) -> std::sync::mpsc::Receiver<Result<String, String>> {
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
pub(crate) fn stop_stdio_session(session: Arc<StdioSession>) {
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
pub(crate) fn serve_server_request(method: &str, request_id: &Value, roots: &[Value]) -> Value {
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
