//! 本机独立 Web 服务模式 HTTP/SSE 监听与领域路由（ADR-0042 / F01 / F02 / F03 / F05 / F13）。

use super::auth::AuthManager;
use super::events::handle_events_sse;
use super::http_helpers::{
    extract_cookie_value, is_loopback_host, validate_origin, write_http_raw,
    write_http_raw_with_cache,
};
use super::rpc::{handle_relay, handle_rpc};
use super::static_files::serve_static_file;
use super::surface_routes::handle_surfaces;
use crate::app_store::AppStore;
use crate::workspace_store::WorkspaceStore;
use app_runtime_core::http::HttpRequest;
use app_runtime_core::session::constant_time_eq;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) struct ServiceState {
    pub port: u16,
    pub epoch: String,
    pub relay_secret: String,
    pub auth: Mutex<AuthManager>,
    pub workspace_store: Mutex<Option<WorkspaceStore>>,
    pub app_store: Mutex<Option<AppStore>>,
    pub runtime_manager: super::app_runtime::AppRuntimeManager,
    pub native_surfaces: Mutex<HashMap<String, Instant>>,
    pub request_counts: Mutex<HashMap<String, usize>>,
    pub model_manager: super::model_host::ModelHostManager,
    pub(super) files: super::files::FilesTasks,
    pub sse_senders: Mutex<Vec<(u64, mpsc::SyncSender<String>, Option<String>)>>,
    pub next_sse_id: AtomicU64,
    pub event_sequence: AtomicU64,
    pub active_connections: AtomicUsize,
    pub last_activity: Mutex<Instant>,
    #[allow(dead_code)]
    pub shutdown_signal: Arc<AtomicBool>,
}

impl ServiceState {
    pub(crate) fn new(
        port: u16,
        epoch: String,
        relay_secret: String,
        bootstrap: Option<String>,
        shutdown: Arc<AtomicBool>,
        workspace_store: Option<WorkspaceStore>,
        app_store: Option<AppStore>,
    ) -> Self {
        let runtime_origin = app_store.as_ref().and_then(AppStore::caller_origin);
        Self {
            port,
            epoch,
            relay_secret,
            auth: Mutex::new(AuthManager::new(bootstrap)),
            workspace_store: Mutex::new(workspace_store),
            app_store: Mutex::new(app_store),
            runtime_manager: super::app_runtime::AppRuntimeManager::for_service(
                port,
                runtime_origin,
            ),
            native_surfaces: Mutex::new(HashMap::new()),
            request_counts: Mutex::new(HashMap::new()),
            model_manager: super::model_host::ModelHostManager::default(),
            files: Default::default(),
            sse_senders: Mutex::new(Vec::new()),
            next_sse_id: AtomicU64::new(1),
            event_sequence: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            last_activity: Mutex::new(Instant::now()),
            shutdown_signal: shutdown,
        }
    }

    pub fn touch_activity(&self) {
        if let Ok(mut guard) = self.last_activity.lock() {
            *guard = Instant::now();
        }
    }

    pub fn broadcast_event(&self, event: &str) {
        self.broadcast_to_surface(None, event);
    }

    /// Deliver an event only to the SSE stream owned by `surface`. Task
    /// progress uses this path so another window cannot observe another
    /// window's task timing even though the event carries no paths.
    pub fn broadcast_event_to(&self, surface: &str, event: &str) {
        self.broadcast_to_surface(Some(surface), event);
    }

