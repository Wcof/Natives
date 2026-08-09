//! Workshop local HTTP server: serves module / draft / preview / local-project
//! files over an allowlisted loopback port and routes Bridge requests. Split by
//! responsibility:
//!   - `mod.rs` — server core (WorkerPool, HttpServer, request dispatch)
//!   - `validate` — host / origin / path validation kernel
//!   - `serve` — static, draft, preview, local-project file serving
//!   - `bridge` — Workshop Bridge request handling + namespace routing
//!   - `content` — HTML injection and MIME guessing

use crate::token_manager::TokenManager;

use rusqlite::Connection;

use std::path::{Path, PathBuf};

use std::sync::{Arc, Condvar, Mutex};

use tiny_http::{Header, Method, Request, Response, Server};

pub(crate) fn get_header(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

pub(crate) const ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Maximum concurrent request handler threads (CR-402: bounded workers).
const MAX_CONCURRENT_WORKERS: usize = 16;

/// CSP for published Workshop modules — strict, no external connect-src, no eval.
/// Modules run in iframe sandbox (allow-scripts allow-forms); this is a
/// defense-in-depth layer against sandbox escape (R-S6). `frame-ancestors` is
/// deliberately NOT 'none' here: modules are displayed inside a sandboxed
/// iframe, so blocking frame embedding would contradict the display model
/// (P0-013). The sandbox attribute (no allow-same-origin) is the isolation
/// boundary; CSP `frame-ancestors` is omitted for the module frame so the
/// browser permits the intended iframe embedding.
const WORKSHOP_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:*; form-action 'none'";

/// CSP for draft previews — same as Workshop (drafts are unreviewed model
/// output, must not have weaker CSP than a published module).
const DRAFT_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:*; form-action 'none'";

/// CSP for local creative projects — allows loopback WS for Vite HMR, data:
/// and blob: for hot-reload, and https: for external CDN resources (the
/// project's own code, not the Natives sandbox).
const LOCAL_PROJECT_CSP: &str = "default-src 'self' data: blob: https:; script-src 'self' 'unsafe-inline' 'unsafe-eval' https:; style-src 'self' 'unsafe-inline' https:; img-src 'self' data: blob: https:; font-src 'self' data: https:; connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:* https: wss:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

/// CSP for bridge API responses — strict, no external resources.
/// The bridge is a pure JSON API endpoint, not a rendered page, so CSP is
/// defense-in-depth only.
const BRIDGE_CSP: &str = "default-src 'none'; frame-ancestors 'none'; form-action 'none'";

/// CSP for authorized HTML preview resources served via `/fs/{token}/{path}`.
/// Same strictness as Workshop/Draft (unreviewed local HTML output must not have
/// a weaker CSP than a published module): no external connect-src, no eval.
/// The HTML document itself is displayed inside a sandboxed iframe, so
/// `frame-ancestors` is deliberately omitted for the same reason as Workshop
/// (P0-013) — the sandbox attribute is the isolation boundary.
const PREVIEW_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:*; form-action 'none'";

/// Maximum POST body size for bridge requests (64 MiB). Requests exceeding
/// this limit receive a 413 response before any body is read (CR-402).
const MAX_BRIDGE_BODY: u64 = 64 * 1024 * 1024; // 64 MiB

/// Bounded worker permit — tracks the number of active request handlers.
/// Uses a Mutex+Cdvar pair so that the accept loop blocks when the worker
/// pool is full (CR-402: bounded workers).
struct WorkerPool {
    available: Mutex<u32>,
    condvar: Condvar,
}

impl WorkerPool {
    fn new(max: u32) -> Self {
        Self {
            available: Mutex::new(max),
            condvar: Condvar::new(),
        }
    }

    /// Acquire a worker permit. Blocks until a permit is available.
    fn acquire(&self) -> WorkerPermit<'_> {
        let mut count = self.available.lock().unwrap_or_else(|e| e.into_inner());
        while *count == 0 {
            count = self.condvar.wait(count).unwrap_or_else(|e| e.into_inner());
        }
        *count -= 1;
        WorkerPermit { pool: self }
    }

    fn release(&self) {
        let mut count = self.available.lock().unwrap_or_else(|e| e.into_inner());
        *count += 1;
        self.condvar.notify_one();
    }
}

/// Guard that releases a worker permit on drop.
struct WorkerPermit<'a> {
    pool: &'a WorkerPool,
}

impl<'a> Drop for WorkerPermit<'a> {
    fn drop(&mut self) {
        self.pool.release();
    }
}

pub struct HttpServer {
    port: u16,
    modules_dir: PathBuf,
    token_manager: Arc<TokenManager>,
    db_path: PathBuf,
}

impl HttpServer {
    pub fn new(modules_dir: PathBuf, token_manager: Arc<TokenManager>, db_path: PathBuf) -> Self {
        Self {
            port: 0,
            modules_dir,
            token_manager,
            db_path,
        }
    }

