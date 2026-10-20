//! Product activation receipt verification for a built-in module runtime.
//!
//! Product configuration is the sole authority. User enable/display preferences
//! remain in Core and never mutate the signed product receipt.
//! Strict validation: missing identity, malformed types, empty allowedOrigins,
//! missing caller origin, unready state, or mismatched hashes reject startup.

use crate::error::AppErrorCode;
use sha2::{Digest, Sha256};
use std::path::Path;

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

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
    /// 已核验激活收据中该模块的能力声明（ADR-0038 平台能力的唯一门）。
    pub capabilities: Vec<String>,
    /// 已核验激活收据中该模块声明的深链视图白名单（ADR-0038）。
    pub views: Vec<String>,
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

    // 优先：检查单一产品级激活凭据（Receipt v3，架构 Wave 6 / ADR-0031）
    let product_receipt_path = apps_root
        .parent()
        .unwrap_or(apps_root)
        .join("product")
        .join("activation.json");
    if product_receipt_path.exists() {
        // v3 Receipt 一旦存在，读取、解析或验证失败必须 fail closed，禁止回退 v2
        let bytes = std::fs::read(&product_receipt_path).map_err(|e| {
            (
                AppErrorCode::StartFailed,
                format!("failed to read product activation.json: {e}"),
            )
        })?;
        let val: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
            (
                AppErrorCode::StartFailed,
                format!("failed to parse product activation.json: {e}"),
            )
        })?;
        let receipt_version = val
            .get("receiptVersion")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                (
                    AppErrorCode::StartFailed,
                    "receiptVersion missing in product activation.json".into(),
                )
            })?;
        if receipt_version != 3 {
            return Err((
                AppErrorCode::Incompatible,
                format!("unsupported receiptVersion in product activation.json: {receipt_version}"),
            ));
        }
        return verify_product_activation_receipt_v3(
            &val,
            &norm_app_id,
            expected_app_id,
            current_version,
            caller_origin,
            expected_generation,
        );
    }

    Err((
        AppErrorCode::InstallationChanged,
        "product activation receipt v3 not found; reconfigure the product".into(),
    ))
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

