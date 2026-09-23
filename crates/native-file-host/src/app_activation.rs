//! Activation projection and Native Messaging registration for built-in modules.

use crate::app_store::types::AppError;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn app_runtime_host_name() -> String {
    if crate::app_signing::is_production_build() {
        "com.natives.app_runtime".to_string()
    } else {
        "com.natives.local.app_runtime".to_string()
    }
}

/// 统一返回产品级 App Runtime Native Host 名称（ADR-0031）。
pub fn runtime_host_name(_app_id: &str) -> String {
    app_runtime_host_name()
}

/// 旧版每模块 Host 名称生成（仅用于升级时清理历史残留 manifest）。
pub fn legacy_runtime_host_name(app_id: &str) -> String {
    let digest = Sha256::digest(app_id.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{}.a{hex}", crate::app_signing::runtime_host_namespace())
}

pub fn make_executable(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// 注册统一的产品级 App Runtime Native Messaging Host（com.natives.app_runtime）。
pub fn register_app_runtime_host(
    executable: &Path,
    origin: &str,
    manifest_dir: &Path,
) -> Result<String, AppError> {
    let host = app_runtime_host_name();
    let manifest = serde_json::json!({
        "name": host,
        "description": "Natives official Built-in App Runtime",
        "path": executable.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [origin],
    });
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let script = format!("New-Item -Path 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\{host}' -Force | Out-Null; Set-ItemProperty -Path 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\{host}' -Name '(Default)' -Value '{}'", executable.display());
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x08000000)
            .status()?;
        if !status.success() {
            return Err(AppError::InvalidState(
                "APP_REGISTER_FAILED: Native Messaging registration rejected".into(),
            ));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::fs::create_dir_all(manifest_dir)?;
        let path = crate::app_host_manifest::manifest_path_in(manifest_dir, &host)?;
        let body = serde_json::to_vec_pretty(&manifest)
            .map_err(|_| AppError::InvalidState("manifest serialization failed".into()))?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, &body)?;
        std::fs::rename(&temp, &path)?;
    }
    Ok(host)
}

/// 清理历史旧版 per-app manifest 残留（如 com.natives.app.a<hash>.json 及 local 变体）
pub fn clean_legacy_runtime_hosts(manifest_dir: &Path, app_ids: &[&str]) {
    for app_id in app_ids {
        let legacy = legacy_runtime_host_name(app_id);
        if let Ok(path) = crate::app_host_manifest::manifest_path_in(manifest_dir, &legacy) {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
        let digest = Sha256::digest(app_id.as_bytes());
        let mut hex = String::with_capacity(64);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        for prefix in &["com.natives.app", "com.natives.local.app"] {
            let legacy_name = format!("{prefix}.a{hex}");
            if let Ok(path) = crate::app_host_manifest::manifest_path_in(manifest_dir, &legacy_name)
            {
                if path.exists() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

pub fn activation_path(apps_root: &Path, app_id: &str) -> PathBuf {
    apps_root.join(app_id).join("activation.json")
}

pub fn write_activation_projection(
    apps_root: &Path,
    app_id: &str,
    projection: &serde_json::Value,
) -> Result<(), AppError> {
    let path = activation_path(apps_root, app_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(parent)?.permissions();
            p.set_mode(0o700);
            std::fs::set_permissions(parent, p)?;
        }
    }
    let body = serde_json::to_vec_pretty(projection)
        .map_err(|_| AppError::InvalidState("activation projection serialization failed".into()))?;
    let temp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = file.metadata()?.permissions();
            p.set_mode(0o600);
            file.set_permissions(p)?;
        }
        file.write_all(&body)?;
        file.sync_all()?;
    }
    std::fs::rename(temp, path)?;
    Ok(())
}

pub fn read_activation_projection(
    apps_root: &Path,
    app_id: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let path = activation_path(apps_root, app_id);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(
        serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|e| AppError::InvalidState(format!("invalid activation.json: {e}")))?,
    ))
}

pub fn next_activation_generation(apps_root: &Path, app_id: &str) -> u64 {
    read_activation_projection(apps_root, app_id)
        .ok()
        .flatten()
        .and_then(|v| v.get("generation").and_then(serde_json::Value::as_u64))
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn update_activation_enabled(
    apps_root: &Path,
    app_id: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if let Some(mut value) = read_activation_projection(apps_root, app_id)? {
        value["generation"] = serde_json::json!(next_activation_generation(apps_root, app_id));
        value["enabled"] = serde_json::Value::Bool(enabled);
        value["activationState"] = serde_json::json!(if enabled { "ready" } else { "disabled" });
        write_activation_projection(apps_root, app_id, &value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_generation_is_monotonic() {
        let root = std::env::temp_dir().join(format!("natives-activation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_activation_projection(&root, "fund", &serde_json::json!({"generation": 1})).unwrap();
        assert_eq!(next_activation_generation(&root, "fund"), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clean_legacy_runtime_hosts_removes_old_manifests() {
        let temp =
            std::env::temp_dir().join(format!("natives-legacy-manifest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp);

        let digest = Sha256::digest(b"fund");
        let mut hex = String::with_capacity(64);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        let legacy_file = temp.join(format!("com.natives.app.a{hex}.json"));
        std::fs::write(&legacy_file, b"{}").unwrap();
        assert!(legacy_file.exists());

        clean_legacy_runtime_hosts(&temp, &["fund"]);
        assert!(!legacy_file.exists());

        let _ = std::fs::remove_dir_all(temp);
    }
}