    /// Start the HTTP server on the given port (0 = OS-assigned).
    /// Returns the actual port.
    pub fn start(&mut self, port: u16) -> Result<u16, String> {
        let addr = format!("127.0.0.1:{port}");
        let server =
            Server::http(&addr).map_err(|e| format!("failed to start HTTP server: {e}"))?;
        let actual_port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .unwrap_or(port);
        self.port = actual_port;

        let modules_dir = self.modules_dir.clone();
        let token_manager = self.token_manager.clone();
        let db_path = self.db_path.clone();
        let pool = Arc::new(WorkerPool::new(MAX_CONCURRENT_WORKERS as u32));

        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let modules_dir = modules_dir.clone();
                let token_manager = token_manager.clone();
                let db_path = db_path.clone();
                let pool = pool.clone();
                std::thread::spawn(move || {
                    // CR-402: bounded workers — acquire a permit from the pool.
                    // If the pool is full, the accept loop blocks until a worker
                    // completes, preventing unbounded thread creation.
                    let _permit = pool.acquire();
                    if let Err(e) = handle_request(request, &modules_dir, &token_manager, &db_path)
                    {
                        eprintln!("request error: {e}");
                    }
                });
            }
        });

        Ok(actual_port)
    }

    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }
}

fn handle_request(
    request: Request,
    modules_dir: &Path,
    token_manager: &TokenManager,
    db_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Host validation (DNS rebinding protection)
    if let Some(host) = get_header(&request, "Host") {
        if !validate_host(&host) {
            let resp = Response::from_string("Forbidden").with_status_code(403);
            request.respond(resp)?;
            return Ok(());
        }
    }

    let url = request.url().to_string();
    // Strip query string for routing
    let path_only = url.split('?').next().unwrap_or(&url).to_string();
    let method = request.method().clone();

    // 2. Route matching — only GET/HEAD for static assets; POST for bridge only.
    //    CSP is partitioned per-domain (CR-402): Workshop modules, Drafts, Local
    //    Projects, and Bridge each get a different CSP header.
    match &method {
        Method::Get | Method::Head => {
            let workshop_csp = Header::from_bytes("Content-Security-Policy", WORKSHOP_CSP)
                .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());
            let draft_csp = Header::from_bytes("Content-Security-Policy", DRAFT_CSP)
                .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());

            if path_only == "/natives-sdk.js" {
                // Serve the bridge SDK — Workshop CSP applies. Inject the real
                // origin/port from the request's Host header so the SDK has no
                // `__NATIVES_*__` placeholder (P0-009): the bridge target is
                // this same local server the module was loaded from.
                let host = get_header(&request, "Host").unwrap_or_else(|| "localhost".to_string());
                let origin = format!("http://{host}");
                let port = host.rsplit(':').next().unwrap_or("").to_string();
                let script = include_str!("../bridge_sdk.js")
                    .replace("__NATIVES_ORIGIN__", &origin)
                    .replace("__NATIVES_PORT__", &port);
                let resp = Response::from_string(script)
                    .with_header(workshop_csp)
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                request.respond(resp)?;
            } else if path_only.starts_with("/modules/") {
                // Serve module static files — Workshop CSP (strict)
                serve_module_file(request, modules_dir, workshop_csp)?;
            } else if path_only.starts_with("/drafts/") {
                // Draft preview — Draft CSP (same strictness as Workshop)
                serve_draft_file(request, modules_dir, db_path, draft_csp)?;
            } else if path_only.starts_with("/local-projects/") {
                // Local projects — Local Project CSP (allows Vite HMR loopback WS)
                let local_csp = Header::from_bytes("Content-Security-Policy", LOCAL_PROJECT_CSP)
                    .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());
                serve_local_project_file(
                    request,
                    db_path,
                    local_csp,
                    matches!(method, Method::Head),
                )?;
            } else if path_only.starts_with("/fs/") {
                // Authorized HTML preview resources — per-request session token
                // binding + containment validation (PREV-001). Preview CSP.
                let preview_csp = Header::from_bytes("Content-Security-Policy", PREVIEW_CSP)
                    .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());
                serve_preview_file(request, preview_csp, matches!(method, Method::Head))?;
            } else {
                let resp = Response::from_string("Not Found").with_status_code(404);
                request.respond(resp)?;
            }
        }
        Method::Post => {
            // 3. Origin validation for POST (CSRF protection)
            let origin = get_header(&request, "Origin").map(|h| h.to_string());
            let referer = get_header(&request, "Referer").map(|h| h.to_string());
            if !validate_origin(&origin, &referer) {
                let resp = Response::from_string("Forbidden").with_status_code(403);
                request.respond(resp)?;
                return Ok(());
            }

            if path_only.starts_with("/api/bridge/") {
                let bridge_csp = Header::from_bytes("Content-Security-Policy", BRIDGE_CSP)
                    .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());
                handle_bridge_request(request, token_manager, bridge_csp, db_path)?;
            } else {
                let resp = Response::from_string("Not Found").with_status_code(404);
                request.respond(resp)?;
            }
        }
        _ => {
            let resp = Response::from_string("Method Not Allowed").with_status_code(405);
            request.respond(resp)?;
        }
    }

    Ok(())
}

mod bridge;
mod content;
mod serve;
mod validate;

pub(crate) use bridge::*;
pub(crate) use content::*;
pub(crate) use serve::*;
pub(crate) use validate::*;

#[cfg(test)]
mod http_server_tests;
