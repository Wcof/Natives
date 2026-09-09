//! Validation and removal helpers for legacy child Host registrations.

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

/// Delete inside an explicit dir (tests / future multi-browser support).
pub fn remove_manifest_in(dir: &Path, host: &str) -> std::io::Result<()> {
    let path = manifest_path_in(dir, host)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Windows discovers Native Hosts through HKCU; macOS/Linux use the JSON file.
pub(crate) fn remove_registration(dir: &Path, host: &str) -> std::io::Result<()> {
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
    }
    let _ = path;
    Ok(())
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
        assert!(remove_manifest_in(&dir, "../outside").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"preserved");
        fs::remove_dir_all(base).unwrap();
    }
}
