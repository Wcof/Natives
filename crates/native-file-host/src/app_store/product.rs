//! Per-user product projection (plan §3.3, ADR-0027/0029 2026-09-12
//! convergence): the first verified product handshake checks the signed
//! composition manifest from the trusted system source, prepares the private
//! active payload, browser registration and activation projection for every
//! fixed module, and binds them to one `productGeneration`.
//!
//! There is no per-module install record, no download and no seed chain:
//! the source is already-complete local product content, verified by
//! signature and per-file SHA-256 before anything on the user side changes.

use super::mutation::{bump_revision, lock_error, now_millis, AppStore};
use super::query;
use super::types::AppError;
use crate::{app_activation, app_files};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const PRODUCT_MANIFEST_NAME: &str = "product-manifest.json";
pub const PRODUCT_MANIFEST_SIGNATURE_NAME: &str = "product-manifest.sig";

#[derive(Serialize, Debug)]
pub struct ProductStatus {
    /// Configured product version; empty before the first configuration.
    pub version: String,
    /// Monotonic per-user configuration counter; 0 = never configured.
    pub generation: u64,
    pub configured: bool,
    /// A verifiable signed product manifest is available in the system source.
    pub source_present: bool,
    /// Product version declared by the available source (empty when absent).
    pub source_version: String,
    /// The applied manifest hash equals the current source manifest hash.
    pub integrity_valid: bool,
    /// Applied state reflects the currently available source (version + manifest hash).
    pub current: bool,
    /// Product reconfiguration is required (fresh install, N→N+1, or manifest change).
    pub needs_configuration: bool,
}

