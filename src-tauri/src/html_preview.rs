//! html_preview — Prepare HTML files for a sandboxed iframe with authorized
//! local-resource rewriting.
//!
//! Vertical chain (PREV-001): provider → PreviewContext.prepareHtml →
//! `html_preview_prepare` command (async, bounded IO slot + blocking pool) →
//! authorized `/fs/{token}/{path}` route on the local HTTP server.
//!
//! Security model:
//! - The HTML document itself is authorized through the host allow/deny kernel
//!   (`file_manager::validate_path`) before any read.
//! - Every local resource reference is rewritten to `/fs/{token}/{relative}`
//!   where `token` is a fresh unguessable preview session bound to the HTML's
//!   parent directory. The `/fs/` route re-resolves the session token, enforces
//!   containment inside that base directory, and re-runs the allow/deny kernel
//!   on each served file — no raw path is ever exposed to the renderer as a
//!   plain `/fs/` URL (PREV-001 / R-S5).
//! - Reads are bounded by a size budget (PERF-003): a file over
//!   `HTML_PREVIEW_MAX_BYTES` is rejected before reading, and the blocking read
//!   itself is executed on a blocking pool by the async command (R-P2 / R-B6).

use crate::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Size budget for a single HTML preview document (PERF-003). Files above this
/// limit are rejected before any read so the command path never loads an
/// unbounded "complete" HTML file. Aligned with `file_manager`'s full-read cap.
pub const HTML_PREVIEW_MAX_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

/// Preview session TTL: a `/fs/{token}/` URL is only servable while its session
/// is alive, so a static preview URL is revocable by expiry like local-project
/// run URLs are revocable by stop (CR-303 pattern).
const PREVIEW_SESSION_TTL: Duration = Duration::from_secs(10 * 60);

/// Cap on concurrent preview sessions (R-P9): the registry is bounded and evicts
/// the oldest session when the cap is reached.
const MAX_PREVIEW_SESSIONS: usize = 32;

/// Result of preparing an HTML file for sandboxed preview.
///
/// `content` already carries rewritten `/fs/{token}/…` absolute URLs (the token
/// stays server-side; the renderer renders via `srcDoc`). `fsBase` keeps the
/// frozen wire contract shape; it is the authorized base directory and is not
/// used for URL construction.
#[derive(serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HtmlPreviewResult {
    pub content: String,
    pub fs_base: String,
    pub server_port: u16,
}

struct PreviewSession {
    base_dir: PathBuf,
    created_at: Instant,
}

static PREVIEW_SESSIONS: OnceLock<Mutex<HashMap<String, PreviewSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, PreviewSession>> {
    PREVIEW_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_locked(map: &mut HashMap<String, PreviewSession>) {
    let now = Instant::now();
    map.retain(|_, s| now.duration_since(s.created_at) < PREVIEW_SESSION_TTL);
    if map.len() > MAX_PREVIEW_SESSIONS {
        // Evict the oldest sessions beyond the cap (deterministic LRU-ish).
        let mut by_age: Vec<(String, Instant)> =
            map.iter().map(|(k, s)| (k.clone(), s.created_at)).collect();
        by_age.sort_by_key(|(_, t)| *t);
        let excess = map.len() - MAX_PREVIEW_SESSIONS;
        for (token, _) in by_age.into_iter().take(excess) {
            map.remove(&token);
        }
    }
}

/// Register a preview authorization session bound to `base_dir`, returning a
/// fresh unguessable token. Callers must only pass already-authorized base dirs.
pub fn register_preview_session(base_dir: PathBuf) -> String {
    let token = generate_token();
    let mut map = sessions().lock().unwrap_or_else(|e| e.into_inner());
    prune_locked(&mut map);
    map.insert(
        token.clone(),
        PreviewSession {
            base_dir,
            created_at: Instant::now(),
        },
    );
    token
}

/// Resolve a preview token to its authorized base dir while the session is alive.
pub fn resolve_preview_session(token: &str) -> Option<PathBuf> {
    let mut map = sessions().lock().unwrap_or_else(|e| e.into_inner());
    prune_locked(&mut map);
    map.get(token).map(|s| s.base_dir.clone())
}

/// Remove a preview session (explicit revocation).
///
/// Currently exercised by tests; production surfaces rely on TTL expiry
/// (sessions die on their own). Kept as a `cfg(test)` API so tests can prove a
/// revoked session stops serving without pulling dead production code.
#[cfg(test)]
pub fn revoke_preview_session(token: &str) {
    let mut map = sessions().lock().unwrap_or_else(|e| e.into_inner());
    map.remove(token);
}

fn generate_token() -> String {
    let mut buf = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut buf);
    hex::encode(buf)
}

