//! Suite Seed Reconciliation on Core Startup (ADR-0029, P4-1 to P4-6;
//! AC-03/AC-04 revision).
//!
//! Reconciles preinstalled apps from a SIGNED suite manifest against the
//! Local Registry through the same install transaction as online updates:
//! - Fresh Install: installs seed via the signed catalog trust chain.
//! - Idempotent: if installed == seed version, NOOP (with on-disk repair).
//! - Newer Local Version: KEEP newer, NEVER downgrade.
//! - Older Local Version: upgrade via the unified signed transaction.
//! - User Removal Intent: an app the user removed is never reinstalled;
//!   it stays "kept_removed" until an explicit user action restores it.
//! - Corrupted Seed: fails the item, records the failure, never pollutes
//!   the registry with an unverifiable install.

use super::mutation::AppStore;
use super::types::{AppError, SeedReconcileItem, SeedReconciliationReport, SuiteManifest};
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};

pub const SUITE_MANIFEST_NAME: &str = "suite-manifest.json";

pub fn default_seeds_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::var("NATIVES_SEEDS_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Library/Application Support/Natives/seeds"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".natives").join("seeds"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("seeds"));
            if let Some(up) = parent.parent() {
                dirs.push(up.join("dist").join("seeds"));
                if let Some(up2) = up.parent() {
                    dirs.push(up2.join("dist").join("seeds"));
                }
            }
        }
    }
    dirs
}

impl AppStore {
    /// AC-07: suite preparation runs ONLY on a verified foreground
    /// connection (never at startup, never from SW/newtab). The center calls
    /// this idempotently; per-item statuses and failures are returned so the
    /// UI can show progress and repair needs instead of swallowing them.
    pub fn prepare_suite_seeds(&self) -> Result<SeedReconciliationReport, AppError> {
        if self.caller_origin().is_none() {
            return Err(AppError::InvalidState(
                "APP_ORIGIN_REQUIRED: suite preparation requires a verified foreground connection"
                    .into(),
            ));
        }
        for dir in default_seeds_dirs() {
            if dir.join(SUITE_MANIFEST_NAME).is_file() {
                return self.reconcile_suite_seeds(&dir);
            }
        }
        Ok(SeedReconciliationReport {
            ok: true,
            items: Vec::new(),
        })
    }

    /// Reconciles all apps declared in the signed `suite-manifest.json`.
    pub fn reconcile_suite_seeds(
        &self,
        seeds_dir: &Path,
    ) -> Result<SeedReconciliationReport, AppError> {
        let manifest_path = seeds_dir.join(SUITE_MANIFEST_NAME);
        if !manifest_path.is_file() {
            return Ok(SeedReconciliationReport {
                ok: true,
                items: Vec::new(),
            });
        }
        let manifest_bytes = std::fs::read(&manifest_path)?;
        if manifest_bytes.len() > 256 * 1024 {
            return Err(AppError::InvalidState(
                "suite manifest exceeds 256 KiB bound".into(),
            ));
        }
        let manifest: SuiteManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| AppError::InvalidState(format!("invalid suite manifest: {e}")))?;
        if manifest.schema_version != 2 {
            return Err(AppError::InvalidState(format!(
                "unsupported suite manifest schemaVersion {} (want 2)",
                manifest.schema_version
            )));
        }

        let mut items = Vec::new();
        let mut all_ok = true;

        for entry in manifest.apps {
            let app_id = entry.app_id.clone();
            let item = self.reconcile_seed_entry(seeds_dir, &entry);
            let ok = matches!(
                item.status.as_str(),
                "installed"
                    | "upgraded"
                    | "up_to_date"
                    | "kept_newer"
                    | "kept_removed"
                    | "repaired"
            );
            if !ok {
                all_ok = false;
            }
            items.push(SeedReconcileItem { app_id, ..item });
        }

