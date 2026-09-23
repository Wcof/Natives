//! Validation and path helpers for the product's Native Messaging hosts.

use std::path::{Path, PathBuf};

/// V1 targets Chrome on macOS/Windows/Linux (ADR-0025 D16).
pub fn chrome_manifest_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = Path::new(&home);
    #[cfg(target_os = "macos")]
    {
        return Some(home.join("Library/Application Support/Google/Chrome/NativeMessagingHosts"));
    }
    #[cfg(target_os = "windows")]
    {
        return Some(home.join("AppData/Roaming/Google/Chrome/NativeMessagingHosts"));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Some(home.join(".config/google-chrome/NativeMessagingHosts"))
}

/// Host manifest path inside `dir` (`<host>.json`). Accepts both the
/// production namespace (`com.natives.app.`) and the local-candidate
/// namespace (`com.natives.local.app.`) so the two modes never share
/// registration files (plan §5 P1 mode isolation).
pub(crate) fn manifest_path_in(dir: &Path, host: &str) -> std::io::Result<PathBuf> {
    let in_namespace = host == "com.natives.app_runtime"
        || host == "com.natives.local.app_runtime"
        || host.starts_with("com.natives.app.")
        || host.starts_with("com.natives.local.app.");
    if !in_namespace
        || host.len() > 128
        || host.split('.').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid app host name",
        ));
    }
    let path = dir.join(format!("{host}.json"));
    crate::app_files::validate_app_path(dir, &path)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    Ok(path)
}

/// Chrome extension IDs encode the public-key hash with the letters a-p.
pub fn normalize_chrome_extension_origin(origin: &str) -> Option<String> {
    let rest = origin.strip_prefix("chrome-extension://")?;
    let id = rest.strip_suffix('/').unwrap_or(rest);
    (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .then(|| format!("chrome-extension://{id}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn origin_gate_accepts_real_chrome_ids() {
        for origin in [
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
        ] {
            assert!(
                normalize_chrome_extension_origin(origin).is_some(),
                "{origin}"
            );
        }
        for origin in [
            "chrome-extension://0123456789abcdef0123456789abcdef",
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/extra",
            "chrome-extension://ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP/",
        ] {
            assert!(
                normalize_chrome_extension_origin(origin).is_none(),
                "{origin}"
            );
        }
    }

    #[test]
    fn origin_gate_rejects_garbage() {
        assert!(normalize_chrome_extension_origin(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        )
        .is_some());
        assert!(normalize_chrome_extension_origin("http://localhost:3000").is_none());
        assert!(normalize_chrome_extension_origin("chrome-extension://short").is_none());
        assert!(normalize_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6/extra"
        )
        .is_none());
        assert!(normalize_chrome_extension_origin("").is_none());
        // 31 chars and 33 chars are both rejected
        assert!(normalize_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d"
        )
        .is_none());
        assert!(normalize_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d67"
        )
        .is_none());
    }

    #[test]
    fn manifest_host_name_cannot_escape_registration_directory() {
        let base =
            std::env::temp_dir().join(format!("natives-manifest-path-{}", std::process::id()));
        let dir = base.join("manifests");
        fs::create_dir_all(&dir).unwrap();
        let outside = base.join("outside.json");
        fs::write(&outside, b"preserved").unwrap();
        assert!(manifest_path_in(&dir, "../outside").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"preserved");
        fs::remove_dir_all(base).unwrap();
    }
}