/// Prepare an HTML file for sandboxed preview.
///
/// Security (PREV-001 / P0-008): the file MUST be authorized before reading —
/// an arbitrary `html_path` must never reach the filesystem read. We run the
/// host allow/deny kernel (`file_manager::validate_path`, the same kernel
/// `FileAccessPolicy` delegates to) before touching the file, and we enforce the
/// size budget before any read.
///
/// This is the synchronous core; the async command wraps it in
/// `spawn_blocking` so the blocking read never runs on an async runtime thread
/// (R-B6) and is bounded by the IO semaphore (R-P9).
pub fn prepare_html_preview(html_path: &str, server_port: u16) -> Result<HtmlPreviewResult> {
    let path = Path::new(html_path);
    // Authorize first: reject out-of-scope / `..` / blocklisted paths before
    // any filesystem read (mirrors FileAccessPolicy semantics).
    crate::file_manager::validate_path(path)
        .map_err(|e| format!("html preview authorization failed for {html_path}: {e}"))?;

    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(Error::InvalidInput(
            "html preview target is not a file".into(),
        ));
    }
    if meta.len() > HTML_PREVIEW_MAX_BYTES {
        return Err(Error::InvalidInput(format!(
            "html preview exceeds size budget ({HTML_PREVIEW_MAX_BYTES} bytes)"
        )));
    }

    let content = std::fs::read_to_string(path)?;
    let parent = path
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .unwrap_or_else(|| path.parent().unwrap_or(Path::new("/")).to_path_buf());

    // Bind a preview session to the authorized parent dir; the returned token is
    // embedded in every rewritten /fs/ URL so the /fs/ route can re-authorize.
    let token = register_preview_session(parent.clone());
    let rewritten = rewrite_local_paths(&content, server_port, &token);

    Ok(HtmlPreviewResult {
        content: rewritten,
        fs_base: parent.to_string_lossy().to_string(),
        server_port,
    })
}

/// Resolve a relative path strictly inside `base_dir`, canonicalizing both sides
/// so symlink escapes are rejected. Returns `None` for absolute paths, `..`
/// components, non-existent targets, or targets outside the base.
pub fn resolve_within_base(base_dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() {
        return None; // require an explicit file; no directory listing
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return None;
    }
    for component in p.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return None, // ParentDir / RootDir / Prefix rejected
        }
    }
    let candidate = base_dir.join(p);
    let canon = std::fs::canonicalize(&candidate).ok()?;
    let base_canon = std::fs::canonicalize(base_dir).ok()?;
    if canon.starts_with(&base_canon) {
        Some(canon)
    } else {
        None
    }
}

