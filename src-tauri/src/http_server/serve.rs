//! Static / draft / preview / local-project file serving.

use super::*;
use crate::creative_draft::paths as draft_paths;
use crate::html_preview;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tiny_http::{Header, Request, Response};

pub(crate) fn serve_module_file(
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
                        .with_header(
                            Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap(),
                        );
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
pub(crate) fn data_dir_from_modules_dir(modules_dir: &Path) -> Option<&Path> {
    modules_dir.parent()
}

/// The current revision is a *pointer*, not "the highest file on disk": a
/// rollback moves the pointer back while keeping the newer revision files so the
/// undo can itself be undone. Only the database knows which one is current, so
/// the default file is resolved the same way `/local-projects/` resolves its
/// root — one short read on the connection this server already owns.
pub(crate) fn lookup_draft_current_revision(db_path: &Path, draft_id: &str) -> Option<i64> {
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
pub(crate) fn resolve_draft_file(
    data_dir: &Path,
    db_path: &Path,
    path_part: &str,
) -> Option<PathBuf> {
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
pub(crate) fn serve_draft_file(
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

/// Serve authorized HTML preview resources.
/// Route (PREV-001): `/fs/{token}/{relativePath}`
///
/// `token` is a preview session minted during `html_preview_prepare`, bound to
/// the HTML document's parent directory. Every request is re-authorized:
/// 1. the token must be an alive preview session;
/// 2. the relative path must resolve strictly inside that session's base dir
///    (`html_preview::resolve_within_base`, symlink-safe);
/// 3. the resolved file must pass the host allow/deny kernel again
///    (`file_manager::validate_path`) so a preview cannot reach blocklisted
///    paths (e.g. `~/.ssh`) that happen to live inside the base dir.
/// This is a revocable, per-resource authorization — not a raw-path proxy.
pub(crate) fn serve_preview_file(
    request: Request,
    csp: Header,
    head_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = request.url().to_string();
    let path_part = url.split('?').next().unwrap_or(&url);
    let path_part = path_part.strip_prefix("/fs/").unwrap_or(path_part);
    // percent-decode relative path segments carefully
    let path_part = percent_decode(path_part);
    let mut seg = path_part.splitn(2, '/');
    let token = seg.next().unwrap_or("");
    let rel = seg.next().unwrap_or("");

    // Shape / token sanity: unguessable hex token + an explicit relative path.
    if token.is_empty()
        || rel.is_empty()
        || !token.chars().all(|c| c.is_ascii_hexdigit())
        || token.contains("..")
        || rel.contains('\0')
    {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
    }

    let base_dir = match html_preview::resolve_preview_session(token) {
        Some(dir) => dir,
        // Unknown/expired/revoked session: answer uniformly so probing cannot
        // distinguish a dead session from a missing file.
        None => {
            let resp = Response::from_string("Not Found").with_status_code(404);
            request.respond(resp)?;
            return Ok(());
        }
    };

    let resolved = match html_preview::resolve_within_base(&base_dir, &rel) {
        Some(p) if p.is_file() => p,
        _ => {
            let resp = Response::from_string("Forbidden").with_status_code(403);
            request.respond(resp)?;
            return Ok(());
        }
    };

    // Defense-in-depth: the prepare step authorized the HTML document; here every
    // served sibling must pass the same kernel so blocklisted paths are unreachable.
    if crate::file_manager::validate_path(&resolved).is_err() {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
    }

    let mime = guess_mime(&resolved);
    if head_only {
        let len = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
        let resp = Response::empty(200)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", mime).unwrap())
            .with_header(
                Header::from_bytes("Content-Length", len.to_string().into_bytes())
                    .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap()),
            );
        request.respond(resp)?;
        return Ok(());
    }

    if mime == "text/html" {
        // Serve the (already rewritten) document raw so `previewUrl`-based
        // rendering works; no Bridge injection — HTML previews never receive a
        // Workshop Bridge token.
        let raw = std::fs::read_to_string(&resolved)?;
        let resp = Response::from_string(raw)
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
/// Route (CR-303): `/local-projects/{runtimeId}/{creativeId}/{relativePath}`
///
/// The runtime instance id is part of the URL and must be the app's ACTIVE
/// running instance: a stopped (or superseded) run's URL answers 410, so a
/// static preview URL is revocable and can never keep serving files after stop
/// (audit #03). The retired single-segment `/local-projects/{creativeId}/…`
/// URL (pre-CR-303) is not redirected: it answers 410 Gone like any other
/// unknown/dead path (MIG-004). No Workshop Bridge injection; no Tauri
/// capability.
pub(crate) fn serve_local_project_file(
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
    let mut seg = path_part.splitn(2, '/');
    let seg1 = seg.next().unwrap_or("");
    let rest = seg.next().unwrap_or("").to_string();
    if seg1.is_empty() || seg1.contains("..") || seg1.contains('\0') {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
    }

    // Tokenized shape: {runtimeId}/{creativeId}/{rel}
    let mut rest_seg = rest.splitn(2, '/');
    let creative_id = rest_seg.next().unwrap_or("").to_string();
    let rel = rest_seg.next().unwrap_or("").to_string();

    // seg1 must be an ACTIVE runtime instance of the named project.
    if !creative_id.is_empty() && !creative_id.contains("..") && !creative_id.contains('\0') {
        if let Some(project_root) = lookup_servable_runtime(db_path, seg1, &creative_id) {
            return serve_project_files(
                request,
                project_root,
                &format!("/local-projects/{seg1}/{creative_id}/"),
                &rel,
                &csp,
                head_only,
            );
        }
    }
    // Dead URL (stopped / superseded / unknown, including the retired
    // single-segment legacy shape): 410 Gone.
    let resp = Response::from_string("Gone").with_status_code(410);
    request.respond(resp)?;
    Ok(())
}

/// Serve one file under a project root with a tokenized `<base href>` so
/// relative subresource URLs resolve under `/local-projects/{runtimeId}/{creativeId}/`.
pub(crate) fn serve_project_files(
    request: Request,
    project_root: PathBuf,
    base_href: &str,
    rel: &str,
    csp: &Header,
    head_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut rel = rel.to_string();
    if rel.is_empty() || rel.ends_with('/') {
        rel = format!("{rel}index.html");
    }
    if rel.contains('\0') || rel.contains("..") {
        let resp = Response::from_string("Forbidden").with_status_code(403);
        request.respond(resp)?;
        return Ok(());
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
            .with_header(csp.clone())
            .with_header(Header::from_bytes("Content-Type", mime).unwrap())
            .with_header(
                Header::from_bytes("Content-Length", len.to_string().into_bytes())
                    .unwrap_or_else(|_| Header::from_bytes("x-placeholder", "x").unwrap()),
            );
        request.respond(resp)?;
        return Ok(());
    }

    // HTML: serve raw, no bridge injection, but anchor relative subresources to
    // the tokenized prefix so they resolve even though the URL carries a runtime id.
    if mime == "text/html" {
        let raw = std::fs::read_to_string(&file_canon)?;
        let body = inject_base_href(&raw, base_href);
        let resp = Response::from_string(body)
            .with_header(csp.clone())
            .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
        request.respond(resp)?;
    } else {
        let content = std::fs::read(&file_canon)?;
        let resp = Response::from_data(content)
            .with_header(csp.clone())
            .with_header(Header::from_bytes("Content-Type", mime).unwrap());
        request.respond(resp)?;
    }
    Ok(())
}

/// Tokenized route validation: the runtime instance must exist, be active
/// (running/starting), and belong to the named local project (CR-303).
pub(crate) fn lookup_servable_runtime(
    db_path: &Path,
    runtime_id: &str,
    creative_id: &str,
) -> Option<PathBuf> {
    let conn = Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT lc.canonical_project_root
         FROM runtime_instances ri
         JOIN applications a ON a.id = ri.application_id AND a.source = 'local_project'
         JOIN local_creative_apps lc ON lc.id = a.source_id
         WHERE ri.id = ?1 AND lc.id = ?2
           AND ri.status IN ('running','starting') AND lc.state = 'running'",
        rusqlite::params![runtime_id, creative_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .map(PathBuf::from)
}

/// Insert `<base href>` before `</head>` so relative subresources on a
/// tokenized local-project URL resolve under the correct prefix.
pub(crate) fn inject_base_href(html: &str, base: &str) -> String {
    let lower = html.to_lowercase();
    let Some(pos) = lower.find("</head>") else {
        return html.to_string();
    };
    let mut result = String::with_capacity(html.len() + base.len() + 16);
    result.push_str(&html[..pos]);
    result.push_str("<base href=\"");
    result.push_str(base);
    result.push_str("\">");
    result.push_str(&html[pos..]);
    result
}

pub(crate) fn resolve_under_project(root: &Path, rel: &str) -> Option<PathBuf> {
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
