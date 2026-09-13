//! Activation projection verification for managed app host (contract §3.1).
//!
//! Core App Store is the install authority; activation.json is the atomic
//! projection Core writes at install/enable/disable/uninstall.
//! The App Host reads this projection before starting to verify that Core has
//! enabled this app, the version matches, and the caller's origin is allowed.
//! Strict validation: missing identity, malformed types, empty allowedOrigins,
//! missing caller origin, unready state, or mismatched hashes reject startup.

use crate::error::AppErrorCode;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRecord {
    pub receipt_version: u32,
    pub app_id: String,
    pub runtime_host: String,
    pub active_version: String,
    pub generation: u64,
    pub activation_state: String,
    pub enabled: bool,
    pub app_protocol_version: u32,
    pub payload_sha256: Option<String>,
    pub allowed_origins: Vec<String>,
    pub catalog_evidence_sha256: Option<String>,
}

/// Compute and compare the SHA-256 of an executable file against an expected hash.
pub fn verify_executable_sha256(
    exe_path: &Path,
    expected_sha256: &str,
) -> Result<(), (AppErrorCode, String)> {
    let bytes = std::fs::read(exe_path).map_err(|e| {
        (
            AppErrorCode::StartFailed,
            format!("failed to read executable for verification: {e}"),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash = format!("{:x}", hasher.finalize());
    if !hash.eq_ignore_ascii_case(expected_sha256) {
        return Err((
            AppErrorCode::PackageInvalid,
            format!(
                "executable SHA-256 '{hash}' does not match activation payloadSha256 '{expected_sha256}'"
            ),
        ));
    }
    Ok(())
}

/// 严格校验激活投影，支持校验期望的 generation。
pub fn verify_activation_with_generation(
    apps_root: &Path,
    expected_app_id: &str,
    current_version: &str,
    caller_origin: Option<&str>,
    expected_generation: Option<u64>,
) -> Result<ActivationRecord, (AppErrorCode, String)> {
    let norm_app_id = crate::lock::normalize_app_id(expected_app_id);
    let path = apps_root.join(norm_app_id).join("activation.json");
    if !path.exists() {
        return Err((
            AppErrorCode::InstallationChanged,
            "activation.json projection not found".into(),
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| {
        (
            AppErrorCode::StartFailed,
            format!("failed to read activation.json: {e}"),
        )
    })?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
        (
            AppErrorCode::StartFailed,
            format!("failed to parse activation.json: {e}"),
        )
    })?;

    // 1. receiptVersion: 必填 u32，必须为 1
    let receipt_version = val
        .get("receiptVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "receiptVersion is missing or invalid".into(),
            )
        })? as u32;
    if receipt_version != 1 {
        return Err((
            AppErrorCode::Incompatible,
            format!("unsupported receiptVersion: {receipt_version}"),
        ));
    }

    // 2. appId: 必填非空字符串，归一化后必须与 expected_app_id 一致（禁止使用默认值冒充身份）
    let app_id = val.get("appId").and_then(|v| v.as_str()).ok_or_else(|| {
        (
            AppErrorCode::StartFailed,
            "appId is missing or invalid".into(),
        )
    })?;
    if app_id.is_empty() {
        return Err((AppErrorCode::StartFailed, "appId must not be empty".into()));
    }
    if crate::lock::normalize_app_id(app_id) != norm_app_id {
        return Err((
            AppErrorCode::ProtocolMismatch,
            format!("appId mismatch: expected '{expected_app_id}', got '{app_id}'"),
        ));
    }

    // 3. runtimeHost: 必填非空字符串
    let runtime_host = val
        .get("runtimeHost")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "runtimeHost is missing or invalid".into(),
            )
        })?;
    if runtime_host.is_empty() {
        return Err((
            AppErrorCode::StartFailed,
            "runtimeHost must not be empty".into(),
        ));
    }

    // 4. activeVersion: 必填字符串，必须与当前版本候选精确一致
    let active_version = val
        .get("activeVersion")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "activeVersion is missing or invalid".into(),
            )
        })?;
    if active_version != current_version {
        return Err((
            AppErrorCode::InstallationChanged,
            format!(
                "activeVersion '{active_version}' does not match candidate '{current_version}'"
            ),
        ));
    }

    // 5. generation: 必填严格正整数（> 0 单调计数器，严禁缺省为 1）
    let generation = val
        .get("generation")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "generation is missing or invalid".into(),
            )
        })?;
    if generation == 0 {
        return Err((AppErrorCode::StartFailed, "generation must be > 0".into()));
    }
    if let Some(expected_gen) = expected_generation {
        if generation != expected_gen {
            return Err((
                AppErrorCode::InstallationChanged,
                format!(
                    "activation generation {generation} does not match expected {expected_gen}"
                ),
            ));
        }
    }

    // 6. enabled: 必填布尔值，必须为 true
    let enabled = val
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "enabled must be a boolean".into(),
            )
        })?;
    if !enabled {
        return Err((
            AppErrorCode::InstallationChanged,
            "app is disabled by Core".into(),
        ));
    }

    // 7. activationState: 必填字符串，必须严格为 ready
    let state = val
        .get("activationState")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "activationState is missing or invalid".into(),
            )
        })?;
    if state != "ready" {
        return Err((
            AppErrorCode::InstallationChanged,
            format!("app activation state is '{state}'"),
        ));
    }

    // 8. appProtocolVersion: 必填整数，必须等于 APP_PROTOCOL_VERSION (1)
    let app_protocol_version = val
        .get("appProtocolVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "appProtocolVersion is missing or invalid".into(),
            )
        })? as u32;
    if app_protocol_version != crate::protocol::APP_PROTOCOL_VERSION {
        return Err((
            AppErrorCode::ProtocolMismatch,
            format!(
                "appProtocolVersion {app_protocol_version} does not match expected {}",
                crate::protocol::APP_PROTOCOL_VERSION
            ),
        ));
    }

    // 9. allowedOrigins: 必填非空数组，元素均为有效非空字符串
    let allowed_origins_val = val
        .get("allowedOrigins")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "allowedOrigins must be an array of strings".into(),
            )
        })?;
    if allowed_origins_val.is_empty() {
        return Err((
            AppErrorCode::SessionInvalid,
            "allowedOrigins must not be empty".into(),
        ));
    }
    let mut allowed_origins = Vec::with_capacity(allowed_origins_val.len());
    for item in allowed_origins_val {
        let s = item.as_str().ok_or_else(|| {
            (
                AppErrorCode::SessionInvalid,
                "allowedOrigins entry must be a string".into(),
            )
        })?;
        if s.is_empty() {
            return Err((
                AppErrorCode::SessionInvalid,
                "allowedOrigins entry must not be empty".into(),
            ));
        }
        allowed_origins.push(s.to_string());
    }

    // 10. caller_origin: 浏览器调用来源必须存在且在 allowedOrigins 列表中（严禁缺少 caller 或空列表绕过）
    let caller = caller_origin.ok_or_else(|| {
        (
            AppErrorCode::SessionInvalid,
            "missing browser caller origin".into(),
        )
    })?;
    if caller.is_empty() {
        return Err((
            AppErrorCode::SessionInvalid,
            "caller origin must not be empty".into(),
        ));
    }
    if !allowed_origins.iter().any(|o| o == caller) {
        return Err((
            AppErrorCode::SessionInvalid,
            format!("caller origin '{caller}' is not in allowedOrigins"),
        ));
    }

    // 11. payloadSha256: 可选字符串，若存在必须为 64 字节十六进制哈希
    let payload_sha256 = if let Some(v) = val.get("payloadSha256") {
        let s = v.as_str().ok_or_else(|| {
            (
                AppErrorCode::PackageInvalid,
                "payloadSha256 must be a string".into(),
            )
        })?;
        if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err((
                AppErrorCode::PackageInvalid,
                "payloadSha256 must be a 64-character hex string".into(),
            ));
        }
        Some(s.to_ascii_lowercase())
    } else {
        None
    };

    // 12. catalogEvidenceSha256: 可选字符串
    let catalog_evidence_sha256 = val
        .get("catalogEvidenceSha256")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 13. 若在标准安装路径下运行（apps/<appId>/runtime/<version>/），校验实际可执行文件哈希与投影 payloadSha256 一致
    if let Some((_layout_root, layout_app_id, layout_version)) = crate::layout::installed_parts() {
        if crate::lock::normalize_app_id(&layout_app_id) == norm_app_id
            && layout_version == current_version
        {
            if let Some(ref expected_hash) = payload_sha256 {
                if let Ok(exe_path) = std::env::current_exe() {
                    verify_executable_sha256(&exe_path, expected_hash)?;
                }
            }
        }
    }

    Ok(ActivationRecord {
        receipt_version,
        app_id: app_id.to_string(),
        runtime_host: runtime_host.to_string(),
        active_version: active_version.to_string(),
        generation,
        activation_state: state.to_string(),
        enabled,
        app_protocol_version,
        payload_sha256,
        allowed_origins,
        catalog_evidence_sha256,
    })
}

