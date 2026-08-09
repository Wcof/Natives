//! Request validation / path-safety kernel: host & origin checks,
//! module path sanitization, and URL percent-decoding.

use super::*;
use std::path::{Path, PathBuf};

pub(crate) fn validate_host(host: &str) -> bool {
    // Strip port: "localhost:3001" -> "localhost", "[::1]:3001" -> "::1"
    let hostname = if host.starts_with('[') {
        // IPv6: [::1]:port
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or(host)
    };
    ALLOWED_HOSTS.contains(&hostname)
}

pub(crate) fn validate_origin(origin: &Option<String>, referer: &Option<String>) -> bool {
    match (origin, referer) {
        (Some(o), _) => is_loopback_url(o),
        (_, Some(r)) => is_loopback_url(r),
        _ => false, // POST without Origin or Referer = rejected
    }
}

pub(crate) fn is_loopback_url(url: &str) -> bool {
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

pub(crate) fn sanitize_path(
    module_id: &str,
    file_path: &str,
    modules_dir: &Path,
) -> Option<PathBuf> {
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

pub(crate) fn percent_decode(input: &str) -> String {
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

pub(crate) fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
