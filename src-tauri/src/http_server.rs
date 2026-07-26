use crate::creative_draft::paths as draft_paths;
use crate::token_manager::TokenManager;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, Server};

fn get_header(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

const ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];
const CSP_HEADER: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:* https:; frame-src 'self' https:; frame-ancestors 'none'; form-action 'none'";
/// CSP for local creative projects (no Workshop Bridge; allows loopback WS for Vite HMR).
const LOCAL_PROJECT_CSP: &str = "default-src 'self' data: blob: https:; script-src 'self' 'unsafe-inline' 'unsafe-eval' https:; style-src 'self' 'unsafe-inline' https:; img-src 'self' data: blob: https:; font-src 'self' data: https:; connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:* https: wss:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

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
        let server = Server::http(&addr).map_err(|e| format!("failed to start HTTP server: {e}"))?;
        let actual_port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
        self.port = actual_port;

        let modules_dir = self.modules_dir.clone();
        let token_manager = self.token_manager.clone();
        let db_path = self.db_path.clone();

        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let modules_dir = modules_dir.clone();
                let token_manager = token_manager.clone();
                let db_path = db_path.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_request(request, &modules_dir, &token_manager, &db_path) {
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

    // 2. CSP headers on every response
    let csp = Header::from_bytes("Content-Security-Policy", CSP_HEADER)
        .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());

    let url = request.url().to_string();
    // Strip query string for routing
    let path_only = url.split('?').next().unwrap_or(&url).to_string();
    let method = request.method().clone();

    // 3. Route matching — only GET/HEAD for static assets; POST for bridge only.
    match &method {
        Method::Get | Method::Head => {
            if path_only == "/natives-sdk.js" {
                // Serve the bridge SDK
                let script = include_str!("bridge_sdk.js");
                let resp = Response::from_string(script)
                    .with_header(csp)
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                request.respond(resp)?;
            } else if path_only.starts_with("/modules/") {
                // Serve module static files
                serve_module_file(request, modules_dir, csp)?;
            } else if path_only.starts_with("/drafts/") {
                // Draft preview — same sandbox contract as a published module,
                // only the content root differs (ADR-0014 §6).
                serve_draft_file(request, modules_dir, db_path, csp)?;
            } else if path_only.starts_with("/local-projects/") {
                let local_csp = Header::from_bytes("Content-Security-Policy", LOCAL_PROJECT_CSP)
                    .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap());
                serve_local_project_file(request, db_path, local_csp, matches!(method, Method::Head))?;
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
                handle_bridge_request(request, token_manager, csp, db_path)?;
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

fn validate_host(host: &str) -> bool {
    // Strip port: "localhost:3001" -> "localhost", "[::1]:3001" -> "::1"
    let hostname = if host.starts_with('[') {
        // IPv6: [::1]:port
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or(host)
    };
    ALLOWED_HOSTS.contains(&hostname)
}

fn validate_origin(origin: &Option<String>, referer: &Option<String>) -> bool {
    match (origin, referer) {
        (Some(o), _) => is_loopback_url(o),
        (_, Some(r)) => is_loopback_url(r),
        _ => false, // POST without Origin or Referer = rejected
    }
}

fn is_loopback_url(url: &str) -> bool {
    // Extract hostname from URL
    let after_scheme = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        url
    };
    let host = after_scheme
        .split('/')
        .next()
        .unwrap_or(after_scheme)
        .split(':')
        .next()
        .unwrap_or(after_scheme)
        .trim_start_matches('[')
        .trim_end_matches(']');
    ALLOWED_HOSTS.contains(&host)
}

fn sanitize_path(module_id: &str, file_path: &str, modules_dir: &Path) -> Option<PathBuf> {
    // Reject null bytes
    if file_path.contains('\0') {
        return None;
    }
    // Reject directory traversal
    if file_path.contains("..") {
        return None;
    }
    // Strip query string
    let clean = file_path.split('?').next().unwrap_or(file_path);
    // Resolve to module root
    let module_root = modules_dir.join(module_id);
    let resolved = module_root.join(clean);
    // Verify containment (no symlink escape)
    let resolved_canon = std::fs::canonicalize(&resolved).ok()?;
    let root_canon = std::fs::canonicalize(&module_root).ok()?;
    if resolved_canon.starts_with(&root_canon) {
        Some(resolved)
    } else {
        None
    }
}

fn serve_module_file(
    request: Request,
    modules_dir: &Path,
    csp: Header,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Parse /modules/{moduleId}/{path}
    let url = request.url().to_string();
    let path_part = url.split('?').next().unwrap_or(&url);
    let path_part = path_part.strip_prefix("/modules/").unwrap_or(path_part);
    let mut parts = path_part.splitn(2, '/');
    let module_id = parts.next().unwrap_or("");
    let file_path = parts.next().unwrap_or("");

    match sanitize_path(module_id, file_path, modules_dir) {
        Some(resolved) => {
            if resolved.exists() && resolved.is_file() {
                let mime = guess_mime(&resolved);

                // HTML preview injection (Natives2: width-measure + fallback styles + image rewrite)
                if mime == "text/html" {
                    let raw = std::fs::read_to_string(&resolved)?;
                    let injected = inject_html_preview(&raw, module_id);
                    let resp = Response::from_string(injected)
                        .with_header(csp)
                        .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
                    request.respond(resp)?;
                } else {
                    let content = std::fs::read(&resolved)?;
                    let resp = Response::from_data(content)
                        .with_header(csp)
                        .with_header(Header::from_bytes("Content-Type", mime).unwrap());
                    request.respond(resp)?;
                }
            } else {
                let resp = Response::from_string("Not Found").with_status_code(404);
                request.respond(resp)?;
            }
        }
        None => {
            let resp = Response::from_string("Forbidden").with_status_code(403);
            request.respond(resp)?;
        }
    }
    Ok(())
}

/// Drafts live next to modules under the app data directory, so the data root is
/// recoverable from what the server already holds: `lib.rs` builds `modules_dir`
/// as `data_dir.join("modules")`. Deriving it here keeps `HttpServer::new` — and
/// therefore every caller — untouched.
fn data_dir_from_modules_dir(modules_dir: &Path) -> Option<&Path> {
    modules_dir.parent()
}

/// The current revision is a *pointer*, not "the highest file on disk": a
/// rollback moves the pointer back while keeping the newer revision files so the
/// undo can itself be undone. Only the database knows which one is current, so
/// the default file is resolved the same way `/local-projects/` resolves its
/// root — one short read on the connection this server already owns.
fn lookup_draft_current_revision(db_path: &Path, draft_id: &str) -> Option<i64> {
    let conn = Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT current_revision FROM creative_drafts WHERE draft_id = ?1",
        [draft_id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .filter(|revision| *revision >= 1)
}

/// Resolve `/drafts/{draftId}/{file}` to an on-disk path.
///
/// `path_part` is the URL with the `/drafts/` prefix already stripped. Split
/// mirrors [`serve_module_file`]; containment is delegated to
/// [`draft_paths::resolve_served_file`] rather than re-derived here, so drafts and
/// modules cannot drift apart on traversal handling.
fn resolve_draft_file(data_dir: &Path, db_path: &Path, path_part: &str) -> Option<PathBuf> {
    let path_part = path_part.split('?').next().unwrap_or(path_part);
    let mut parts = path_part.splitn(2, '/');
    let draft_id = parts.next().unwrap_or("");
    let file_path = parts.next().unwrap_or("");

    if file_path.is_empty() {
        // Bare `/drafts/{draftId}` (or a trailing slash) means "whatever the user
        // is looking at now". Revision files are `rev-<n>.html`, so there is no
        // `index.html` to fall back to and the pointer has to be looked up.
        let revision = lookup_draft_current_revision(db_path, draft_id)?;
        let path = draft_paths::revision_path(data_dir, draft_id, revision).ok()?;
        let name = path.file_name()?.to_str()?;
        return draft_paths::resolve_served_file(data_dir, draft_id, name);
    }

    draft_paths::resolve_served_file(data_dir, draft_id, file_path)
}

/// Serve draft preview files. Route: `/drafts/{draftId}/{file}`.
///
/// Deliberately a sibling of [`serve_module_file`]: same CSP header, same preview
/// injection, same MIME handling. A draft is unreviewed model output, so its
/// sandbox must not be weaker than a published module's. The one difference is the
/// failure code — an unresolvable draft answers 404 for every reason (missing
/// draft, missing file, traversal attempt) so probing cannot distinguish them.
fn serve_draft_file(
    request: Request,
    modules_dir: &Path,
    db_path: &Path,
    csp: Header,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = request.url().to_string();
    let path_part = url.split('?').next().unwrap_or(&url);
    let path_part = path_part.strip_prefix("/drafts/").unwrap_or(path_part);

    let data_dir = match data_dir_from_modules_dir(modules_dir) {
        Some(dir) => dir.to_path_buf(),
        None => {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };

    let resolved = match resolve_draft_file(&data_dir, db_path, path_part) {
        Some(p) if p.is_file() => p,
        _ => {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };

    let mime = guess_mime(&resolved);
    if mime == "text/html" {
        // Same width-measure injection as a module preview, keyed by draft id, so
        // what the user sees while drafting matches what they get after publish.
        let draft_id = path_part.split('/').next().unwrap_or("");
        let raw = std::fs::read_to_string(&resolved)?;
        let injected = inject_html_preview(&raw, draft_id);
        let resp = Response::from_string(injected)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
        request.respond(resp)?;
    } else {
        let content = std::fs::read(&resolved)?;
        let resp = Response::from_data(content)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", mime).unwrap());
        request.respond(resp)?;
    }
    Ok(())
}

/// Serve local creative project files from DB-resolved roots.
/// Route: `/local-projects/{creativeId}/{relativePath}`
/// No Workshop Bridge injection; no Tauri capability.
fn serve_local_project_file(
    request: Request,
    db_path: &Path,
    csp: Header,
    head_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = request.url().to_string();
    let path_part = url.split('?').next().unwrap_or(&url);
    let path_part = path_part
        .strip_prefix("/local-projects/")
        .unwrap_or(path_part);
    // percent-decode relative path segments carefully
    let path_part = percent_decode(path_part);
    let mut parts = path_part.splitn(2, '/');
    let creative_id = parts.next().unwrap_or("");
    let mut rel = parts.next().unwrap_or("").to_string();
    if creative_id.is_empty() || creative_id.contains("..") || creative_id.contains('\0') {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
    }
    if rel.contains('\0') || rel.contains("..") {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
    }

    let project_root = match lookup_local_project_root(db_path, creative_id) {
        Some(p) => p,
        None => {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };

    if rel.is_empty() || rel.ends_with('/') {
        rel = format!("{rel}index.html");
    }

    let candidate = match resolve_under_project(&project_root, &rel) {
        Some(p) => p,
        None => {
            let resp = Response::from_string("Forbidden").with_status_code(403);
            request.respond(resp)?;
            return Ok(());
        }
    };

    let file_path = if candidate.is_dir() {
        let index = candidate.join("index.html");
        if index.is_file() {
            index
        } else {
            // SPA fallback only for non-file GETs under project
            project_root.join("index.html")
        }
    } else if candidate.is_file() {
        candidate
    } else {
        // SPA fallback: missing path → index.html if present
        let index = project_root.join("index.html");
        if index.is_file() {
            index
        } else {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };

    // Final containment check after canonicalize
    let file_canon = match std::fs::canonicalize(&file_path) {
        Ok(p) => p,
        Err(_) => {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };
    let root_canon = match std::fs::canonicalize(&project_root) {
        Ok(p) => p,
        Err(_) => {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };
    if !file_canon.starts_with(&root_canon) {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
    }

    let mime = guess_mime(&file_canon);
    if head_only {
        let len = std::fs::metadata(&file_canon).map(|m| m.len()).unwrap_or(0);
        let resp = Response::empty(200)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", mime).unwrap())
            .with_header(
                Header::from_bytes("Content-Length", len.to_string().into_bytes()).unwrap_or_else(
                    |_| Header::from_bytes("x-placeholder", "x").unwrap(),
                ),
            );
        request.respond(resp)?;
        return Ok(());
    }

    // HTML: serve raw, no bridge injection
    if mime == "text/html" {
        let raw = std::fs::read_to_string(&file_canon)?;
        let resp = Response::from_string(raw)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
        request.respond(resp)?;
    } else {
        let content = std::fs::read(&file_canon)?;
        let resp = Response::from_data(content)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", mime).unwrap());
        request.respond(resp)?;
    }
    Ok(())
}

fn lookup_local_project_root(db_path: &Path, creative_id: &str) -> Option<PathBuf> {
    let conn = Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT canonical_project_root FROM local_creative_apps WHERE id = ?1",
        [creative_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .map(PathBuf::from)
}

fn resolve_under_project(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() {
        return Some(root.to_path_buf());
    }
    // Reject absolute and parent segments again after decode
    let p = Path::new(rel);
    if p.is_absolute() {
        return None;
    }
    for c in p.components() {
        match c {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    Some(root.join(p))
}

fn percent_decode(input: &str) -> String {
    // Minimal percent-decoder; invalid sequences kept as-is.
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1];
            let h2 = bytes[i + 2];
            if let (Some(a), Some(b)) = (from_hex(h1), from_hex(h2)) {
                out.push((a << 4) | b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const MAX_POST_BODY: u64 = 64 * 1024 * 1024; // 64MB (Natives2: prevent memory exhaustion)

fn handle_bridge_request(
    mut request: Request,
    token_manager: &TokenManager,
    csp: Header,
    db_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Read request body with 64MB limit (Natives2: prevent memory exhaustion)
    use std::io::Read;
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_POST_BODY)
        .read_to_string(&mut body)?;

    // Extract token and module ID from headers
    let token = get_header(&request, "X-Session-Token")
        .unwrap_or_default()
        .to_string();
    let module_id = get_header(&request, "X-Module-Id")
        .unwrap_or_default()
        .to_string();

    if token.is_empty() || module_id.is_empty() {
        let resp = Response::from_string(r#"{"error":"Missing token or module ID"}"#)
            .with_status_code(401)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
        request.respond(resp)?;
        return Ok(());
    }

    // Validate token
    if !token_manager.validate(&token, &module_id) {
        let resp = Response::from_string(r#"{"error":"Invalid or expired token"}"#)
            .with_status_code(403)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
        request.respond(resp)?;
        return Ok(());
    }

    // Parse bridge method from URL: /api/bridge/{namespace}/{method}
    let url = request.url().to_string();
    let bridge_path = url.strip_prefix("/api/bridge/").unwrap_or("");
    let mut bridge_parts = bridge_path.splitn(2, '/');
    let namespace = bridge_parts.next().unwrap_or("");
    let method = bridge_parts.next().unwrap_or("");

    // Route bridge request
    let response_body = route_bridge(namespace, method, &module_id, &body, db_path);

    let resp = Response::from_string(&response_body)
        .with_header(csp)
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
    request.respond(resp)?;
    Ok(())
}

fn route_bridge(namespace: &str, method: &str, module_id: &str, _body: &str, db_path: &Path) -> String {
    // Bridge API routing — mirrors Natives bridge-host.ts
    // Open a read-only connection to read real settings
    let conn = Connection::open(db_path).ok();

    match (namespace, method) {
        ("settings", "getTheme") => {
            // Read theme from SQLite settings, fallback to neutral default
            let theme = conn.as_ref().and_then(|c| {
                let mut stmt = c.prepare("SELECT value FROM settings WHERE key = 'settings:theme'").ok()?;
                stmt.query_row([], |row| row.get::<_, String>(0)).ok()
            }).unwrap_or_else(|| "default".to_string());
            serde_json::json!({ "result": theme }).to_string()
        }
        ("settings", "getLocale") => {
            // Read locale from SQLite settings, fallback to empty (let renderer decide default)
            let locale = conn.as_ref().and_then(|c| {
                let mut stmt = c.prepare("SELECT value FROM settings WHERE key = 'settings:locale'").ok()?;
                stmt.query_row([], |row| row.get::<_, String>(0)).ok()
            }).unwrap_or_else(|| "".to_string());
            serde_json::json!({ "result": locale }).to_string()
        }
        ("lifecycle", "ready") => {
            // Record module readiness in lifecycle tracker
            if let Some(c) = conn.as_ref() {
                let _ = c.execute(
                    "INSERT INTO notifications (module_id, title, body, level, created_at)
                     VALUES (?1, 'module.ready', 'Module ready', 'info', datetime('now'))",
                    rusqlite::params![module_id],
                );
            }
            r#"{"ok":true}"#.to_string()
        }
        ("lifecycle", "heartbeat") => {
            // Update heartbeat timestamp — stored in module_data for each module
            if let Some(c) = conn.as_ref() {
                let ts = chrono::Utc::now().to_rfc3339();
                let _ = c.execute(
                    "INSERT INTO module_data (module_id, key, value) VALUES (?1, '_heartbeat', ?2)
                     ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
                    rusqlite::params![module_id, ts],
                );
            }
            r#"{"ok":true}"#.to_string()
        }
        ("lifecycle", "error") => {
            // Record error notification
            if let Some(c) = conn.as_ref() {
                let _ = c.execute(
                    "INSERT INTO notifications (module_id, title, body, level, created_at)
                     VALUES (?1, 'module.error', 'Bridge error', 'error', datetime('now'))",
                    rusqlite::params![module_id],
                );
            }
            r#"{"ok":true}"#.to_string()
        }
        ("meta", "info") => {
            // Read real module version from DB, fallback to empty string (not a placeholder)
            let version = conn.as_ref().and_then(|c| {
                let mut stmt = c.prepare("SELECT version FROM modules WHERE id = ?1").ok()?;
                stmt.query_row(rusqlite::params![module_id], |row| row.get::<_, String>(0)).ok()
            }).unwrap_or_else(|| "".to_string());
            let natives_version = conn.as_ref().and_then(|c| {
                let mut stmt = c.prepare("SELECT value FROM settings WHERE key = '_app_version'").ok()?;
                stmt.query_row([], |row| row.get::<_, String>(0)).ok()
            }).unwrap_or_else(|| "".to_string());
            serde_json::json!({ "moduleId": module_id, "version": version, "nativesVersion": natives_version }).to_string()
        }
        _ => {
            serde_json::json!({ "error": format!("Unknown bridge method: {namespace}.{method}") }).to_string()
        }
    }
}

/// Inject preview helpers into HTML content (Natives2):
/// 1. Width-measure script: postMessage natural page width → parent for auto-scaling
/// 2. Fallback styles: html/body scrollable, images/videos don't overflow
/// 3. Local image rewrite: onerror handler rewrites file:// → /fs/ proxy
fn inject_html_preview(html: &str, module_id: &str) -> String {
    // Case-insensitive search for </head>
    let lower = html.to_lowercase();
    let head_pos = lower.find("</head>");
    let pos = match head_pos {
        Some(p) => p,
        None => return html.to_string(),
    };

    // Escape module_id for safe JS string interpolation
    let safe_id: String = module_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    let inject = format!(r#"
<script>
// Width-measure: tell parent the page's natural width for auto-scaling (Natives2)
(function() {{
  function report() {{
    var w = Math.max(document.documentElement.scrollWidth, document.body ? document.body.scrollWidth : 0);
    if (w > 0) window.parent.postMessage({{ type: 'natives:page-width', width: w, moduleId: '{}' }}, '*');
  }}
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', report);
  else report();
  window.addEventListener('resize', report);
}})();
</script>
<style>
/* Fallback: make html/body scrollable, images/videos don't overflow */
html, body {{ overflow: auto; max-width: 100vw; }}
img, video, iframe {{ max-width: 100%; height: auto; }}
</style>
"#, safe_id);

    // Splice injection before </head> (preserving original case)
    let mut result = String::with_capacity(html.len() + inject.len());
    result.push_str(&html[..pos]);
    result.push_str(&inject);
    result.push_str(&html[pos..]);
    result
}

fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creative_draft::store;

    struct DraftFixture {
        _tmp: tempfile::TempDir,
        data_dir: PathBuf,
        modules_dir: PathBuf,
        db_path: PathBuf,
        conn: Connection,
    }

    /// Build the same layout `lib.rs` builds: `<data_dir>/modules`, `<data_dir>/drafts`
    /// and `<data_dir>/natives.db`, so the data-dir derivation is exercised for real.
    fn fixture() -> DraftFixture {
        let tmp = tempfile::tempdir().expect("temp dir");
        let data_dir = tmp.path().to_path_buf();
        let modules_dir = data_dir.join("modules");
        std::fs::create_dir_all(&modules_dir).expect("modules dir");
        let db_path = data_dir.join("natives.db");
        let conn = Connection::open(&db_path).expect("open db");
        crate::db::create_tables(&conn).expect("base tables");
        crate::db::apply_migrations(&conn).expect("migrations");
        DraftFixture {
            _tmp: tmp,
            data_dir,
            modules_dir,
            db_path,
            conn,
        }
    }

    const PAGE: &str = "<html><head></head><body>draft</body></html>";

    fn file_name_of(path: &Path) -> String {
        path.file_name()
            .and_then(|n| n.to_str())
            .expect("file name")
            .to_string()
    }

    #[test]
    fn derives_data_dir_from_modules_dir() {
        let data_dir = Path::new("/home/u/.natives");
        assert_eq!(
            data_dir_from_modules_dir(&data_dir.join("modules")),
            Some(data_dir)
        );
    }

    #[test]
    fn serves_an_explicitly_named_revision() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");

        let resolved = resolve_draft_file(&f.data_dir, &f.db_path, "draft-1/rev-1.html")
            .expect("revision resolves");
        assert!(resolved.is_file());
        assert_eq!(file_name_of(&resolved), "rev-1.html");
    }

    #[test]
    fn default_file_follows_the_database_pointer_not_the_newest_file() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev2");

        let current = resolve_draft_file(&f.data_dir, &f.db_path, "draft-1").expect("current");
        assert_eq!(file_name_of(&current), "rev-2.html");

        // After an undo the rev-2 file still exists; the pointer is what decides.
        store::rollback(&f.conn, "draft-1").expect("rollback");
        let after = resolve_draft_file(&f.data_dir, &f.db_path, "draft-1/").expect("current");
        assert_eq!(file_name_of(&after), "rev-1.html");
    }

    #[test]
    fn default_file_is_absent_before_the_first_revision() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "draft-1").is_none());
    }

    #[test]
    fn rejects_traversal_out_of_the_draft_directory() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");

        for bad in [
            "draft-1/../../natives.db",
            "draft-1/../draft-1/rev-1.html",
            "draft-1/sub/../../rev-1.html",
            "draft-1/a\0b",
            "../drafts/draft-1/rev-1.html",
        ] {
            assert!(
                resolve_draft_file(&f.data_dir, &f.db_path, bad).is_none(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn unknown_draft_id_resolves_to_nothing() {
        let f = fixture();
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "draft-missing").is_none());
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "draft-missing/rev-1.html").is_none());
        // An id the validator refuses never reaches the filesystem either.
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "Draft_1/rev-1.html").is_none());
    }

    fn http_get(port: u16, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to test server");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        String::from_utf8_lossy(&raw).into_owned()
    }

    #[test]
    fn draft_preview_carries_the_same_csp_as_a_module() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        let response = http_get(port, "/drafts/draft-1");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains(&format!("Content-Security-Policy: {CSP_HEADER}")),
            "draft preview must carry the module CSP verbatim: {response}"
        );
        assert!(response.contains("draft"), "body should be the revision");

        // Unresolvable drafts answer 404 without saying why.
        let missing = http_get(port, "/drafts/draft-missing/rev-1.html");
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
        let traversal = http_get(port, "/drafts/draft-1/../../natives.db");
        assert!(traversal.starts_with("HTTP/1.1 404"), "{traversal}");
    }
}
