//! Generic external tool detect / launch + lsof/OSC path decode helpers
//! (W3 split from terminal.rs).

use crate::{Error, Result};

/// Decode an lsof `\xNN`-escaped path into a plain path.
pub fn decode_lsof_path(path: &str) -> String {
    if !path.contains("\\x") {
        return path.to_string();
    }

    let mut bytes = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 3 < chars.len() && chars[i] == '\\' && chars[i + 1] == 'x' {
            let hex_str: String = chars[i + 2..i + 4].iter().collect();
            if let Ok(byte_val) = u8::from_str_radix(&hex_str, 16) {
                bytes.push(byte_val);
                i += 4;
                continue;
            }
        }
        let mut buf = [0; 4];
        let char_str = chars[i].encode_utf8(&mut buf);
        bytes.extend_from_slice(char_str.as_bytes());
        i += 1;
    }

    String::from_utf8(bytes).unwrap_or_else(|_| path.to_string())
}

/// 简单的 URL 解码（OSC 7 路径可能包含 %20 等编码）
pub fn url_decode_path(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Detect whether an external binary is installed.
/// Checks `extra_paths` first, then falls back to `which $name`.
pub fn detect_binary(name: &str, extra_paths: &[&str]) -> bool {
    // Check extra paths first (e.g. macOS .app bundles)
    for path in extra_paths {
        if std::path::Path::new(path).exists() {
            return true;
        }
    }
    // Fallback: `which` / `where`
    let output = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(name)
        .output();
    if let Ok(out) = output {
        if out.status.success() && !out.stdout.is_empty() {
            return true;
        }
    }
    false
}

/// Launch an external binary. Searches `extra_paths` first, then `which $name`.
pub fn launch_binary(name: &str, extra_paths: &[&str]) -> Result<()> {
    // Find the actual path
    let mut resolved_path: Option<String> = None;
    for path in extra_paths {
        if std::path::Path::new(path).exists() {
            resolved_path = Some(path.to_string());
            break;
        }
    }
    if resolved_path.is_none() {
        let output = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
            .arg(name)
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    resolved_path = Some(first_line.to_string());
                }
            }
        }
    }

    let bin_path = resolved_path.ok_or_else(|| Error::NotFound(format!("{name} not found")))?;

    std::process::Command::new(&bin_path)
        .spawn()
        .map_err(|e| Error::Internal(format!("failed to launch {bin_path}: {e}")))?;

    Ok(())
}