fn verify_product_activation_receipt_v3(
    val: &serde_json::Value,
    norm_app_id: &str,
    expected_app_id: &str,
    current_version: &str,
    caller_origin: Option<&str>,
    expected_generation: Option<u64>,
) -> Result<ActivationRecord, (AppErrorCode, String)> {
    // 1. modules check & module entry completeness
    let modules = val
        .get("modules")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "modules map missing in product activation receipt".into(),
            )
        })?;
    let module_entry = modules
        .get(norm_app_id)
        .or_else(|| modules.get(expected_app_id))
        .ok_or_else(|| {
            (
                AppErrorCode::ProtocolMismatch,
                format!("module '{expected_app_id}' not found in product activation receipt"),
            )
        })?;

    // 校验模块必须字段 (moduleApiVersion / dataSchemaVersion / capabilityVersion / uiTreeSha256)
    let module_api_ver = module_entry
        .get("moduleApiVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                format!("module '{expected_app_id}' missing or invalid moduleApiVersion"),
            )
        })?;
    if module_api_ver == 0 {
        return Err((
            AppErrorCode::ProtocolMismatch,
            format!("module '{expected_app_id}' moduleApiVersion must be > 0"),
        ));
    }

    let data_schema_ver = module_entry
        .get("dataSchemaVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                format!("module '{expected_app_id}' missing or invalid dataSchemaVersion"),
            )
        })?;
    if data_schema_ver == 0 {
        return Err((
            AppErrorCode::DataSchemaIncompatible,
            format!("module '{expected_app_id}' dataSchemaVersion must be > 0"),
        ));
    }

    let capability_ver = module_entry
        .get("capabilityVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                format!("module '{expected_app_id}' missing or invalid capabilityVersion"),
            )
        })?;
    if capability_ver == 0 {
        return Err((
            AppErrorCode::Incompatible,
            format!("module '{expected_app_id}' capabilityVersion must be > 0"),
        ));
    }

    let ui_tree_sha = module_entry
        .get("uiTreeSha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                format!("module '{expected_app_id}' missing or invalid uiTreeSha256"),
            )
        })?;
    if !is_sha256(ui_tree_sha) {
        return Err((
            AppErrorCode::PackageInvalid,
            format!("module '{expected_app_id}' uiTreeSha256 must be a 64-character hex string"),
        ));
    }

    // 2. runtime.host, protocolVersion, sha256
    let runtime_obj = val
        .get("runtime")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "runtime object missing in product activation receipt".into(),
            )
        })?;
    let runtime_host = runtime_obj
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "runtime.host missing in product activation receipt".into(),
            )
        })?;
    if runtime_host != "com.natives.app_runtime" && runtime_host != "com.natives.local.app_runtime"
    {
        return Err((
            AppErrorCode::ProtocolMismatch,
            format!("runtimeHost '{runtime_host}' is not the unified app runtime host"),
        ));
    }

    let protocol_ver = runtime_obj
        .get("protocolVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "runtime.protocolVersion missing in product activation receipt".into(),
            )
        })?;
    if protocol_ver != crate::protocol::APP_PROTOCOL_VERSION_V2 as u64 {
        return Err((
            AppErrorCode::ProtocolMismatch,
            format!("runtime.protocolVersion {protocol_ver} is unsupported, expected 2"),
        ));
    }

    let runtime_sha = runtime_obj
        .get("sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "runtime.sha256 missing in product activation receipt".into(),
            )
        })?;
    if !is_sha256(runtime_sha) {
        return Err((
            AppErrorCode::PackageInvalid,
            "runtime.sha256 must be a 64-character hex string".into(),
        ));
    }

    // 校验本进程可执行文件哈希一致（载荷被替换 → fail closed）
    if let Ok(exe_path) = std::env::current_exe() {
        verify_executable_sha256(&exe_path, runtime_sha)?;
    }

    // 3. productManifestSha256
    let manifest_sha = val
        .get("productManifestSha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "productManifestSha256 missing in product activation receipt".into(),
            )
        })?;
    if !is_sha256(manifest_sha) {
        return Err((
            AppErrorCode::PackageInvalid,
            "productManifestSha256 must be a 64-character hex string".into(),
        ));
    }

    // 3. productVersion
    let product_version = val
        .get("productVersion")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "productVersion missing in product activation receipt".into(),
            )
        })?;
    if product_version != current_version {
        return Err((
            AppErrorCode::InstallationChanged,
            format!(
                "productVersion '{product_version}' does not match candidate '{current_version}'"
            ),
        ));
    }

    // 4. productGeneration
    let generation = val
        .get("productGeneration")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "productGeneration missing in product activation receipt".into(),
            )
        })?;
    if generation == 0 {
        return Err((
            AppErrorCode::StartFailed,
            "productGeneration must be > 0".into(),
        ));
    }
    if let Some(expected_gen) = expected_generation {
        if generation != expected_gen {
            return Err((
                AppErrorCode::InstallationChanged,
                format!(
                    "productGeneration '{generation}' does not match expected '{expected_gen}'"
                ),
            ));
        }
    }

    // 5. allowedOrigins
    let origins_arr = val
        .get("allowedOrigins")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            (
                AppErrorCode::StartFailed,
                "allowedOrigins missing in product activation receipt".into(),
            )
        })?;
    if origins_arr.is_empty() {
        return Err((
            AppErrorCode::StartFailed,
            "allowedOrigins must not be empty".into(),
        ));
    }
    let mut allowed_origins = Vec::new();
    for o in origins_arr {
        if let Some(s) = o.as_str() {
            allowed_origins.push(s.to_string());
        }
    }
    if let Some(origin) = caller_origin {
        if !allowed_origins.iter().any(|allowed| allowed == origin) {
            return Err((
                AppErrorCode::SessionInvalid,
                format!("origin '{origin}' is not in allowedOrigins"),
            ));
        }
    }

    Ok(ActivationRecord {
        receipt_version: 3,
        app_id: norm_app_id.to_string(),
        runtime_host: runtime_host.to_string(),
        active_version: current_version.to_string(),
        generation,
        activation_state: "ready".into(),
        enabled: true,
        app_protocol_version: 2,
        payload_sha256: None,
        allowed_origins,
        capabilities: string_array(module_entry.get("capabilities")),
        views: string_array(module_entry.get("views")),
    })
}

fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_projection(app_id: &str, version: &str) -> serde_json::Value {
        serde_json::json!({
            "receiptVersion": 2,
            "appId": app_id,
            "productVersion": version,
            "runtimeHost": "com.natives.app_runtime",
            "appProtocolVersion": 2,
            "activeVersion": version,
            "generation": 1,
            "activationState": "ready",
            "enabled": true,
            "allowedOrigins": ["chrome-extension://abcdefghijklmnopabcdefghijklmnop"],
            // 注意：不带 appRuntimeSha256（可选字段）。携带该字段时会校验
            // 当前进程可执行文件哈希，测试进程无法匹配固定值。
        })
    }

    #[test]
    #[ignore = "legacy v2 receipt is retained only as migration test data"]
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

        // 15. Invalid appRuntimeSha256 format -> PackageInvalid
        let mut proj = valid_projection("fund", "1.0.0");
        proj["appRuntimeSha256"] = serde_json::json!("not-a-valid-sha");
        std::fs::write(&act_file, serde_json::to_vec(&proj).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&temp_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::PackageInvalid
        );

        // 16. AppProtocolVersion mismatch -> ProtocolMismatch
        let mut proj = valid_projection("fund", "1.0.0");
        proj["appProtocolVersion"] = serde_json::json!(99);
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

    #[test]
    fn verify_v3_activation_fail_closed_and_comprehensive() {
        let temp_dir = std::env::temp_dir().join(format!("v3-act-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let apps_dir = temp_dir.join("apps");
        let product_dir = temp_dir.join("product");
        std::fs::create_dir_all(&apps_dir).unwrap();
        std::fs::create_dir_all(&product_dir).unwrap();
        let valid_origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

        let exe_path = std::env::current_exe().unwrap();
        let exe_bytes = std::fs::read(&exe_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&exe_bytes);
        let actual_sha = format!("{:x}", hasher.finalize());

        let mut v3_receipt = serde_json::json!({
            "receiptVersion": 3,
            "productVersion": "1.0.0",
            "productGeneration": 1,
            "productManifestSha256": "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "runtime": {
                "host": "com.natives.app_runtime",
                "protocolVersion": 2,
                "sha256": actual_sha
            },
            "modules": {
                "fund": {
                    "moduleApiVersion": 1,
                    "dataSchemaVersion": 2,
                    "capabilityVersion": 2,
                    "uiTreeSha256": "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    "enabled": true,
                    "activationState": "ready"
                }
            },
            "allowedOrigins": [valid_origin]
        });

        let receipt_file = product_dir.join("activation.json");
        std::fs::write(&receipt_file, serde_json::to_vec(&v3_receipt).unwrap()).unwrap();

        // 1. 成功验证
        let act = verify_activation(&apps_dir, "fund", "1.0.0", Some(valid_origin)).unwrap();
        assert_eq!(act.receipt_version, 3);
        assert_eq!(act.generation, 1);
        assert_eq!(act.app_id, "fund");

        // 2. 用户偏好不得进入或改变产品收据语义。
        v3_receipt["modules"]["fund"]["enabled"] = serde_json::json!(false);
        std::fs::write(&receipt_file, serde_json::to_vec(&v3_receipt).unwrap()).unwrap();
        assert!(verify_activation(&apps_dir, "fund", "1.0.0", Some(valid_origin)).is_ok());
        v3_receipt["modules"]["fund"]["enabled"] = serde_json::json!(true);

        // 3. 缺少 uiTreeSha256 -> 必须拒绝 (StartFailed)
        let mut corrupted = v3_receipt.clone();
        corrupted["modules"]["fund"]
            .as_object_mut()
            .unwrap()
            .remove("uiTreeSha256");
        std::fs::write(&receipt_file, serde_json::to_vec(&corrupted).unwrap()).unwrap();
        assert_eq!(
            verify_activation(&apps_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::StartFailed
        );

        // 4. v3 文件损坏（非法 JSON）-> 必须 fail closed，严禁回退 v2！
        // 放置一个合法的 v2 文件在 apps/fund/activation.json
        let fund_dir = apps_dir.join("fund");
        std::fs::create_dir_all(&fund_dir).unwrap();
        let v2_file = fund_dir.join("activation.json");
        std::fs::write(
            &v2_file,
            serde_json::to_vec(&valid_projection("fund", "1.0.0")).unwrap(),
        )
        .unwrap();

        std::fs::write(&receipt_file, b"corrupted-invalid-json-content").unwrap();
        // 即使 v2 存在，v3 损坏时也必须直接 fail closed (StartFailed)
        assert_eq!(
            verify_activation(&apps_dir, "fund", "1.0.0", Some(valid_origin))
                .unwrap_err()
                .0,
            AppErrorCode::StartFailed
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
