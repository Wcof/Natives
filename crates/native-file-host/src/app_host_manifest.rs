//! Native Messaging Host manifest writer (ADR-0025 D16/D18).
//!
//! The ONLY sanctioned location outside `~/.natives/`: the browser's
//! Native Messaging Host Manifest directory. The manifest may contain
//! exactly `name` / `description` / `path` / `type` / `allowed_origins` —
//! never a token, secret, account, user wealth data, or configuration.
//!
//! `allowed_origins` MUST be the real caller origin Chrome provides when
//! it spawns the Host (the extension origin). A missing or non
//! `chrome-extension://` origin makes registration SKIP — V1 never
//! fabricates origins, and an unregistered host is an explicit state
//! (`host_registered = 0`), not a silent fake registration.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// V1 targets Chrome on macOS/Windows/Linux (ADR-0025 D16).
pub fn chrome_manifest_dir() -> Option<PathBuf> {
    // Test seam: tests point the manifest dir at a per-process temp dir
    // (set once, before any test that writes manifests) so unit tests
    // never touch the real browser directory.
    if let Some(dir) = std::env::var_os("NATIVES_NM_HOSTS_DIR") {
        return Some(PathBuf::from(dir));
    }
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

/// Host manifest path inside `dir` (`<host>.json`).
fn manifest_path_in(dir: &Path, host: &str) -> PathBuf {
    dir.join(format!("{host}.json"))
}

/// Write the manifest atomically (temp + rename) inside `dir`.
pub fn write_manifest_in(
    dir: &Path,
    host: &str,
    binary: &Path,
    origin: &str,
) -> std::io::Result<PathBuf> {
    if !is_chrome_extension_origin(origin) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "origin must be a chrome-extension:// origin",
        ));
    }
    fs::create_dir_all(dir)?;
    let path = manifest_path_in(dir, host);
    let manifest = json!({
        "name": host,
        "description": "Natives app runtime host",
        "path": binary.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [origin],
    });
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&manifest)?)?;
    fs::rename(&temp, &path)?;
    Ok(path)
}

/// Write into the browser's real manifest directory (production path).
pub fn write_manifest(host: &str, binary: &Path, origin: &str) -> std::io::Result<PathBuf> {
    let dir = chrome_manifest_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory"))?;
    write_manifest_in(&dir, host, binary, origin)
}

/// Delete a previously written manifest. Idempotent.
pub fn remove_manifest(host: &str) -> std::io::Result<()> {
    if let Some(dir) = chrome_manifest_dir() {
        let path = manifest_path_in(&dir, host);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Delete inside an explicit dir (tests / future multi-browser support).
pub fn remove_manifest_in(dir: &Path, host: &str) -> std::io::Result<()> {
    let path = manifest_path_in(dir, host);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Origin gate: exactly `chrome-extension://<32 hex chars>`.
pub fn is_chrome_extension_origin(origin: &str) -> bool {
    let rest = origin.strip_prefix("chrome-extension://").unwrap_or("");
    rest.len() == 32 && rest.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_gate_rejects_garbage() {
        assert!(is_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        ));
        assert!(!is_chrome_extension_origin("http://localhost:3000"));
        assert!(!is_chrome_extension_origin("chrome-extension://short"));
        assert!(!is_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6/extra"
        ));
        assert!(!is_chrome_extension_origin(""));
        // 31 chars and 33 chars are both rejected
        assert!(!is_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d"
        ));
        assert!(!is_chrome_extension_origin(
            "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d67"
        ));
    }

    #[test]
    fn manifest_round_trip_injected_dir() {
        let dir = std::env::temp_dir().join(format!(
            "natives-host-manifest-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let bin = dir.join("host-bin");
        let origin = "chrome-extension://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
        let written =
            write_manifest_in(&dir, "com.natives.app.test", &bin, origin).expect("write manifest");
        let parsed: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&written).unwrap()).unwrap();
        assert_eq!(parsed["name"], "com.natives.app.test");
        assert_eq!(parsed["type"], "stdio");
        assert_eq!(parsed["allowed_origins"][0], origin);
        assert_eq!(parsed["path"], bin.to_string_lossy().into_owned());
        // no forbidden keys
        for key in ["token", "secret", "api_key", "account"] {
            assert!(parsed.get(key).is_none(), "manifest must not carry {key}");
        }
        // overwrite is atomic and idempotent-safe
        write_manifest_in(&dir, "com.natives.app.test", &bin, origin).unwrap();
        // bad origin rejected
        assert!(write_manifest_in(&dir, "x", &bin, "http://localhost").is_err());
        remove_manifest_in(&dir, "com.natives.app.test").unwrap();
        assert!(!written.exists());
        // remove is idempotent
        remove_manifest_in(&dir, "com.natives.app.test").unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
