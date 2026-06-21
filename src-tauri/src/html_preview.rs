//! html_preview — Serve HTML files in a sandboxed iframe with local resource rewriting.
//!
//! Security model (matches fanbox server.js L1297-1359):
//! - sandbox="allow-scripts allow-forms" (NO allow-same-origin)
//! - Path mirroring: /fs/ prefix serves files from the HTML's parent directory
//! - Rewrites img src, href, poster, video src to use /fs/ proxy

use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// Result of preparing an HTML file for sandboxed preview.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlPreviewResult {
    pub content: String,
    pub fs_base: String,
    pub server_port: u16,
}

/// Prepare an HTML file for sandboxed preview.
pub fn prepare_html_preview(html_path: &str, server_port: u16) -> Result<HtmlPreviewResult> {
    let path = Path::new(html_path);
    if !path.exists() {
        return Err(Error::NotFound(format!("HTML file not found: {html_path}")));
    }

    let content = std::fs::read_to_string(path)?;
    let parent = path.parent().unwrap_or(Path::new("/"));
    let fs_base = parent.to_string_lossy().to_string();
    let rewritten = rewrite_local_paths(&content, server_port);

    Ok(HtmlPreviewResult {
        content: rewritten,
        fs_base,
        server_port,
    })
}

/// Rewrite local file references in HTML to use the /fs/ proxy endpoint.
fn rewrite_local_paths(html: &str, port: u16) -> String {
    let base_url = format!("http://localhost:{port}/fs/");
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
    !value.starts_with("http://")
        && !value.starts_with("https://")
        && !value.starts_with("data:")
        && !value.starts_with("javascript:")
        && !value.starts_with('#')
        && !value.starts_with('/')
        && !value.starts_with("blob:")
}

/// Validate that a requested file path is within the allowed preview directory.
#[allow(dead_code)]
pub fn is_path_allowed(requested_path: &str, allowed_base: &str) -> bool {
    let requested = match PathBuf::from(requested_path).canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let base = match PathBuf::from(allowed_base).canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    requested.starts_with(&base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_img_src() {
        let html = "<img src=\"photo.jpg\" alt=\"test\">";
        let result = rewrite_local_paths(html, 4321);
        assert!(result.contains("http://localhost:4321/fs/photo.jpg"));
    }

    #[test]
    fn test_skip_http_urls() {
        let html = "<img src=\"https://example.com/img.png\">";
        let result = rewrite_local_paths(html, 4321);
        assert!(result.contains("https://example.com/img.png"));
        assert!(!result.contains("localhost"));
    }

    #[test]
    fn test_skip_data_urls() {
        let html = "<img src=\"data:image/png;base64,abc123\">";
        let result = rewrite_local_paths(html, 4321);
        assert!(result.contains("data:image/png"));
    }

    #[test]
    fn test_path_allowed() {
        let tmp = std::env::temp_dir().join("natives-test-preview");
        std::fs::create_dir_all(&tmp).ok();
        let file = tmp.join("test.html");
        std::fs::write(&file, "test").ok();

        assert!(is_path_allowed(
            &file.to_string_lossy(),
            &tmp.to_string_lossy()
        ));
        assert!(!is_path_allowed("/etc/passwd", &tmp.to_string_lossy()));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
