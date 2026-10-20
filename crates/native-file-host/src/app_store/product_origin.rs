//! The Web shell uses the extension identity sealed by the installed product.
use super::types::AppError;
use super::AppStore;
use base64::Engine;
use sha2::{Digest, Sha256};

impl AppStore {
    pub(crate) fn trusted_extension_origin(&self) -> Result<String, AppError> {
        let (manifest, source) = self.read_source_manifest()?;
        let extension = manifest
            .get("extension")
            .ok_or_else(|| AppError::PackageInvalid("signed extension metadata missing".into()))?;
        let path = extension
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::PackageInvalid("extension path missing".into()))?;
        let expected = extension
            .get("treeSha256")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::PackageInvalid("extension digest missing".into()))?;
        let directory = source.join(path);
        crate::app_files::validate_app_path(&source, &directory)?;
        if crate::app_tree_hash::tree_sha256(&directory)?.as_deref() != Some(expected) {
            return Err(AppError::PackageInvalid("extension digest mismatch".into()));
        }
        let bytes = std::fs::read(directory.join("manifest.json"))?;
        let extension: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| AppError::PackageInvalid("extension manifest invalid".into()))?;
        let key = extension
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::PackageInvalid("extension key missing".into()))?;
        let public_key = base64::engine::general_purpose::STANDARD
            .decode(key)
            .map_err(|_| AppError::PackageInvalid("extension key invalid".into()))?;
        let digest = Sha256::digest(public_key);
        let id: String = digest[..16]
            .iter()
            .flat_map(|byte| {
                [
                    char::from(b'a' + (byte >> 4)),
                    char::from(b'a' + (byte & 15)),
                ]
            })
            .collect();
        Ok(format!("chrome-extension://{id}/"))
    }
}