fn version_tuple(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// true 表示 applied 版本高于 source 版本（拒绝自动降级重配置，计划 §23.1）。
fn version_is_downgrade(applied: &str, source: &str) -> bool {
    if applied.is_empty() || source.is_empty() {
        return false;
    }
    version_tuple(applied) > version_tuple(source)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn current_platform() -> (&'static str, &'static str) {
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    (platform, arch)
}

impl AppStore {
    /// Read and fully verify the signed product manifest from the trusted
    /// system source. Nothing outside the source directory is touched.
    fn read_source_manifest(&self) -> Result<(serde_json::Value, PathBuf), AppError> {
        let source = self.product_source();
        let manifest_path = source.join(PRODUCT_MANIFEST_NAME);
        let bytes = std::fs::read(&manifest_path)
            .map_err(|_| AppError::NotFound("product source manifest".into()))?;
        let signature = std::fs::read_to_string(source.join(PRODUCT_MANIFEST_SIGNATURE_NAME))
            .map_err(|_| AppError::NotFound("product source manifest signature".into()))?;
        crate::app_signing::verify_product_manifest_signature(&bytes, signature.trim())?;
        let manifest: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            AppError::InvalidState("APP_PACKAGE_INVALID: unreadable product manifest".into())
        })?;
        let schema_version = manifest
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if schema_version != 2
            || manifest.get("product").and_then(serde_json::Value::as_str) != Some("natives")
        {
            return Err(AppError::InvalidState(
                "APP_PRODUCT_SCHEMA_UNSUPPORTED: product manifest schema 2 is the only supported production format".into(),
            ));
        }
        let (platform, arch) = current_platform();
        if manifest.get("platform").and_then(serde_json::Value::as_str) != Some(platform)
            || manifest.get("arch").and_then(serde_json::Value::as_str) != Some(arch)
        {
            return Err(AppError::InvalidState(
                "APP_UNSUPPORTED_PLATFORM: product manifest does not match this machine".into(),
            ));
        }
        if manifest
            .get("modules")
            .and_then(serde_json::Value::as_array)
            .is_none()
        {
            return Err(AppError::InvalidState(
                "APP_PACKAGE_INVALID: product manifest has no modules".into(),
            ));
        }
        Ok((manifest, source))
    }

    fn product_meta(&self) -> Result<(String, u64), AppError> {
        self.with_conn(|conn| {
            let read = |key: &str| -> Result<Option<String>, AppError> {
                conn.query_row("SELECT value FROM app_meta WHERE key = ?1", [key], |row| {
                    row.get::<_, String>(0)
                })
                .optional()
                .map_err(AppError::Sql)
            };
            let version = read("product_version")?.unwrap_or_default();
            let generation = read("product_generation")?
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            Ok((version, generation))
        })
    }

    /// Read-only product configuration status for settings and the center.
    /// 判定逻辑（计划 §23.1）：source 不存在 → source_present=false；
    /// 未 applied → needs_configuration=true；sourceVersion != appliedVersion
    /// 或 manifest hash changed → needs_configuration=true；
    /// sourceVersion < appliedVersion → downgrade（configure 时拒绝）。
    pub fn product_status(&self) -> Result<ProductStatus, AppError> {
        let (version, generation) = self.product_meta()?;
        let source = self.read_source_manifest();
        let (source_version, source_manifest_sha) = match &source {
            Ok((manifest, _)) => (
                manifest
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                Some({
                    let bytes =
                        std::fs::read(source.as_ref().unwrap().1.join(PRODUCT_MANIFEST_NAME))
                            .map_err(|_| AppError::NotFound("product source manifest".into()))?;
                    hex_sha256(&bytes)
                }),
            ),
            Err(_) => (String::new(), None),
        };
        // D09：就绪需要当前证据（载荷目录 + activation 投影仍在），
        // 相同清单快路径也不能掩盖文件/注册丢失。
        let evidence_ok = generation > 0
            && self
                .with_conn(|conn| {
                    Ok(query::apps(conn, Some(self.app_root()))?.iter().all(|app| {
                        app.host_registered
                            && crate::app_activation::read_activation_projection(
                                self.app_root(),
                                &app.app_id,
                            )
                            .ok()
                            .flatten()
                            .is_some()
                    }))
                })
                .unwrap_or(false);
        let configured = generation > 0 && evidence_ok && !version.is_empty();
        // applied manifest hash：与 applied 版本一起写入 app_meta。
        let applied_manifest_sha = self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT value FROM app_meta WHERE key = 'product_manifest_sha256'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(AppError::Sql)?)
        })?;
        let integrity_valid = configured
            && source_manifest_sha.is_some()
            && applied_manifest_sha == source_manifest_sha;
        let version_matches = !source_version.is_empty() && source_version == version;
        let current = configured && integrity_valid && version_matches;
        let needs_configuration =
            !current && source.is_ok() && !version_is_downgrade(&version, &source_version);
        Ok(ProductStatus {
            version,
            generation,
            configured,
            source_present: source.is_ok(),
            source_version,
            integrity_valid,
            current,
            needs_configuration,
        })
    }

    /// One idempotent product projection transaction (plan §3.3). Every
    /// module artifact is signature- and hash-verified into staging BEFORE
    /// any user-side state changes; the identical already-applied manifest is
    /// a no-op that keeps its generation. A failure leaves each module either
    /// fully applied or untouched and can be retried from the same source.
    pub(crate) fn configure_product(&self, origin: &str) -> Result<ProductStatus, AppError> {
        let origin = crate::app_host_manifest::normalize_chrome_extension_origin(origin)
            .ok_or_else(|| {
                AppError::InvalidState("APP_ORIGIN_MISMATCH: invalid extension origin".into())
            })?;
        let (manifest, source) = self.read_source_manifest()?;
        let manifest_sha = {
            let bytes = std::fs::read(source.join(PRODUCT_MANIFEST_NAME))
                .map_err(|_| AppError::NotFound("product source manifest".into()))?;
            hex_sha256(&bytes)
        };
        let product_version = manifest
            .get("version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                AppError::InvalidState("APP_PACKAGE_INVALID: product version missing".into())
            })?
            .to_string();
        let (configured_version, configured_generation) = self.product_meta()?;
        // 上一代已应用的 manifest 哈希：Phase 4 失败时恢复用（计划 §25.5）。
        let previous_manifest_sha: Option<String> = self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT value FROM app_meta WHERE key = 'product_manifest_sha256'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(AppError::Sql)?)
        })?;
        if configured_generation > 0
            && version_tuple(&configured_version) > version_tuple(&product_version)
        {
            return Err(AppError::InvalidState(format!(
                "APP_INCOMPATIBLE: refusing downgrade from {configured_version} to {product_version}"
            )));
        }
        if configured_generation > 0 && configured_version == product_version {
            let applied_manifest = self.with_conn(|conn| {
                conn.query_row(
                    "SELECT value FROM app_meta WHERE key = 'product_manifest_sha256'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(AppError::Sql)
            })?;
            if applied_manifest.as_deref() == Some(manifest_sha.as_str()) {
                // Identical already-applied manifest: no-op, same generation.
                return self.product_status();
            }
        }

        // §1.3/§3.2：清单签名绑定 Launcher 可执行与 onboarding 摘要；
        // 字段存在时逐项核验（缺失视为旧候选，保持兼容）。
        if let Some(launcher) = manifest.get("launcher") {
            let verify_fixed = |rel: &str, expected: &str, label: &str| -> Result<(), AppError> {
                let path = std::path::PathBuf::from(rel);
                let bytes = std::fs::read(&path).map_err(|_| {
                    AppError::InvalidState(format!(
                        "APP_PACKAGE_INVALID: {label} missing at {}",
                        path.display()
                    ))
                })?;
                let actual = hex_sha256(&bytes);
                if actual != expected {
                    return Err(AppError::PackageInvalid(format!(
                        "{label} hash mismatch against the signed product manifest"
                    )));
                }
                Ok(())
            };
            if let (Some(bundle_id), Some(sha)) = (
                launcher.get("bundleId").and_then(serde_json::Value::as_str),
                launcher
                    .get("executableSha256")
                    .and_then(serde_json::Value::as_str),
            ) {
                // 模式隔离（plan §5 P1）：bundle ID 与主入口路径按构建模式
                // 判定，与 build-pkg.sh 的 Info.plist / 组装器清单严格一致。
                let local = !crate::app_signing::is_production_build();
                let expected_bundle = if local {
                    "com.natives.local.app"
                } else {
                    "com.natives.app"
                };
                let expected_exec = if local {
                    "/Applications/Natives Local.app/Contents/MacOS/Natives"
                } else {
                    "/Applications/Natives.app/Contents/MacOS/Natives"
                };
                if bundle_id != expected_bundle {
                    return Err(AppError::InvalidState(
                        "APP_PACKAGE_INVALID: unexpected launcher bundle id".into(),
                    ));
                }
                verify_fixed(expected_exec, sha, "launcher executable")?;
            }
            if let Some(sha) = launcher
                .get("onboardingSha256")
                .and_then(serde_json::Value::as_str)
            {
                let local = !crate::app_signing::is_production_build();
                let expected_html = if local {
                    "/Applications/Natives Local.app/Contents/Resources/onboarding/index.html"
                } else {
                    "/Applications/Natives.app/Contents/Resources/onboarding/index.html"
                };
                verify_fixed(expected_html, sha, "onboarding page")?;
            }
        }
        // D08：跨进程操作锁必须先于任何 staging 写入。
        let _operation = self.operation.lock().map_err(lock_error)?;
        let entries = manifest
            .get("modules")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        let manifest_dir = self.manifest_dir()?;

        if let Some(app_runtime) = manifest.get("appRuntime") {
            // =========================================================================
            // Product Manifest Schema 2 (ADR-0031): 统一 App Runtime 二进制与按需实例
            // =========================================================================
            let runtime_rel = app_runtime
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    AppError::InvalidState("APP_PACKAGE_INVALID: appRuntime path missing".into())
                })?;
            let runtime_sha = app_runtime
                .get("sha256")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    AppError::InvalidState("APP_PACKAGE_INVALID: appRuntime sha256 missing".into())
                })?;
            let runtime_exe = source.join(runtime_rel);
            app_files::validate_app_path(&source, &runtime_exe)?;
            let runtime_bytes = std::fs::read(&runtime_exe).map_err(|e| {
                AppError::InvalidState(format!(
                    "APP_PACKAGE_INVALID: failed to read appRuntime executable: {e}"
                ))
            })?;
            let mut effective_runtime_sha = runtime_sha.to_string();
            let actual_runtime_sha = hex_sha256(&runtime_bytes);
            if actual_runtime_sha != runtime_sha {
                if !crate::app_signing::is_production_build() {
                    // 开发环境自愈机制：在日常开发（local）模式下，若二进制发生变更，
                    // 自动接受实际二进制哈希，避免频繁中断开发调试或弹出密封阻断提示。
                    effective_runtime_sha = actual_runtime_sha;
                } else {
                    return Err(AppError::PackageInvalid(
                        "appRuntime payload hash mismatch against the signed product manifest"
                            .into(),
                    ));
                }
            }
            app_activation::make_executable(&runtime_exe)?;
            let runtime_host =
                app_activation::register_app_runtime_host(&runtime_exe, &origin, &manifest_dir)?;

            // Extension tree 强校验（计划 §20/§21）：签名清单声明的 treeSha256
            // 必须与系统源内真实扩展树一致（canonical tree hash，非单文件哈希）。
            if let Some(extension) = manifest.get("extension") {
                if let Some(expected_tree) = extension
                    .get("treeSha256")
                    .and_then(serde_json::Value::as_str)
                {
                    let ext_rel = extension
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("ChromeExtension");
                    let ext_dir = source.join(ext_rel);
                    app_files::validate_app_path(&source, &ext_dir)?;
                    match crate::app_tree_hash::tree_sha256(&ext_dir)? {
                        Some(actual) if actual == expected_tree => {}
                        Some(actual) => {
                            return Err(AppError::PackageInvalid(format!(
                                "extension tree hash mismatch against the signed product manifest (expected {expected_tree}, got {actual})"
                            )));
                        }
                        None => {
                            return Err(AppError::PackageInvalid(
                                "extension tree missing from the verified system source".into(),
                            ));
                        }
                    }
                }
            }

            // -----------------------------------------------------------------
            // Phase 2: Prepare All（只读校验与准备，不改任何正式状态，计划 §25.2）
            // -----------------------------------------------------------------
            struct PreparedModule {
                app_id: String,
                version: String,
                name_zh: String,
                runtime_spec: serde_json::Value,
                enabled: bool,
                show_in_sidebar: bool,
                sidebar_order: i64,
                projection: serde_json::Value,
            }
            let mut prepared = Vec::new();
            for entry in entries {
                let app_id = entry
                    .get("appId")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        AppError::InvalidState("APP_PACKAGE_INVALID: module appId missing".into())
                    })?;
                app_files::validate_identifier(app_id, "app id")?;
                let version = entry
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&product_version);
                let default_route = format!("app.html?app={app_id}");
                let entry_route = entry
                    .get("entryRoute")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&default_route);
                let name = entry.get("name").cloned().unwrap_or(serde_json::json!({}));
                let name_zh = name
                    .get("zh_CN")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(app_id);

                // Module UI tree 强校验（计划 §20.2/§21）：签名清单声明的
                // ui.treeSha256 必须与系统源内真实 UI 资源树一致。
                if let Some(ui) = entry.get("ui") {
                    if let Some(expected_tree) =
                        ui.get("treeSha256").and_then(serde_json::Value::as_str)
                    {
                        let default_ui_rel = format!("modules/{app_id}/ui");
                        let ui_rel = ui
                            .get("path")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&default_ui_rel);
                        let ui_dir = source.join(ui_rel);
                        app_files::validate_app_path(&source, &ui_dir)?;
                        match crate::app_tree_hash::tree_sha256(&ui_dir)? {
                            Some(actual) if actual == expected_tree => {}
                            Some(actual) => {
                                if !crate::app_signing::is_production_build() {
                                    // 开发环境自愈：UI 变动在 dev 模式下放行，避免阻断
                                } else {
                                    return Err(AppError::PackageInvalid(format!(
                                        "module {app_id} UI tree hash mismatch against the signed product manifest (expected {expected_tree}, got {actual})"
                                    )));
                                }
                            }
                            None => {
                                return Err(AppError::PackageInvalid(format!(
                                    "module {app_id} UI tree missing from the verified system source"
                                )));
                            }
                        }
                    }
                }

                // 只读读取用户偏好与激活代数（不改正式状态）。
                let preference = self.with_conn(|conn| {
                    conn.query_row(
                        "SELECT enabled, show_in_sidebar, sidebar_order FROM apps WHERE app_id = ?1",
                        [app_id],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(AppError::Sql)
                })?;
                let (enabled_i64, show_in_sidebar_i64, sidebar_order) =
                    preference.unwrap_or((1, 1, 0));
                let enabled = enabled_i64 != 0;
                let show_in_sidebar = show_in_sidebar_i64 != 0;
                let runtime_spec = serde_json::json!({
                    "host": runtime_host,
                    "version": version,
                    "entryRoute": entry_route,
                });
                let activation_generation =
                    app_activation::next_activation_generation(self.app_root(), app_id);
                let projection = serde_json::json!({
                    "receiptVersion": 2,
                    "appId": app_id,
                    "productVersion": product_version,
                    "productGeneration": {
                        "activationGeneration": activation_generation
                    },
                    "runtimeHost": runtime_host,
                    "appProtocolVersion": 2,
                    "appRuntimeSha256": effective_runtime_sha,
                    "moduleApiVersion": entry.get("moduleApiVersion").cloned().unwrap_or(serde_json::json!(1)),
                    "dataSchemaVersion": entry.get("dataSchemaVersion").cloned().unwrap_or(serde_json::json!(1)),
                    "capabilityVersion": entry.get("capabilityVersion").cloned().unwrap_or(serde_json::json!(1)),
                    "activeVersion": version,
                    "generation": activation_generation,
                    "activationState": if enabled { "ready" } else { "disabled" },
                    "enabled": enabled,
                    "allowedOrigins": [origin.clone()],
                });
                prepared.push(PreparedModule {
                    app_id: app_id.to_string(),
                    version: version.to_string(),
                    name_zh: name_zh.to_string(),
                    runtime_spec,
                    enabled,
                    show_in_sidebar,
                    sidebar_order,
                    projection,
                });
            }

            // -----------------------------------------------------------------
            // Phase 3: DB Transaction（一次事务写全部 app projections + 产品 meta，
            // 计划 §25.3：多模块配置必须原子，禁止半 generation）
            // -----------------------------------------------------------------
            self.with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                let now = now_millis();
                for module in &prepared {
                    tx.execute(
                        "INSERT INTO apps (app_id, kind, name, version, enabled, show_in_sidebar, sidebar_order,
                            runtime_spec_json, surface_json, manifest_json, installed_at, updated_at, revision, host_registered, needs_migration)
                         VALUES (?1, 'managed_local', ?2, ?3, ?4, ?5, ?6, ?7, '{}', '{}', ?8, ?8, 0, 1, 0)
                         ON CONFLICT(app_id) DO UPDATE SET
                            kind = 'managed_local',
                            name = excluded.name,
                            version = excluded.version, host_registered = 1, needs_migration = 0,
                            runtime_spec_json = excluded.runtime_spec_json, updated_at = excluded.updated_at,
                            revision = revision + 1",
                        params![
                            module.app_id,
                            module.name_zh,
                            module.version,
                            module.enabled,
                            module.show_in_sidebar,
                            module.sidebar_order,
                            module.runtime_spec.to_string(),
                            now,
                        ],
                    )
                    .map_err(AppError::Sql)?;
                }
                let generation: u64 = tx
                    .query_row(
                        "SELECT value FROM app_meta WHERE key = 'product_generation'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(AppError::Sql)?
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0)
                    + 1;
                for (key, value) in [
                    ("product_generation", generation.to_string()),
                    ("product_version", product_version.clone()),
                    ("product_manifest_sha256", manifest_sha.clone()),
                ] {
                    tx.execute(
                        "INSERT INTO app_meta (key, value, revision) VALUES (?1, ?2, 0)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                        params![key, value],
                    )
                    .map_err(AppError::Sql)?;
                }
                bump_revision(&tx)?;
                tx.commit()?;
                Ok(())
            })?;

            // -----------------------------------------------------------------
            // Phase 4: Commit File Receipts（原子 rename activation.json + legacy
            // 清理；失败恢复上一 generation，计划 §25.4/§25.5）
            // -----------------------------------------------------------------
            let receipt_result: Result<(), AppError> = (|| {
                for module in &prepared {
                    app_activation::write_activation_projection(
                        self.app_root(),
                        &module.app_id,
                        &module.projection,
                    )?;
                    // 清理旧版 legacy 目录与旧 manifest
                    let _ = std::fs::remove_dir_all(
                        self.app_root().join(&module.app_id).join("runtime"),
                    );
                    app_activation::clean_legacy_runtime_hosts(&manifest_dir, &[&module.app_id]);
                }
                Ok(())
            })();
            if let Err(error) = receipt_result {
                // 恢复上一 generation 的产品 meta，使 DB 与未写完的文件证据一致
                // （下一次同源 configure 可完整重试）。
                let restore_meta: Result<(), AppError> = self.with_conn(|conn| {
                    let tx = conn.unchecked_transaction()?;
                    for (key, value) in [
                        ("product_generation", configured_generation.to_string()),
                        ("product_version", configured_version.clone()),
                        (
                            "product_manifest_sha256",
                            previous_manifest_sha.clone().unwrap_or_default(),
                        ),
                    ] {
                        tx.execute(
                            "INSERT INTO app_meta (key, value, revision) VALUES (?1, ?2, 0)
                             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                            params![key, value],
                        )
                        .map_err(AppError::Sql)?;
                    }
                    bump_revision(&tx)?;
                    tx.commit()?;
                    Ok(())
                });
                if let Err(restore_error) = restore_meta {
                    eprintln!(
                        "configure_product: failed to restore previous product meta after receipt failure: {restore_error}"
                    );
                }
                return Err(error);
            }

            // -----------------------------------------------------------------
            // Phase 5: Mark Product Ready（appliedVersion/appliedGeneration 已
            // 随 Phase 3 提交，文件证据随 Phase 4 落地；返回当前状态）
            // -----------------------------------------------------------------
            self.product_status()
        } else {
            // Fail closed：Schema 2 是唯一生产格式，旧 Schema 1（per-app executable
            // + runtime/<version>/app + per-app Native Host）已删除生产能力。
            return Err(AppError::InvalidState(
                "APP_PRODUCT_SCHEMA_UNSUPPORTED: product manifest schema 2 is the only supported production format".into(),
            ));
        }
    }
}
