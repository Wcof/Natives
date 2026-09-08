//! Native Messaging Host manifest writer (ADR-0025 D16/D18).
//!
//! The ONLY sanctioned location outside `~/.natives/`: the browser's
//! Native Messaging Host Manifest directory. The manifest may contain
//! exactly `name` / `description` / `path` / `type` / `allowed_origins` —
//! never a token, secret, account, user wealth data, or configuration.
//!
//! `allowed_origins` MUST be the real caller origin Chrome provides when
//! it spawns the Host (the extension origin). A missing or non
//! `chrome-extension://` origin rejects installation. Legacy unregistered
//! rows remain explicit (`host_registered = 0`) and cannot open a runtime.

use serde_json::json;
use std::fs;
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

/// Host manifest path inside `dir` (`<host>.json`).
pub(crate) fn manifest_path_in(dir: &Path, host: &str) -> std::io::Result<PathBuf> {
    if !host.starts_with("com.natives.app.")
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
    crate::app_install::validate_app_path(dir, &path)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    Ok(path)
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
    let path = manifest_path_in(dir, host)?;
    let manifest = json!({
        "name": host,
        "description": "Natives app runtime host",
        "path": binary.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [normalize_chrome_extension_origin(origin)],
    });
    crate::app_install::atomic_write(&path, &serde_json::to_vec_pretty(&manifest)?)
        .map_err(std::io::Error::other)?;
    Ok(path)
}

/// Delete inside an explicit dir (tests / future multi-browser support).
pub fn remove_manifest_in(dir: &Path, host: &str) -> std::io::Result<()> {
    let path = manifest_path_in(dir, host)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Windows discovers Native Hosts through HKCU; macOS/Linux use the JSON file.
pub(crate) fn sync_registration(dir: &Path, host: &str, present: bool) -> std::io::Result<()> {
    let path = manifest_path_in(dir, host)?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND},
            System::Registry::*,
        };
        let subkey: Vec<u16> = format!("Software\\Google\\Chrome\\NativeMessagingHosts\\{host}\0")
            .encode_utf16()
            .collect();
        unsafe {
            if !present {
                let status = RegDeleteTreeW(HKEY_CURRENT_USER, subkey.as_ptr());
                return if status == 0
                    || status == ERROR_FILE_NOT_FOUND
                    || status == ERROR_PATH_NOT_FOUND
                {
                    Ok(())
                } else {
                    Err(std::io::Error::from_raw_os_error(status as i32))
                };
            }
            let mut key = std::ptr::null_mut();
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            );
            if status != 0 {
                return Err(std::io::Error::from_raw_os_error(status as i32));
            }
            let value: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let status = RegSetValueExW(
                key,
                std::ptr::null(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            );
            RegCloseKey(key);
            if status != 0 {
                return Err(std::io::Error::from_raw_os_error(status as i32));
            }
        }
    }
    let _ = (path, present);
    Ok(())
}

/// Chrome extension IDs encode the public-key hash with the letters a-p.
pub fn normalize_chrome_extension_origin(origin: &str) -> Option<String> {
    let rest = origin.strip_prefix("chrome-extension://")?;
    let id = rest.strip_suffix('/').unwrap_or(rest);
    (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .then(|| format!("chrome-extension://{id}/"))
}

pub fn is_chrome_extension_origin(origin: &str) -> bool {
    normalize_chrome_extension_origin(origin).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_gate_accepts_real_chrome_ids() {
        for origin in [
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
        ] {
            assert!(is_chrome_extension_origin(origin), "{origin}");
        }
        for origin in [
            "chrome-extension://0123456789abcdef0123456789abcdef",
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/extra",
            "chrome-extension://ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP/",
        ] {
            assert!(!is_chrome_extension_origin(origin), "{origin}");
        }
    }

    #[test]
    fn origin_gate_rejects_garbage() {
        assert!(is_chrome_extension_origin(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
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
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
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

    #[test]
    fn manifest_host_name_cannot_escape_registration_directory() {
        let base =
            std::env::temp_dir().join(format!("natives-manifest-path-{}", std::process::id()));
        let dir = base.join("manifests");
        fs::create_dir_all(&dir).unwrap();
        let outside = base.join("outside.json");
        fs::write(&outside, b"preserved").unwrap();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        assert!(write_manifest_in(&dir, "../outside", &base.join("host"), origin).is_err());
        assert!(remove_manifest_in(&dir, "../outside").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"preserved");
        fs::remove_dir_all(base).unwrap();
    }
}