/// Rewrite local file references in HTML to use the authorized `/fs/{token}/`
/// proxy endpoint. Rewrites `src`, `poster`, and `href` attribute values.
fn rewrite_local_paths(html: &str, port: u16, token: &str) -> String {
    let base_url = format!("http://localhost:{port}/fs/{token}/");
    let mut result = html.to_string();

    for attr in &["src", "poster", "href"] {
        let mut new_result = String::with_capacity(result.len());
        let mut pos = 0;

        while pos < result.len() {
            // Find attribute=
            let search = format!("{attr}=");
            if let Some(idx) = result[pos..].find(&search) {
                // Copy everything before the match
                new_result.push_str(&result[pos..pos + idx]);
                let match_start = pos + idx;
                let after_eq = match_start + search.len();

                // Skip whitespace after =
                let ws_end = result[after_eq..]
                    .find(|c: char| c != ' ')
                    .map(|i| after_eq + i)
                    .unwrap_or(after_eq);

                // Check for quote
                if ws_end < result.len() {
                    let quote_char = result.as_bytes()[ws_end];
                    if quote_char == b'"' || quote_char == b'\'' {
                        let q = quote_char as char;
                        // Find closing quote
                        if let Some(end) = result[ws_end + 1..].find(q) {
                            let value_start = ws_end + 1;
                            let value_end = ws_end + 1 + end;
                            let value = &result[value_start..value_end];

                            let new_value = if should_rewrite(value) {
                                format!("{base_url}{value}")
                            } else {
                                value.to_string()
                            };

                            new_result.push_str(&search);
                            new_result.push(q);
                            new_result.push_str(&new_value);
                            new_result.push(q);
                            pos = value_end + 1;
                            continue;
                        }
                    }
                }

                // No quote found or malformed — copy as-is
                new_result.push_str(&search);
                pos = match_start + search.len();
            } else {
                new_result.push_str(&result[pos..]);
                break;
            }
        }
        result = new_result;
    }

    result
}