/// 兼容既有调用点（未传 expected_generation 时仅校验投影内部有效性）。
pub fn verify_activation(
    apps_root: &Path,
    expected_app_id: &str,
    current_version: &str,
    caller_origin: Option<&str>,
) -> Result<ActivationRecord, (AppErrorCode, String)> {
    verify_activation_with_generation(
        apps_root,
        expected_app_id,
        current_version,
        caller_origin,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_projection(app_id: &str, version: &str) -> serde_json::Value {
        serde_json::json!({
            "receiptVersion": 1,
            "appId": app_id,
            "runtimeHost": format!("com.natives.app.{app_id}"),
            "activeVersion": version,
            "generation": 1,
            "activationState": "ready",
            "enabled": true,
            "appProtocolVersion": 1,
            "allowedOrigins": ["chrome-extension://abcdefghijklmnopabcdefghijklmnop"],
            "payloadSha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        })
    }

    #[test]
    fn verify_activation_checks_all_invariants() {
        let temp_dir = std::env::temp_dir().join(format!("act-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let app_dir = temp_dir.join("fund");
        std::fs::create_dir_all(&app_dir).unwrap();
        let act_file = app_dir.join("activation.json");
        let valid_origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

        // 1. Missing projection -> InstallationChanged
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::InstallationChanged
        );

        // 2. Disabled app -> InstallationChanged
        let mut proj = valid_projection("fund", "1.0.0");
        proj["enabled"] = serde_json::json!(false);
        proj["activationState"] = serde_json::json!("disabled");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::InstallationChanged
        );

        // 3. Maintenance state -> InstallationChanged
        let mut proj = valid_projection("fund", "1.0.0");
        proj["activationState"] = serde_json::json!("maintenance");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::InstallationChanged
        );

        // 4. Version mismatch -> InstallationChanged
        let proj = valid_projection("fund", "1.0.0");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "2.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::InstallationChanged
        );

        // 5. Origin mismatch -> SessionInvalid
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some("chrome-extension://other"))
                .unwrap_err()
                .0,
            AppErrorCode::SessionInvalid
        );

        // 6. Missing caller origin -> SessionInvalid
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", None)
                .unwrap_err()
                .0,
            AppErrorCode::SessionInvalid
        );

        // 7. Empty allowedOrigins -> SessionInvalid
        let mut proj = valid_projection("fund", "1.0.0");
        proj["allowedOrigins"] = serde_json::json!([]);
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::SessionInvalid
        );

        // 8. Missing appId -> StartFailed
        let mut proj = valid_projection("fund", "1.0.0");
        proj.as_object_mut().unwrap().remove("appId");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::StartFailed
        );

        // 9. AppId mismatch -> ProtocolMismatch
        let proj = valid_projection("other", "1.0.0");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::ProtocolMismatch
        );

        // 10. Missing generation -> StartFailed
        let mut proj = valid_projection("fund", "1.0.0");
        proj.as_object_mut().unwrap().remove("generation");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::StartFailed
        );

        // 11. Generation = 0 -> StartFailed
        let mut proj = valid_projection("fund", "1.0.0");
        proj["generation"] = serde_json::json!(0);
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::StartFailed
        );

        // 12. Generation mismatch with expected_generation -> InstallationChanged
        let proj = valid_projection("fund", "1.0.0"); // generation = 1
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation_with_generation(
                &temp_dir,
                "fund",
                "1.0.0",
                Some(valid_origin),
                Some(2)
            )
            .unwrap_err()
            .0,
            AppErrorCode::InstallationChanged
        );

        // 13. Generation matches expected_generation -> Ok
        let rec = verify_activation_with_generation(
            &temp_dir,
            "fund",
            "1.0.0",
            Some(valid_origin),
            Some(1),
        )
        .unwrap();
        assert_eq!(rec.generation, 1);
        assert_eq!(rec.app_id, "fund");
        assert!(rec.enabled);

        // 14. Normalized com.natives.app.fund matches fund
        let rec2 = verify_activation(
            &temp_dir,
            "com.natives.app.fund",
            "1.0.0",
            Some(valid_origin),
        )
        .unwrap();
        assert_eq!(rec2.app_id, "fund");

        // 15. Invalid payloadSha256 format -> PackageInvalid
        let mut proj = valid_projection("fund", "1.0.0");
        proj["payloadSha256"] = serde_json::json!("not-a-valid-sha");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::PackageInvalid
        );

        // 16. AppProtocolVersion mismatch -> ProtocolMismatch
        let mut proj = valid_projection("fund", "1.0.0");
        proj["appProtocolVersion"] = serde_json::json!(2);
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::ProtocolMismatch
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn verify_executable_sha256_checks_exact_hash() {
        let temp_dir = std::env::temp_dir().join(format!("exe-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("test-bin");
        let content = b"binary-executable-content-12345";
        std::fs::write(&file_path, content).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(content);
        let correct_hash = format!("{:x}", hasher.finalize());

        assert!(verify_executable_sha256(&file_path, &correct_hash).is_ok());
        assert_eq!(
            verify_executable_sha256(&file_path, &"0".repeat(64))
                .unwrap_err()
                .0,
            AppErrorCode::PackageInvalid
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
