//! AI Tool Integration 公共辅助（backup、atomic patch、verify、rollback）。

use super::adapter::{ApplyRequest, EnvRef, IntegrationError};
use super::model::{
    BackupResult, DetectResult, InspectResult, PlannedPatch, RollbackResult, VerifyResult,
};
use std::path::{Path, PathBuf};

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn find_executable(names: &[&str], extra_paths: &[PathBuf]) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for &name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for dir in extra_paths {
        for &name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn detect_tool(names: &[&str], extra_paths: &[PathBuf]) -> DetectResult {
    if let Some(exe) = find_executable(names, extra_paths) {
        let version = std::process::Command::new(&exe)
            .arg("--version")
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    String::from_utf8(out.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            });
        DetectResult {
            installed: true,
            version,
            executable: Some(exe.to_string_lossy().to_string()),
        }
    } else {
        DetectResult {
            installed: false,
            version: None,
            executable: None,
        }
    }
}

pub fn inspect_config(config_path: &Path, sensitive_patterns: &[&str]) -> InspectResult {
    let exists = config_path.exists();
    let config_path_str = config_path.to_string_lossy().to_string();
    if !exists {
        return InspectResult {
            config_path: Some(config_path_str),
            exists: false,
            managed: false,
            sensitive_keys: Vec::new(),
            summary: "Config file not found".to_string(),
        };
    }

    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
    let managed = json.get("managedBy").and_then(|v| v.as_str()) == Some("AiNative")
        || json.get("ainative_managed").and_then(|v| v.as_bool()) == Some(true);

    let mut sensitive_keys = Vec::new();
    if let Some(obj) = json.as_object() {
        for (key, _) in obj {
            for &pattern in sensitive_patterns {
                if key.to_lowercase().contains(pattern) {
                    sensitive_keys.push(key.clone());
                }
            }
        }
    }

    let summary = format!("Config file exists ({} bytes)", content.len());
    InspectResult {
        config_path: Some(config_path_str),
        exists: true,
        managed,
        sensitive_keys,
        summary,
    }
}

pub fn backup_file(config_path: &Path) -> Result<BackupResult, IntegrationError> {
    if !config_path.exists() {
        return Ok(BackupResult {
            backup_path: None,
            created: false,
        });
    }

    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let backup_dir = parent.join("backups");
    if let Err(e) = std::fs::create_dir_all(&backup_dir) {
        return Err(IntegrationError::new(
            "backup_failed",
            format!("Failed to create backup dir: {e}"),
        ));
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let file_name = config_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let backup_path = backup_dir.join(format!("{file_name}.{timestamp}.bak"));

    if let Err(e) = std::fs::copy(config_path, &backup_path) {
        return Err(IntegrationError::new(
            "backup_failed",
            format!("Failed to copy backup: {e}"),
        ));
    }

    Ok(BackupResult {
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        created: true,
    })
}

pub fn plan_patch(
    config_path: &Path,
    desired_env: &[EnvRef],
    base_url_key: &str,
    api_key_key: &str,
    default_proxy_url: &str,
) -> Result<PlannedPatch, IntegrationError> {
    let mut base_json: serde_json::Map<String, serde_json::Value> = if config_path.exists() {
        let content = std::fs::read_to_string(config_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    base_json.insert("managedBy".to_string(), serde_json::json!("AiNative"));
    base_json.insert(
        base_url_key.to_string(),
        serde_json::json!(default_proxy_url),
    );

    let key_ref = desired_env
        .iter()
        .find(|e| e.key == api_key_key)
        .map(|e| e.source.clone())
        .unwrap_or_else(|| "{{secret_ref:default}}".to_string());
    base_json.insert(api_key_key.to_string(), serde_json::json!(key_ref));

    let patch_json = serde_json::to_string_pretty(&base_json).map_err(|e| {
        IntegrationError::new(
            "plan_failed",
            format!("Failed to serialize planned patch: {e}"),
        )
    })?;

    Ok(PlannedPatch {
        target: config_path.to_string_lossy().to_string(),
        patch_json,
        summary: format!("Configure proxy URL to {default_proxy_url} and set API key secret ref"),
    })
}

pub fn apply_patch(request: &ApplyRequest) -> Result<(), IntegrationError> {
    if !request.user_approved {
        return Err(IntegrationError::new(
            "approval_required",
            "User approval required before applying configuration change",
        ));
    }

    let target_path = Path::new(&request.patch.target);
    if let Some(parent) = target_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let temp_path = target_path.with_extension("tmp");
    if let Err(e) = std::fs::write(&temp_path, &request.patch.patch_json) {
        return Err(IntegrationError::new(
            "write_failed",
            format!("Failed to write temp file: {e}"),
        ));
    }

    if let Err(e) = std::fs::rename(&temp_path, target_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(IntegrationError::new(
            "rename_failed",
            format!("Failed to atomically rename config: {e}"),
        ));
    }

    Ok(())
}

pub fn verify_config(config_path: &Path) -> Result<VerifyResult, IntegrationError> {
    if !config_path.exists() {
        return Ok(VerifyResult {
            ok: false,
            checks: vec!["file_exists: false".into()],
            error: Some("Config file does not exist".into()),
        });
    }

    let content = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            return Ok(VerifyResult {
                ok: false,
                checks: vec!["file_read: error".into()],
                error: Some(format!("Failed to read config file: {e}")),
            });
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return Ok(VerifyResult {
                ok: false,
                checks: vec!["json_parse: error".into()],
                error: Some(format!("Invalid JSON syntax: {e}")),
            });
        }
    };

    let mut checks = vec!["file_exists: true".into(), "json_parse: ok".into()];
    if parsed.is_object() {
        checks.push("structure: valid_object".into());
    }

    Ok(VerifyResult {
        ok: true,
        checks,
        error: None,
    })
}

pub fn rollback_backup(
    config_path: &Path,
    backup_path_str: Option<&str>,
) -> Result<RollbackResult, IntegrationError> {
    let backup_path = if let Some(p) = backup_path_str {
        PathBuf::from(p)
    } else {
        let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
        let backup_dir = parent.join("backups");
        if !backup_dir.exists() {
            return Err(IntegrationError::new(
                "no_backup",
                "No backups directory found",
            ));
        }
        let entries = std::fs::read_dir(&backup_dir)
            .map_err(|e| IntegrationError::new("backup_read_error", e.to_string()))?;
        let mut backups: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|ent| ent.path()))
            .filter(|p| p.extension().and_then(|ext| ext.to_str()) == Some("bak"))
            .collect();
        backups.sort();
        backups
            .pop()
            .ok_or_else(|| IntegrationError::new("no_backup", "No backup files found"))?
    };

    if !backup_path.exists() {
        return Err(IntegrationError::new(
            "backup_not_found",
            format!("Backup file not found: {}", backup_path.display()),
        ));
    }

    std::fs::copy(&backup_path, config_path).map_err(|e| {
        IntegrationError::new("rollback_failed", format!("Failed to restore backup: {e}"))
    })?;

    Ok(RollbackResult {
        restored: true,
        backup_path: Some(backup_path.to_string_lossy().to_string()),
    })
}
