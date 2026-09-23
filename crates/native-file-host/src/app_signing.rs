//! Product manifest trust and build-mode isolation.

use crate::app_store::types::AppError;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Release rotation replaces this compiled product trust root.
pub const PRODUCT_TRUST_ROOT_HEX: &str =
    "b266cdb84c18a63d7369906112ec35be2cb6534c21d2a13b416c7c5a88c5caf2";

fn trust_root() -> Result<VerifyingKey, AppError> {
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&PRODUCT_TRUST_ROOT_HEX[index * 2..index * 2 + 2], 16)
            .map_err(|_| AppError::InvalidState("corrupted product trust root".into()))?;
    }
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| AppError::InvalidState("corrupted product trust root".into()))
}

pub fn verify_product_manifest_signature(
    manifest: &[u8],
    signature_b64: &str,
) -> Result<(), AppError> {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| {
            AppError::InvalidState("APP_SIGNATURE_INVALID: undecodable signature".into())
        })?;
    let signature = Signature::from_slice(&encoded).map_err(|_| {
        AppError::InvalidState("APP_SIGNATURE_INVALID: bad signature length".into())
    })?;
    trust_root()?.verify(manifest, &signature).map_err(|_| {
        AppError::InvalidState("APP_SIGNATURE_INVALID: product manifest signature rejected".into())
    })?;
    if is_production_build() {
        return Err(AppError::InvalidState(
            "APP_DEV_TRUST_ROOT_IN_PRODUCTION: release trust root is required".into(),
        ));
    }
    Ok(())
}

pub fn is_production_build() -> bool {
    !cfg!(debug_assertions)
}
pub fn natives_dir_name() -> &'static str {
    if is_production_build() {
        ".natives"
    } else {
        ".natives-local"
    }
}
pub fn runtime_host_namespace() -> &'static str {
    if is_production_build() {
        "com.natives.app"
    } else {
        "com.natives.local.app"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_product_signatures_are_rejected() {
        assert!(verify_product_manifest_signature(b"{}", "not-base64!!").is_err());
        assert!(verify_product_manifest_signature(b"{}", "").is_err());
    }
}
