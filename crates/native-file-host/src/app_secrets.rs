//! Delete only the service namespace declared by this App. No plaintext is read.

use crate::app_store::types::AppError;

pub(crate) fn purge_namespace(namespace: &str) -> Result<(), AppError> {
    crate::app_host_manifest::manifest_path_in(std::path::Path::new("."), namespace)?;
    purge(namespace)
}

#[cfg(target_os = "macos")]
fn purge(namespace: &str) -> Result<(), AppError> {
    for _ in 0..256 {
        let status = run_bounded(
            "/usr/bin/security",
            &["delete-generic-password", "-s", namespace],
        )?;
        match status.code() {
            Some(0) => continue,
            Some(44) => return Ok(()), // errSecItemNotFound
            _ => {
                return Err(AppError::InvalidState(
                    "APP_KEYCHAIN_LOCKED: keychain cleanup failed; unlock and retry".into(),
                ))
            }
        }
    }
    Err(AppError::InvalidState(
        "keychain cleanup batch limit reached; retry".into(),
    ))
}

#[cfg(target_os = "linux")]
fn purge(namespace: &str) -> Result<(), AppError> {
    purge_secret_service(namespace, bus_call)
}

#[cfg(any(target_os = "linux", test))]
fn purge_secret_service(
    namespace: &str,
    mut call: impl FnMut(&[&str]) -> Result<serde_json::Value, AppError>,
) -> Result<(), AppError> {
    let query = [
        "/org/freedesktop/secrets",
        "org.freedesktop.Secret.Service",
        "SearchItems",
        "a{ss}",
        "1",
        "service",
        namespace,
    ];
    let items = secret_service_items(call(&query)?)?;
    if items.len() > 256 {
        return Err(AppError::InvalidState(
            "secret cleanup batch limit exceeded".into(),
        ));
    }
    for path in items {
        let deleted = call(&[&path, "org.freedesktop.Secret.Item", "Delete"])?;
        if deleted["type"] != "o" || deleted["data"] != serde_json::json!(["/"]) {
            return Err(AppError::InvalidState(
                "APP_KEYCHAIN_LOCKED: secret deletion needs unlocking".into(),
            ));
        }
    }
    if !secret_service_items(call(&query)?)?.is_empty() {
        return Err(AppError::InvalidState(
            "secret cleanup incomplete; retry".into(),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn secret_service_items(value: serde_json::Value) -> Result<Vec<String>, AppError> {
    let invalid = || AppError::InvalidState("invalid secret service metadata".into());
    if value["type"] != "aoao" {
        return Err(invalid());
    }
    let data = value["data"]
        .as_array()
        .filter(|data| data.len() == 2)
        .ok_or_else(invalid)?;
    let unlocked = data[0].as_array().ok_or_else(invalid)?;
    let locked = data[1].as_array().ok_or_else(invalid)?;
    if !locked.is_empty() {
        return Err(AppError::InvalidState(
            "APP_KEYCHAIN_LOCKED: unlock the app credentials and retry".into(),
        ));
    }
    unlocked
        .iter()
        .map(|item| {
            let path = item.as_str().ok_or_else(invalid)?;
            if !path.starts_with("/org/freedesktop/secrets/collection/")
                || path.len() > 1024
                || !path
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'/')
            {
                return Err(invalid());
            }
            Ok(path.to_owned())
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn bus_call(args: &[&str]) -> Result<serde_json::Value, AppError> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new("busctl")
        .args([
            "--user",
            "--json=short",
            "--timeout=2",
            "call",
            "org.freedesktop.secrets",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::InvalidState("secret metadata output unavailable".into()))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(65537).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let result = (|| {
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err(AppError::InvalidState("secret service unavailable".into()));
                }
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(AppError::InvalidState("secret service timed out".into()));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let bytes = receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .map_err(|_| AppError::InvalidState("secret metadata timed out".into()))??;
        if bytes.len() > 65536 {
            return Err(AppError::InvalidState("secret metadata too large".into()));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| AppError::InvalidState("invalid secret metadata".into()))
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(target_os = "windows")]
fn purge(namespace: &str) -> Result<(), AppError> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::*,
    };
    let filter: Vec<u16> = format!("{namespace}:*\0").encode_utf16().collect();
    let mut count = 0;
    let mut entries = std::ptr::null_mut();
    // Credential Manager returns only targets within the app's fixed namespace.
    unsafe {
        if CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut entries) == 0 {
            return if GetLastError() == ERROR_NOT_FOUND {
                Ok(())
            } else {
                Err(AppError::InvalidState(
                    "credential cleanup unavailable".into(),
                ))
            };
        }
        let mut result = Ok(());
        for credential in std::slice::from_raw_parts(entries, count as usize) {
            if CredDeleteW((**credential).TargetName, (**credential).Type, 0) == 0
                && GetLastError() != ERROR_NOT_FOUND
            {
                result = Err(AppError::InvalidState("credential cleanup failed".into()));
                break;
            }
        }
        CredFree(entries.cast());
        result
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn purge(_: &str) -> Result<(), AppError> {
    Err(AppError::InvalidState(
        "keychain cleanup unsupported on this platform".into(),
    ))
}

#[cfg(target_os = "macos")]
fn run_bounded(program: &str, args: &[&str]) -> Result<std::process::ExitStatus, AppError> {
    use std::process::Stdio;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::InvalidState("keychain cleanup timed out".into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secret_cleanup_is_idempotent_without_reading_plaintext() {
        let mut calls = Vec::new();
        purge_secret_service("com.natives.app.demo", |args| {
            calls.push(
                args.iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>(),
            );
            Ok(json!({"type":"aoao","data":[[],[]]}))
        })
        .unwrap();
        assert!(calls
            .iter()
            .all(|call| call[2] == "SearchItems" && call[6] == "com.natives.app.demo"));
    }

    #[test]
    fn secret_cleanup_rejects_locked_and_pending_deletions() {
        assert!(purge_secret_service("com.natives.app.demo", |_| Ok(json!({
            "type":"aoao","data":[[],["/org/freedesktop/secrets/collection/login/1"]]
        })))
        .is_err());
        let mut calls = 0;
        assert!(purge_secret_service("com.natives.app.demo", |args| {
            calls += 1;
            if args[2] == "SearchItems" {
                Ok(json!({"type":"aoao","data":[["/org/freedesktop/secrets/collection/login/1"],[]]}))
            } else { Ok(json!({"type":"o","data":["/org/freedesktop/secrets/prompt/1"]})) }
        }).is_err());
        assert_eq!(calls, 2);
    }
}
