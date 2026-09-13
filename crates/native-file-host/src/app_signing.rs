//! Catalog signature verification and platform code-signature checks
//! (ADR-0027: fixed official trust root; contract §4.0 — the Host verifies
//! the signed catalog itself, never trusting page-supplied verdicts).
//!
//! The trust root is a single Ed25519 public key compiled into the Host.
//! Rotation is a Core release event, not a Catalog field. Signing happens
//! in the release pipeline (`scripts/apps/sign-catalog.mjs`); this module
//! only verifies. Platform checks are time-boxed and must reap the spawned
//! process; unsigned binaries are only allowed for isolated dev fixtures.

use crate::app_store::types::AppError;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Fixed official catalog trust root (dev root; release rotation replaces
/// these bytes via the pipeline, single source of truth per ADR-0027).
/// Ed25519 verification key, raw 32 bytes.
pub const CATALOG_TRUST_ROOT_HEX: &str =
    "b266cdb84c18a63d7369906112ec35be2cb6534c21d2a13b416c7c5a88c5caf2";

/// Time limit for external platform verification calls (plan §132: 30 s),
/// enforced by polling; the child is killed on expiry.
const PLATFORM_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

fn trust_root() -> Result<VerifyingKey, AppError> {
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&CATALOG_TRUST_ROOT_HEX[i * 2..i * 2 + 2], 16)
            .map_err(|_| AppError::InvalidState("corrupted trust root".into()))?;
    }
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| AppError::InvalidState("corrupted trust root".into()))
}

/// Verify a base64 Ed25519 signature over the raw catalog bytes against
/// the fixed trust root. `signature` must decode to exactly 64 bytes.
pub fn verify_catalog_signature(catalog: &[u8], signature_b64: &str) -> Result<(), AppError> {
    use base64::Engine;
    let sig = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .map_err(|_| {
            AppError::InvalidState("APP_SIGNATURE_INVALID: undecodable signature".into())
        })?;
    let sig = Signature::from_slice(&sig).map_err(|_| {
        AppError::InvalidState("APP_SIGNATURE_INVALID: bad signature length".into())
    })?;
    trust_root()?.verify(catalog, &sig).map_err(|_| {
        AppError::InvalidState("APP_SIGNATURE_INVALID: catalog signature rejected".into())
    })?;
    // AC-13: production builds must fail closed rather than keep trusting the
    // repository development root. Release rotation compiles a different
    // root; until that happens a release build rejects dev-signed catalogs.
    if is_production_build() {
        return Err(AppError::InvalidState(
            "APP_DEV_TRUST_ROOT_IN_PRODUCTION: release build rejects the development catalog trust root".into(),
        ));
    }
    Ok(())
}

/// Production builds are release-profile binaries; isolated dev fixtures and
/// the development trust root are rejected there (managed-app contract §4.2).
pub fn is_production_build() -> bool {
    !cfg!(debug_assertions)
}

/// Platform gatekeeper check for a managed_local executable. Returns the
/// verification backend actually used, so evidence can name it. Isolated
/// dev fixtures bypass this only through the explicit fixture flag.
pub fn verify_platform_signature(
    executable: &std::path::Path,
    fixture: bool,
) -> Result<&'static str, AppError> {
    if fixture {
        // Defense in depth: install only sets the fixture flag in debug
        // builds, but the gate itself must also refuse in production.
        if is_production_build() {
            return Err(AppError::InvalidState(
                "APP_FIXTURE_REJECTED_IN_PRODUCTION: fixture binaries are not installable in release builds".into(),
            ));
        }
        return Ok("fixture-unverified");
    }
    #[cfg(target_os = "macos")]
    {
        return verify_macos(executable);
    }
    #[cfg(target_os = "windows")]
    {
        return verify_windows(executable);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = executable;
        // Linux has no unified OS code-signing mechanism (plan §132): the
        // Ed25519 catalog signature + payload hash + restricted file modes
        // are the acceptance evidence.
        Ok("linux-ed25519-only")
    }
}

