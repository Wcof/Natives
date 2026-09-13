//! Per-user product configuration (plan §3.3, ADR-0027/0029 2026-09-12
//! convergence): one "Finish Natives setup" transaction verifies the signed
//! product composition manifest from the trusted system source, prepares the
//! private active payload, browser registration and activation projection
//! for every fixed module, and binds them to one `productGeneration`.
//!
//! There is no per-module install record, no download and no seed chain:
//! the source is already-complete local product content, verified by
//! signature and per-file SHA-256 before anything on the user side changes.

use super::mutation::{bump_revision, lock_error, now_millis, AppStore};
use super::query;
use super::types::AppError;
use crate::{app_activation, app_install};
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
}

fn version_tuple(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
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

struct StagedModule {
    app_id: String,
    version: String,
    entry_route: String,
    name_zh: String,
    name_en: String,
    payload_sha256: String,
    staged_exe: PathBuf,
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
        crate::app_signing::verify_catalog_signature(&bytes, signature.trim())?;
        let manifest: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            AppError::InvalidState("APP_PACKAGE_INVALID: unreadable product manifest".into())
        })?;
        if manifest
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
            || manifest.get("product").and_then(serde_json::Value::as_str) != Some("natives")
        {
            return Err(AppError::InvalidState(
                "APP_PACKAGE_INVALID: unsupported product manifest".into(),
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
    pub fn product_status(&self) -> Result<ProductStatus, AppError> {
        let (version, generation) = self.product_meta()?;
        let source = self.read_source_manifest();
        let source_version = source
            .as_ref()
            .ok()
            .and_then(|(manifest, _)| manifest.get("version").and_then(serde_json::Value::as_str))
            .unwrap_or_default()
            .to_string();
        // D09：就绪需要当前证据（载荷目录 + activation 投影仍在），
        // 相同清单快路径也不能掩盖文件/注册丢失。
        let evidence_ok = generation > 0
            && self
                .with_conn(|conn| {
                    Ok(query::apps(conn, Some(self.app_root()))?.iter().all(|app| {
                        app.host_registered
                            && self.app_root().join(&app.app_id).join("runtime").is_dir()
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
        Ok(ProductStatus {
            version,
            generation,
            configured,
            source_present: source.is_ok(),
            source_version,
        })
    }

    /// One idempotent product configuration transaction (plan §3.3). Every
    /// module artifact is signature- and hash-verified into staging BEFORE
    /// any user-side state changes; the identical already-applied manifest is
    /// a no-op that keeps its generation. A failure leaves each module either
    /// fully applied or untouched and can be retried from the same source.
    pub fn product_configure(&self, origin: &str) -> Result<ProductStatus, AppError> {
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
        let mut staged = Vec::new();
        for entry in entries {
            let app_id = entry
                .get("appId")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    AppError::InvalidState("APP_PACKAGE_INVALID: module appId missing".into())
                })?;
            app_install::validate_identifier(app_id, "app id")?;
            let version = entry
                .get("version")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    AppError::InvalidState("APP_PACKAGE_INVALID: module version missing".into())
                })?;
            let artifact = entry
                .get("artifactPath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    AppError::InvalidState(
                        "APP_PACKAGE_INVALID: module artifactPath missing".into(),
                    )
                })?;
            let expected = entry
                .get("payloadSha256")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    AppError::InvalidState(
                        "APP_PACKAGE_INVALID: module payloadSha256 missing".into(),
                    )
                })?;
            let artifact_path = source.join(artifact);
            app_install::validate_app_path(&source, &artifact_path)?;
            // Copy first, verify what actually landed in the private staging
            // area: no byte of user state changes before every hash matches.
            let staging = self.app_root().join(app_id).join("staging").join("product");
            let _ = app_install::remove_staging_dir(&staging);
            std::fs::create_dir_all(&staging)?;
            let staged_exe = staging.join("app");
            std::fs::copy(&artifact_path, &staged_exe)?;
            let staged_bytes = std::fs::read(&staged_exe)?;
            if hex_sha256(&staged_bytes) != expected {
                let _ = app_install::remove_staging_dir(&staging);
                return Err(AppError::PackageInvalid(format!(
                    "module {app_id} payload hash mismatch against the signed product manifest"
                )));
            }
            app_activation::make_executable(&staged_exe)?;
            let name = entry.get("name").cloned().unwrap_or(serde_json::json!({}));
            staged.push(StagedModule {
                app_id: app_id.to_string(),
                version: version.to_string(),
                entry_route: entry
                    .get("entryRoute")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&format!("app.html?app={app_id}"))
                    .to_string(),
                name_zh: name
                    .get("zh_CN")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(app_id)
                    .to_string(),
                name_en: name
                    .get("en")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(app_id)
                    .to_string(),
                payload_sha256: expected.to_string(),
                staged_exe,
            });
        }

        let manifest_dir = self.manifest_dir()?;
        for module in &staged {
            let runtime_version = self
                .app_root()
                .join(&module.app_id)
                .join("runtime")
                .join(&module.version);
            let existing = runtime_version.join("app");
            let already_applied = existing.exists()
                && std::fs::read(&existing)
                    .map(|bytes| hex_sha256(&bytes) == module.payload_sha256)
                    .unwrap_or(false);
            if already_applied {
                let _ = app_install::remove_staging_dir(
                    module.staged_exe.parent().unwrap_or(&runtime_version),
                );
            } else {
                if existing.exists() {
                    std::fs::remove_dir_all(&runtime_version)?;
                }
                std::fs::create_dir_all(&runtime_version)?;
                if std::fs::rename(&module.staged_exe, &existing).is_err() {
                    std::fs::copy(&module.staged_exe, &existing)?;
                }
                let _ = app_install::remove_staging_dir(
                    module.staged_exe.parent().unwrap_or(&runtime_version),
                );
            }
            let host = app_activation::register_runtime_host(
                &module.app_id,
                &existing,
                &origin,
                &manifest_dir,
            )?;
            self.with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                let preference = tx
                    .query_row(
                        "SELECT enabled, show_in_sidebar, sidebar_order FROM apps WHERE app_id = ?1",
                        [&module.app_id],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
                    )
                    .optional()
                    .map_err(AppError::Sql)?;
                let (enabled, show_in_sidebar, sidebar_order) =
                    preference.unwrap_or((1, 1, 0));
                let runtime_spec = serde_json::json!({
                    "host": host, "version": module.version, "entryRoute": module.entry_route,
                });
                tx.execute(
                    "INSERT INTO apps (app_id, kind, name, version, enabled, show_in_sidebar, sidebar_order,
                        runtime_spec_json, surface_json, manifest_json, installed_at, updated_at, revision, host_registered, needs_migration)
                     VALUES (?1, 'managed_local', ?2, ?3, ?4, ?5, ?6, ?7, '{}', '{}', ?8, ?8, 0, 1, 0)
                     ON CONFLICT(app_id) DO UPDATE SET
                        version = excluded.version, host_registered = 1, needs_migration = 0,
                        runtime_spec_json = excluded.runtime_spec_json, updated_at = excluded.updated_at,
                        revision = revision + 1",
                    params![
                        module.app_id,
                        module.name_zh,
                        module.version,
                        enabled,
                        show_in_sidebar,
                        sidebar_order,
                        runtime_spec.to_string(),
                        now_millis(),
                    ],
                )
                .map_err(AppError::Sql)?;
                bump_revision(&tx)?;
                tx.commit()?;
                Ok(())
            })?;
            let activation_generation =
                app_activation::next_activation_generation(self.app_root(), &module.app_id);
            // D08：activation enabled 与 DB 偏好一致；历史停用选择保留。
            let enabled = self.with_conn(|conn| {
                Ok(query::app(conn, &module.app_id, Some(self.app_root()))
                    .map(|app| app.enabled)
                    .unwrap_or(true))
            })?;
            let projection = serde_json::json!({
                "receiptVersion": 1,
                "appId": module.app_id,
                "runtimeHost": host,
                "activeVersion": module.version,
                "generation": activation_generation,
                "activationState": if enabled { "ready" } else { "disabled" },
                "enabled": enabled,
                "appProtocolVersion": 1,
                "payloadSha256": module.payload_sha256,
                "allowedOrigins": [origin],
            });
            app_activation::write_activation_projection(
                self.app_root(),
                &module.app_id,
                &projection,
            )?;
        }

        // One productGeneration binds every module of this configuration.
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
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
                ("product_manifest_sha256", manifest_sha),
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
        self.product_status()
    }
}