    fn broadcast_to_surface(&self, target: Option<&str>, event: &str) {
        if let Ok(mut senders) = self.sse_senders.lock() {
            let sequence = self.event_sequence.fetch_add(1, Ordering::Relaxed) + 1;
            let mut payload =
                serde_json::from_str::<Value>(event).unwrap_or_else(|_| json!({"type":"resync"}));
            payload["epoch"] = json!(self.epoch);
            payload["sequence"] = json!(sequence);
            if payload.get("domain").is_none() {
                payload["domain"] = json!("core");
            }
            if payload.get("revision").is_none() {
                payload["revision"] = Value::Null;
            }
            let event = payload.to_string();
            // 256 frames × 4 KiB bounds each slow consumer to at most 1 MiB.
            if event.len() > 4096 {
                senders.clear();
                return;
            }
            senders.retain(|(_, tx, surface)| {
                if target.is_some_and(|scope| surface.as_deref() != Some(scope)) {
                    return true;
                }
                tx.try_send(event.clone()).is_ok()
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 按鉴权、会话、Surface、RPC 和静态资源分离路由职责。
// ---------------------------------------------------------------------------

fn handle_healthz(stream: &mut TcpStream, state: &ServiceState) {
    state.touch_activity();
    let surfaces = state.auth.lock().unwrap().active_surface_count();
    let body = json!({
        "ok": true,
        "service": "native-file-host",
        "version": "0.1.0",
        "epoch": state.epoch,
        "port": state.port,
        "activeSurfaces": surfaces
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    let _ = write_http_raw(stream, 200, "OK", "application/json", &[], &bytes);
}

fn handle_native_ping(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
    let nonce = req.header("x-natives-probe-nonce").unwrap_or("");
    if nonce.len() != 22
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        let _ = write_http_raw(
            stream,
            400,
            "Bad Request",
            "text/plain",
            &[],
            b"invalid nonce",
        );
        return;
    }
    let proof = app_runtime_core::sha256_hex(format!("{}:{nonce}", state.relay_secret).as_bytes());
    let _ = write_http_raw(stream, 200, "OK", "text/plain", &[], proof.as_bytes());
}

fn handle_session_exchange(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
    state.touch_activity();
    if !validate_origin(req, state.port) {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"invalid origin",
        );
        return;
    }

    let Ok(payload) = serde_json::from_slice::<Value>(&req.body) else {
        let _ = write_http_raw(stream, 400, "Bad Request", "text/plain", &[], b"bad JSON");
        return;
    };

    let exchange_res = if let Some(token) = payload.get("bootstrapToken").and_then(Value::as_str) {
        state.auth.lock().unwrap().exchange_bootstrap(token)
    } else if let Some(code) = payload.get("pairingCode").and_then(Value::as_str) {
        state.auth.lock().unwrap().exchange_pairing_code(code)
    } else {
        Err("ERR_PARAM_REQUIRED")
    };

    match exchange_res {
        Ok((session_id, csrf_token)) => {
            let cookie_name = if cfg!(debug_assertions) {
                "natives_dev_session"
            } else {
                "natives_session"
            };
            if let Some(previous) = extract_cookie_value(req, cookie_name) {
                if previous != session_id {
                    state.auth.lock().unwrap().revoke_session(previous);
                }
            }
            let cookie_val =
                format!("{cookie_name}={session_id}; Path=/; SameSite=Strict; HttpOnly");
            let body = json!({
                "ok": true,
                "csrfToken": csrf_token,
                "epoch": state.epoch
            });
            let bytes = serde_json::to_vec(&body).unwrap_or_default();
            let _ = write_http_raw(
                stream,
                200,
                "OK",
                "application/json",
                &[("Set-Cookie", &cookie_val)],
                &bytes,
            );
        }
        Err(err) => {
            let body = json!({ "ok": false, "error": err });
            let bytes = serde_json::to_vec(&body).unwrap_or_default();
            let _ = write_http_raw(stream, 401, "Unauthorized", "application/json", &[], &bytes);
        }
    }
}

fn handle_session_pair_code(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
    state.touch_activity();
    let provided_secret = req.header("x-natives-relay-secret").unwrap_or("");
    if provided_secret.is_empty() || !constant_time_eq(provided_secret, &state.relay_secret) {
        let _ = write_http_raw(
            stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"unauthorized pair-code access",
        );
        return;
    }

    match state.auth.lock().unwrap().generate_pairing_code() {
        Ok(code) => {
            let body = json!({ "ok": true, "pairingCode": code });
            let bytes = serde_json::to_vec(&body).unwrap_or_default();
            let _ = write_http_raw(stream, 200, "OK", "application/json", &[], &bytes);
        }
        Err(err) => {
            let body = json!({ "ok": false, "error": err });
            let bytes = serde_json::to_vec(&body).unwrap_or_default();
            let _ = write_http_raw(stream, 500, "Error", "application/json", &[], &bytes);
        }
    }
}

fn handle_session_check(stream: &mut TcpStream, state: &ServiceState, req: &HttpRequest) {
    state.touch_activity();
    let cookie_name = if cfg!(debug_assertions) {
        "natives_dev_session"
    } else {
        "natives_session"
    };
    let session_id = extract_cookie_value(req, cookie_name).unwrap_or("");
    if !state.auth.lock().unwrap().validate_session(session_id) {
        let body = json!({ "ok": false, "authenticated": false, "error": "ERR_UNAUTHENTICATED" });
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let _ = write_http_raw(stream, 401, "Unauthorized", "application/json", &[], &bytes);
        return;
    }

    if req.method == "GET" {
        let csrf = state
            .auth
            .lock()
            .unwrap()
            .get_csrf_token(session_id)
            .unwrap_or("")
            .to_string();
        let body = json!({
            "ok": true,
            "authenticated": true,
            "csrfToken": csrf,
            "epoch": state.epoch,
            "capabilities": ["files", "workspace", "apps", "model"]
        });
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let _ = write_http_raw(stream, 200, "OK", "application/json", &[], &bytes);
    } else if req.method == "DELETE" {
        if !validate_origin(req, state.port) {
            let _ = write_http_raw(
                stream,
                403,
                "Forbidden",
                "text/plain",
                &[],
                b"invalid origin",
            );
            return;
        }
        let csrf = req.header("x-csrf-token").unwrap_or("");
        if !state.auth.lock().unwrap().validate_csrf(session_id, csrf) {
            let _ = write_http_raw(stream, 403, "Forbidden", "text/plain", &[], b"invalid CSRF");
            return;
        }
        state.auth.lock().unwrap().revoke_session(session_id);
        let cookie_clear = format!("{cookie_name}=; Path=/; Max-Age=0; HttpOnly");
        let _ = write_http_raw(
            stream,
            200,
            "OK",
            "application/json",
            &[("Set-Cookie", &cookie_clear)],
            b"{\"ok\":true}",
        );
    }
}

fn handle_static(stream: &mut TcpStream, route: &str) {
    if let Some((body, mime)) = serve_static_file(route) {
        let _ = write_http_raw_with_cache(
            stream,
            200,
            "OK",
            mime,
            "no-cache",
            &[
                ("X-Content-Type-Options", "nosniff"),
                ("X-Frame-Options", "SAMEORIGIN"),
            ],
            &body,
        );
    } else {
        let _ = write_http_raw(
            stream,
            404,
            "Not Found",
            "text/plain",
            &[],
            b"404 Not Found",
        );
    }
}

pub(crate) fn handle_client(mut stream: TcpStream, state: Arc<ServiceState>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let Ok(req) = HttpRequest::read(&mut stream) else {
        return;
    };

    let port = state.port;
    if !is_loopback_host(req.header("host"), port) {
        let _ = write_http_raw(
            &mut stream,
            403,
            "Forbidden",
            "text/plain",
            &[],
            b"forbidden: invalid loopback host",
        );
        return;
    }

    let route = req.path.split('?').next().unwrap_or(&req.path);

    if route == "/healthz" {
        handle_healthz(&mut stream, &state);
    } else if route == "/native/ping" && req.method == "GET" {
        handle_native_ping(&mut stream, &state, &req);
    } else if route == "/native/relay" && req.method == "POST" {
        handle_relay(&mut stream, &state, &req);
    } else if route == "/web/session/exchange" && req.method == "POST" {
        handle_session_exchange(&mut stream, &state, &req);
    } else if route == "/web/session/pair-code" && req.method == "POST" {
        handle_session_pair_code(&mut stream, &state, &req);
    } else if route == "/web/session" {
        handle_session_check(&mut stream, &state, &req);
    } else if route.starts_with("/web/surfaces") {
        handle_surfaces(&mut stream, &state, &req, route);
    } else if route.starts_with("/web/rpc") && req.method == "POST" {
        handle_rpc(&mut stream, &state, &req);
    } else if matches!(route, "/web/events" | "/native/events") && req.method == "GET" {
        handle_events_sse(&mut stream, &state, &req);
    } else if req.method == "GET" {
        handle_static(&mut stream, route);
    } else {
        let _ = write_http_raw(
            &mut stream,
            404,
            "Not Found",
            "text/plain",
            &[],
            b"404 Not Found",
        );
    }
}

/// Block until the listener has a pending connection or the bounded idle
/// timeout expires. `poll` wakes immediately for real RPC traffic, so query
/// latency is unchanged, while an idle service stops spinning at 200 Hz.
#[cfg(unix)]
fn wait_for_connection(listener: &TcpListener) {
    use std::os::unix::io::AsRawFd;
    let mut descriptor = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // 250 ms only bounds shutdown/reap latency; accepted connections wake at once.
    unsafe {
        libc::poll(&mut descriptor, 1, 250);
    }
}

#[cfg(not(unix))]
fn wait_for_connection(_listener: &TcpListener) {
    thread::sleep(Duration::from_millis(5));
}

pub(crate) fn run_service_listener(
    listener: TcpListener,
    state: Arc<ServiceState>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = listener.set_nonblocking(true);
    let mut last_surface_reap = Instant::now();
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                // macOS accepted sockets inherit O_NONBLOCK from the listener.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                if state.active_connections.fetch_add(1, Ordering::AcqRel) >= 64 {
                    state.active_connections.fetch_sub(1, Ordering::AcqRel);
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    let _ = write_http_raw(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        "text/plain",
                        &[],
                        b"service busy",
                    );
                    continue;
                }
                let state_clone = Arc::clone(&state);
                thread::spawn(move || {
                    let _lease = ConnectionLease(Arc::clone(&state_clone));
                    handle_client(stream, state_clone);
                });
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // Bound idle wakeups while keeping local RPC latency below its 100 ms budget.
                wait_for_connection(&listener);
            }
            Err(_) => break,
        }

        if last_surface_reap.elapsed() < Duration::from_secs(1) {
            continue;
        }
        {
            super::native_lease::reap(&state);
            if let Ok(mut auth) = state.auth.lock() {
                let active = auth.active_surface_ids();
                drop(auth);
                state.runtime_manager.release_missing_web_surfaces(&active);
                let mut all_active = active;
                if let Ok(native) = state.native_surfaces.lock() {
                    all_active.extend(native.keys().cloned());
                }
                state.model_manager.release_missing(&all_active);
                state.files.release_missing(&all_active);
                if state.files.take_changed() {
                    state.broadcast_event(r#"{"type":"files_changed","domain":"files"}"#);
                }
                if state.model_manager.take_changed() {
                    state.broadcast_event(r#"{"type":"model_changed","domain":"model"}"#);
                }
                state.runtime_manager.reap_idle_workers();
            }
            last_surface_reap = Instant::now();
        }

        // 60 秒空闲回收检查：若无活跃 Surface 且空闲超限，优雅触发 shutdown
        let active_surfaces = state
            .auth
            .lock()
            .map(|a| a.active_surface_count())
            .unwrap_or(0);
        if active_surfaces == 0
            && state
                .native_surfaces
                .lock()
                .is_ok_and(|leases| leases.is_empty())
            && !state.runtime_manager.has_workers()
            && state.active_connections.load(Ordering::Acquire) == 0
            && state.sse_senders.lock().is_ok_and(|s| s.is_empty())
        {
            if let Ok(last) = state.last_activity.lock() {
                if Instant::now().duration_since(*last) > Duration::from_secs(60) {
                    shutdown.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    }
    state.broadcast_event("{\"type\":\"shutdown\"}");
}

struct ConnectionLease(Arc<ServiceState>);

impl Drop for ConnectionLease {
    fn drop(&mut self) {
        self.0.active_connections.fetch_sub(1, Ordering::AcqRel);
    }
}