fn should_rewrite(value: &str) -> bool {
    // Skip external/data/javascript URLs, fragments, protocol-relative and
    // filesystem-absolute references. Only bare relative references are proxied.
    let lower = value.to_lowercase();
    let explicit_skip = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("file:")
        || lower.starts_with("blob:")
        || lower.starts_with("about:");
    if explicit_skip || value.starts_with('#') || value.starts_with('/') || value.contains('\\') {
        return false;
    }
    // Any other scheme-like reference (`ftp:`, `mailto:`, `tel:`, …) is not a
    // local relative path — a ':' before any '/' means a URI scheme. Only a
    // colon that appears after a path segment (e.g. `sub/x:y.png`) is allowed.
    match value.find(':') {
        Some(colon) => value[..colon].contains('/'),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_img_src_binds_preview_token() {
        let html = "<img src=\"photo.jpg\" alt=\"test\">";
        let result = rewrite_local_paths(html, 4321, "abc123");
        assert!(result.contains("http://localhost:4321/fs/abc123/photo.jpg"));
    }

    #[test]
    fn test_rewrite_href_and_poster() {
        let html = "<a href=\"page.html\">x</a><video poster=\"poster.png\" src=\"v.mp4\"></video>";
        let result = rewrite_local_paths(html, 4321, "tok");
        assert!(result.contains("http://localhost:4321/fs/tok/page.html"));
        assert!(result.contains("http://localhost:4321/fs/tok/poster.png"));
        assert!(result.contains("http://localhost:4321/fs/tok/v.mp4"));
    }

    #[test]
    fn test_skip_external_and_absolute_urls() {
        let html = "<img src=\"https://example.com/img.png\"><img src=\"/abs/img.png\"><img src=\"data:image/png;base64,abc\"><a href=\"#anchor\">a</a><img src=\"file:///etc/passwd\"><img src=\"ftp://x/y.png\"><img src=\"sub/x:y.png\">";
        let result = rewrite_local_paths(html, 4321, "tok");
        assert!(result.contains("https://example.com/img.png"));
        assert!(result.contains("/abs/img.png"));
        assert!(result.contains("data:image/png"));
        assert!(result.contains("#anchor"));
        assert!(result.contains("file:///etc/passwd"));
        assert!(result.contains("ftp://x/y.png"));
        // A relative path with a colon inside a segment is still a local ref.
        assert!(result.contains("http://localhost:4321/fs/tok/sub/x:y.png"));
        assert!(
            !result.contains("/fs/tok/ftp://x"),
            "scheme-like ref must not be proxied"
        );
        assert!(!result.contains("/fs/tok/https://example.com"));
    }

    #[test]
    fn test_resolve_within_base_accepts_siblings_and_subdirs() {
        let tmp = std::env::temp_dir().join(format!(
            "natives-html-preview-ok-base-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = tmp.join("sub");
        std::fs::create_dir_all(&sub).expect("create dirs");
        std::fs::write(tmp.join("a.png"), "png").ok();
        std::fs::write(sub.join("b.css"), "css").ok();

        let resolved = resolve_within_base(&tmp, "a.png").expect("sibling resolves");
        assert_eq!(resolved.file_name().and_then(|n| n.to_str()), Some("a.png"));
        let resolved_sub = resolve_within_base(&tmp, "sub/b.css").expect("subdir resolves");
        assert_eq!(
            resolved_sub.file_name().and_then(|n| n.to_str()),
            Some("b.css")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_within_base_rejects_traversal_and_escape() {
        let tmp = std::env::temp_dir().join(format!(
            "natives-html-preview-reject-base-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).expect("create dir");
        for bad in [
            "",
            "../etc/passwd",
            "sub/../../etc/passwd",
            "/etc/passwd",
            "a\0b",
            "missing.txt",
        ] {
            assert!(
                resolve_within_base(&tmp, bad).is_none(),
                "expected {bad:?} to be rejected"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_preview_session_register_resolve_revoke() {
        let base = PathBuf::from("/tmp/preview-base");
        let token = register_preview_session(base.clone());
        assert_eq!(resolve_preview_session(&token), Some(base));
        revoke_preview_session(&token);
        assert_eq!(resolve_preview_session(&token), None);
        assert_eq!(resolve_preview_session("unknown-token"), None);
    }

    #[test]
    fn test_prepare_html_preview_rejects_oversized_file() {
        let tmp = std::env::temp_dir().join(format!(
            "natives-html-preview-big-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).expect("create dir");
        let file = tmp.join("big.html");
        let big = "a".repeat((HTML_PREVIEW_MAX_BYTES + 1) as usize);
        std::fs::write(&file, big).expect("write big file");

        let result = prepare_html_preview(&file.to_string_lossy(), 4321);
        assert!(result.is_err(), "oversized html must be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("size budget"), "unexpected error: {err}");
        assert!(
            !err.contains(file.to_string_lossy().as_ref()),
            "no path in error"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_prepare_html_preview_rewrites_against_tokenized_fs_url() {
        let tmp = std::env::temp_dir().join(format!(
            "natives-html-preview-ok-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).expect("create dir");
        let file = tmp.join("page.html");
        std::fs::write(&file, "<img src=\"x.png\">").expect("write html");

        let result = prepare_html_preview(&file.to_string_lossy(), 4321);
        assert!(result.is_ok(), "prepare failed: {:?}", result.err());
        let prepared = result.unwrap();
        // The rewritten URL must carry a session token that resolves to the base dir.
        let fs_url = prepared.content;
        let prefix = "http://localhost:4321/fs/";
        assert!(
            fs_url.contains(prefix),
            "expected tokenized /fs/ URL: {fs_url}"
        );
        let token = fs_url
            .split(prefix)
            .nth(1)
            .and_then(|s| s.split('/').next())
            .unwrap_or("");
        assert!(!token.is_empty(), "token missing from rewritten URL");
        assert!(
            !token.contains('"'),
            "token must not include trailing markup"
        );
        // macOS /var → /private/var symlink: compare against the canonical base.
        let base_canon = std::fs::canonicalize(&tmp).expect("canonicalize base");
        assert_eq!(resolve_preview_session(token), Some(base_canon));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
