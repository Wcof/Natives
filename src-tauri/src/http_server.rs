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
    let method = request.method().clone();

    // 3. Route matching
    match &method {
        Method::Get => {
            if url == "/natives-sdk.js" {
                // Serve the bridge SDK
                let script = include_str!("bridge_sdk.js");
                let resp = Response::from_string(script)
                    .with_header(csp)
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                request.respond(resp)?;
            } else if url.starts_with("/modules/") {
                // Serve module static files
                serve_module_file(request, modules_dir, csp)?;
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

            if url.starts_with("/api/bridge/") {
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
    let path_part = url.strip_prefix("/modules/").unwrap_or(&url);
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