        Ok(SeedReconciliationReport { ok: all_ok, items })
    }

    fn reconcile_seed_entry(
        &self,
        seeds_dir: &Path,
        entry: &super::types::SuiteManifestEntry,
    ) -> SeedReconcileItem {
        let app_id = entry.app_id.clone();

        // AC-04: the user's removal choice outlives uninstall/cleanup and is
        // never overridden by seed reconciliation.
        match self.user_removal_intent(&app_id) {
            Ok(Some(true)) => {
                return SeedReconcileItem {
                    app_id,
                    status: "kept_removed".into(),
                    version: String::new(),
                    detail: Some(
                        "user removed this app; seed reconciliation keeps it removed".into(),
                    ),
                };
            }
            Ok(None | Some(false)) => {}
            Err(e) => {
                return SeedReconcileItem {
                    app_id,
                    status: "failed".into(),
                    version: String::new(),
                    detail: Some(e.to_string()),
                };
            }
        }

        // Query current installed version
        let current_version: Option<String> = match self.with_conn(|conn| {
            conn.query_row(
                "SELECT version FROM apps WHERE app_id = ?1",
                [&app_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
        }) {
            Ok(v) => v,
            Err(e) => {
                return SeedReconcileItem {
                    app_id,
                    status: "failed".into(),
                    version: String::new(),
                    detail: Some(e.to_string()),
                }
            }
        };

        let origin = self.caller_origin();
        let install = |repair: bool| -> Result<String, AppError> {
            if repair {
                self.install_seed_entry_repair(seeds_dir, entry, origin.as_deref())?;
            } else {
                self.install_seed_entry(seeds_dir, entry, origin.as_deref())?;
            }
            Ok(String::new())
        };

        match current_version {
            None => {
                // P4-2 Fresh Install through the signed transaction.
                match install(false) {
                    Ok(_) => SeedReconcileItem {
                        app_id,
                        status: "installed".into(),
                        version: String::new(),
                        detail: None,
                    },
                    Err(e) => SeedReconcileItem {
                        app_id,
                        status: "failed".into(),
                        version: String::new(),
                        detail: Some(e.to_string()),
                    },
                }
            }
            Some(installed_ver) => {
                let installed_sem = semver::Version::parse(&installed_ver);
                // Seed version comes from the signed catalog, not the manifest.
                let seed_version = match self.seed_catalog_info(entry).map(|(v, _)| v) {
                    Ok(v) => v,
                    Err(e) => {
                        return SeedReconcileItem {
                            app_id,
                            status: "failed".into(),
                            version: installed_ver,
                            detail: Some(e.to_string()),
                        }
                    }
                };
                let seed_sem = semver::Version::parse(&seed_version);
                match (installed_sem, seed_sem) {
                    (Ok(cur), Ok(seed)) if cur == seed => {
                        // T09 (AC-06): same version but a different signed
                        // payload hash is a CONFLICT — never auto-replace.
                        let seed_info = match self.seed_catalog_info(entry) {
                            Ok(info) => info,
                            Err(e) => {
                                return SeedReconcileItem {
                                    app_id,
                                    status: "failed".into(),
                                    version: installed_ver,
                                    detail: Some(e.to_string()),
                                }
                            }
                        };
                        if let Ok(pkgs) =
                            self.with_conn(|conn| super::query::packages(conn, &app_id))
                        {
                            if let Some(receipt) = pkgs
                                .iter()
                                .find(|p| p.package_id == "app-exec")
                                .or_else(|| pkgs.first())
                            {
                                if !receipt.payload_sha256.is_empty()
                                    && receipt.payload_sha256 != seed_info.1
                                {
                                    return SeedReconcileItem {
                                        app_id,
                                        status: "conflict".into(),
                                        version: installed_ver,
                                        detail: Some(format!(
                                            "same version but different payload hash (receipt {} != signed {})",
                                            &receipt.payload_sha256[..16.min(receipt.payload_sha256.len())],
                                            &seed_info.1[..16.min(seed_info.1.len())]
                                        )),
                                    };
                                }
                            }
                        }
                        // P6-3 Seed Repair: reinstall only when the runtime
                        // binary is missing on disk (same signed transaction).
                        let runtime_path = crate::app_install::install_path_for(
                            self.app_root(),
                            &app_id,
                            super::types::KIND_MANAGED_LOCAL,
                            &installed_ver,
                            "app-exec",
                        );
                        if let Ok(path) = runtime_path {
                            if !path.is_file() {
                                return match install(true) {
                                    Ok(_) => SeedReconcileItem {
                                        app_id,
                                        status: "repaired".into(),
                                        version: installed_ver,
                                        detail: Some(
                                            "missing runtime binary repaired from suite seed"
                                                .into(),
                                        ),
                                    },
                                    Err(e) => SeedReconcileItem {
                                        app_id,
                                        status: "failed".into(),
                                        version: installed_ver,
                                        detail: Some(e.to_string()),
                                    },
                                };
                            }
                        }
                        SeedReconcileItem {
                            app_id,
                            status: "up_to_date".into(),
                            version: installed_ver,
                            detail: None,
                        }
                    }
                    (Ok(cur), Ok(seed)) if cur > seed => SeedReconcileItem {
                        app_id,
                        status: "kept_newer".into(),
                        version: installed_ver,
                        detail: Some(format!("installed version {cur} > seed version {seed}")),
                    },
                    (Ok(cur), Ok(seed)) if cur < seed => match install(false) {
                        Ok(_) => SeedReconcileItem {
                            app_id,
                            status: "upgraded".into(),
                            version: seed_version,
                            detail: Some(format!("upgraded from {cur} to {seed}")),
                        },
                        Err(e) => SeedReconcileItem {
                            app_id,
                            status: "failed".into(),
                            version: installed_ver,
                            detail: Some(e.to_string()),
                        },
                    },
                    _ => SeedReconcileItem {
                        app_id,
                        status: "failed".into(),
                        version: installed_ver,
                        detail: Some("invalid semver in installed or seed catalog version".into()),
                    },
                }
            }
        }
    }

    /// The seed app's version and signed payload hash, extracted from the
    /// verified catalog itself (never from an unsigned manifest field).
    fn seed_catalog_info(
        &self,
        entry: &super::types::SuiteManifestEntry,
    ) -> Result<(String, String), AppError> {
        use base64::Engine;
        let catalog = base64::engine::general_purpose::STANDARD
            .decode(&entry.catalog_base64)
            .map_err(|_| AppError::InvalidState("invalid catalog encoding".into()))?;
        crate::app_signing::verify_catalog_signature(&catalog, &entry.signature_base64)?;
        let value: serde_json::Value = serde_json::from_slice(&catalog)
            .map_err(|e| AppError::InvalidState(format!("invalid catalog json: {e}")))?;
        let app = value["apps"]
            .as_array()
            .and_then(|apps| {
                apps.iter()
                    .find(|a| a["app_id"] == serde_json::json!(entry.app_id))
            })
            .ok_or_else(|| AppError::InvalidState("seed catalog missing the app entry".into()))?;
        let version = app["version"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::InvalidState("seed catalog missing app version".into()))?;
        let payload_sha256 = app["packages"]
            .as_array()
            .and_then(|pkgs| pkgs.first())
            .and_then(|p| p["payload_sha256"].as_str())
            .unwrap_or("")
            .to_string();
        Ok((version, payload_sha256))
    }

    /// AC-04: removal intent lives in its own table so uninstall/cleanup can
    /// never resurrect a removed app on the next seed reconciliation.
    fn user_removal_intent(&self, app_id: &str) -> Result<Option<bool>, AppError> {
        self.with_conn(|conn| {
            let choice: Option<String> = conn
                .query_row(
                    "SELECT choice FROM app_user_intent WHERE app_id = ?1",
                    [app_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::from)?;
            Ok(choice.map(|c| c == "removed"))
        })
    }

    /// Record (or change) the user's preinstall choice: default | removed.
    /// Only explicit user actions (uninstall / restore) write here.
    pub fn set_user_intent(&self, app_id: &str, choice: &str) -> Result<(), AppError> {
        crate::app_install::validate_identifier(app_id, "app id")?;
        if choice != "default" && choice != "removed" {
            return Err(AppError::InvalidState(
                "intent choice must be default|removed".into(),
            ));
        }
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_user_intent (app_id, choice, changed_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(app_id) DO UPDATE SET choice = excluded.choice, changed_at = excluded.changed_at",
                rusqlite::params![app_id, choice, super::mutation::now_millis()],
            )
            .map_err(AppError::from)?;
            Ok(())
        })
    }
}