#[cfg(target_os = "macos")]
fn verify_macos(executable: &std::path::Path) -> Result<&'static str, AppError> {
    // `codesign --verify --strict --verify-time <s>` validates the embedded
    // signature chain. Gatekeeper assessment (notarization acceptance) is a
    // separate syspolicyd verdict; `spctl -a` asks for it explicitly.
    // Not-applicable outputs are failures, never success (plan §132).
    let mut codesign = Command::new("/usr/bin/codesign");
    codesign
        .args(["--verify", "--strict", "--verify-time=25"])
        .arg(executable)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    run_bounded(&mut codesign).map_err(|_| {
        AppError::InvalidState(
            "APP_PLATFORM_SIGNATURE_INVALID: codesign rejected the executable".into(),
        )
    })?;
    let mut spctl = Command::new("/usr/sbin/spctl");
    spctl
        .args(["-a", "-t", "exec"])
        .arg(executable)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    run_bounded(&mut spctl).map_err(|_| {
        AppError::InvalidState(
            "APP_PLATFORM_SIGNATURE_INVALID: Gatekeeper assessment rejected the executable".into(),
        )
    })?;
    Ok("macos-codesign+gatekeeper")
}

#[cfg(target_os = "windows")]
fn verify_windows(executable: &std::path::Path) -> Result<&'static str, AppError> {
    // WinVerifyTrust via PowerShell Get-AuthenticodeSignature: the Status
    // string must be exactly Valid; anything else (HashMismatch, NotSigned,
    // UnknownError, revocation-ambiguous) is a rejection (plan §132).
    let script = format!(
        "(Get-AuthenticodeSignature -LiteralPath {}).Status.ToString()",
        executable.display()
    );
    let mut powershell = Command::new("powershell");
    powershell
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    let output = run_bounded(&mut powershell).map_err(|_| {
        AppError::InvalidState("APP_PLATFORM_SIGNATURE_INVALID: Authenticode check failed".into())
    })?;
    if output != "Valid" {
        return Err(AppError::InvalidState(format!(
            "APP_PLATFORM_SIGNATURE_INVALID: Authenticode status {output}"
        )));
    }
    Ok("windows-authenticode")
}

/// Run a verification command with a hard time limit; kill on expiry and
/// always reap the child. Returns trimmed stdout on success.
fn run_bounded(command: &mut Command) -> Result<String, AppError> {
    command.stdin(Stdio::null());
    let mut child = command.spawn().map_err(|error| {
        AppError::InvalidState(format!("platform verifier unavailable: {error}"))
    })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stdout);
                }
                if status.success() {
                    return Ok(stdout.trim().to_string());
                }
                return Err(AppError::InvalidState("platform verifier rejected".into()));
            }
            Ok(None) if started.elapsed() > PLATFORM_CHECK_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::InvalidState(
                    "APP_PLATFORM_SIGNATURE_INVALID: verification timed out".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::InvalidState(format!(
                    "platform verifier error: {error}"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV_PRIVATE_PEM: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/apps/keys/catalog-trust-dev.private.pem"
    );

    fn sign_with_openssl(catalog: &[u8]) -> Option<String> {
        let input = std::env::temp_dir().join(format!(
            "sign-test-{}-{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace([' ', ':'], "_")
        ));
        std::fs::write(&input, catalog).ok()?;
        let output = Command::new("openssl")
            .args(["pkeyutl", "-sign", "-inkey"])
            .arg(DEV_PRIVATE_PEM)
            .args(["-rawin", "-in"])
            .arg(&input)
            .output()
            .ok()?;
        let _ = std::fs::remove_file(&input);
        if !output.status.success() {
            return None;
        }
        use base64::Engine;
        Some(base64::engine::general_purpose::STANDARD.encode(&output.stdout))
    }

    #[test]
    fn valid_catalog_signature_verifies() {
        let catalog = br#"{"v":3,"apps":[]}"#;
        let Some(signature) = sign_with_openssl(catalog) else {
            // openssl unavailable in this environment; the signing fixture
            // test in scripts/apps covers the real pipeline.
            return;
        };
        verify_catalog_signature(catalog, &signature).expect("valid signature must verify");
    }

    #[test]
    fn tampered_catalog_is_rejected() {
        let catalog = br#"{"v":3,"apps":[]}"#;
        let Some(signature) = sign_with_openssl(catalog) else {
            return;
        };
        let tampered = br#"{"v":3,"apps":[{"app_id":"evil"}]}"#;
        assert!(verify_catalog_signature(tampered, &signature).is_err());
    }

    #[test]
    fn malformed_signatures_are_rejected() {
        let catalog = br#"{}"#;
        assert!(verify_catalog_signature(catalog, "not-base64!!").is_err());
        assert!(verify_catalog_signature(catalog, "").is_err());
        // 32 bytes: wrong length even though it decodes.
        assert!(
            verify_catalog_signature(catalog, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .is_err()
        );
    }
}
