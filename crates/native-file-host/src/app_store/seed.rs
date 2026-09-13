//! Suite Seed artifact installation through the signed catalog trust chain
//! (AC-03, ADR-0029 R-APP-20): a seed entry is a full Ed25519-signed Catalog
//! v3 plus a local `.nap`. The install reuses the exact online transaction —
//! `install_begin_catalog_for` (signature verification, platform selection,
//! budgets) → chunk staging from the local file → `install_finish` (artifact
//! hash + bounded decompression against SIGNED metadata) → commit. No
//! JSON-self-declared hashes, no second install authority.

use super::mutation::AppStore;
use super::types::{AppError, SuiteManifestEntry, INSTALL_CHUNK_SIZE};
use crate::app_install;
use base64::Engine;
use std::io::Read;

impl AppStore {
    /// Install one seed app via the unified signed-catalog transaction.
    pub fn install_seed_entry(
        &self,
        seeds_dir: &std::path::Path,
        entry: &SuiteManifestEntry,
        origin: Option<&str>,
    ) -> Result<super::types::App, AppError> {
        self.install_seed_entry_inner(seeds_dir, entry, origin, false)
    }

    /// Same-version repair variant (only callable from seed reconciliation's
    /// broken-install branch; downgrade still rejected by the shared guard).
    pub fn install_seed_entry_repair(
        &self,
        seeds_dir: &std::path::Path,
        entry: &SuiteManifestEntry,
        origin: Option<&str>,
    ) -> Result<super::types::App, AppError> {
        self.install_seed_entry_inner(seeds_dir, entry, origin, true)
    }

    fn install_seed_entry_inner(
        &self,
        seeds_dir: &std::path::Path,
        entry: &SuiteManifestEntry,
        origin: Option<&str>,
        repair: bool,
    ) -> Result<super::types::App, AppError> {
        crate::app_install::validate_identifier(&entry.app_id, "app id")?;
        let caller = self.caller_origin();
        let trusted_origin = origin.or(caller.as_deref()).ok_or_else(|| {
            AppError::InvalidState("Chrome caller origin required for seed install".into())
        })?;

        // The artifact must be a bare file name inside the seeds directory:
        // no absolute paths, no `..`, no traversal, no symlink escape.
        let is_bare_name = std::path::Path::new(&entry.artifact)
            .file_name()
            .is_some_and(|name| name == std::ffi::OsStr::new(entry.artifact.as_str()));
        if entry.artifact.is_empty()
            || entry.artifact.contains('/')
            || entry.artifact.contains('\\')
            || entry.artifact.starts_with('.')
            || !is_bare_name
        {
            return Err(AppError::InvalidState(format!(
                "seed artifact name {:?} must be a bare file name",
                entry.artifact
            )));
        }
        let artifact_path = seeds_dir.join(&entry.artifact);
        let meta = std::fs::symlink_metadata(&artifact_path).map_err(|e| {
            AppError::InvalidState(format!("seed artifact {:?}: {e}", entry.artifact))
        })?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(AppError::InvalidState(format!(
                "seed artifact {:?} must be a regular file inside the seeds directory",
                entry.artifact
            )));
        }

        // Same trust chain as online installs: Ed25519 catalog signature,
        // identity/compat selection, budgets, transaction bookkeeping.
        let begin = if repair {
            self.install_begin_catalog_repair_for(
                &entry.catalog_base64,
                &entry.signature_base64,
                &entry.app_id,
            )?
        } else {
            self.install_begin_catalog_for(
                &entry.catalog_base64,
                &entry.signature_base64,
                &entry.app_id,
            )?
        };

        // Feed the local artifact through the standard chunk pipeline. Each
        // chunk is hash-checked and bounded by the SIGNED wire size, so the
        // local file can never exceed or diverge from the catalog contract.
        // On any failure the owned transaction is aborted so the install lock
        // is released and no half-open install survives the caller's error.
        let staged = (|| -> Result<u64, AppError> {
            let mut file = std::fs::File::open(&artifact_path)?;
            let mut offset: u64 = 0;
            let mut buf = vec![0u8; INSTALL_CHUNK_SIZE];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let chunk = &buf[..n];
                self.install_chunk(
                    &begin.install_id,
                    &begin.package_id,
                    offset,
                    &base64::engine::general_purpose::STANDARD.encode(chunk),
                    &app_install::hex_sha256(chunk),
                )?;
                offset += n as u64;
            }
            // Shared verification/decompression pipeline (signed metadata).
            self.install_finish(&begin.install_id, &begin.package_id, offset)?;
            Ok(offset)
        })();

        match staged {
            Ok(offset) => self.install_commit_with_origin(&begin.install_id, Some(trusted_origin)),
            Err(error) => {
                if let Ok(record) = self.owned_transaction(&begin.install_id) {
                    let _ = self.abort_owned(&record, &error.code(), "seed install failed");
                }
                Err(error)
            }
        }
    }
}
