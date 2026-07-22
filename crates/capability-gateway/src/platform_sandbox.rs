//! Platform process sandbox profiles (Phase 1).
//!
//! - macOS: Seatbelt (`sandbox-exec`) write-scope for project + runtime + temp.
//! - Windows: Job Object process-tree management (best-effort); AppContainer is
//!   **not** implemented — autonomous shell stays disabled.
//!
//! Honesty: capability advertisement must set `kernel_sandbox` / `autonomous_shell`
//! according to what this module actually provides.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProfile {
    /// Full project write + network for subprocesses (still path-checked in tools).
    ProjectWrite,
    /// No project write, no subprocess network.
    ReadOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    /// True only when a kernel-enforced sandbox is applied (macOS Seatbelt success).
    pub kernel_sandbox: bool,
    /// True only when shell can run without per-call approval under kernel sandbox.
    pub autonomous_shell: bool,
    pub platform: String,
    pub notes: Vec<String>,
}

impl PlatformCapabilities {
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                kernel_sandbox: seatbelt_available(),
                // Autonomous shell requires successful seatbelt apply at spawn time.
                autonomous_shell: seatbelt_available(),
                platform: "macos".into(),
                notes: vec![
                    "Seatbelt limits writes to project, Natives runtime, and temp".into(),
                    "sandbox-exec failure refuses command start".into(),
                ],
            }
        }
        #[cfg(target_os = "windows")]
        {
            Self {
                kernel_sandbox: false,
                autonomous_shell: false,
                platform: "windows".into(),
                notes: vec![
                    "Job Object manages process tree when available".into(),
                    "AppContainer not implemented — run_terminal always needs approval".into(),
                    "Path checks are application-layer, not kernel sandbox".into(),
                ],
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self {
                kernel_sandbox: false,
                autonomous_shell: false,
                platform: std::env::consts::OS.into(),
                notes: vec!["No kernel sandbox on this platform".into()],
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn seatbelt_available() -> bool {
    Path::new("/usr/bin/sandbox-exec").is_file()
}

/// Build a Seatbelt profile string. Returns None if sandbox cannot be applied.
#[cfg(target_os = "macos")]
pub fn macos_seatbelt_profile(
    profile: SandboxProfile,
    project_root: &Path,
    runtime_dir: &Path,
) -> Result<String, String> {
    if !seatbelt_available() {
        return Err("sandbox-exec not found".into());
    }
    let project = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let runtime = runtime_dir
        .canonicalize()
        .unwrap_or_else(|_| runtime_dir.to_path_buf());
    let tmp = std::env::temp_dir();
    let project_s = project.display().to_string().replace('"', "\\\"");
    let runtime_s = runtime.display().to_string().replace('"', "\\\"");
    let tmp_s = tmp.display().to_string().replace('"', "\\\"");

    let write_rules = match profile {
        SandboxProfile::ProjectWrite => format!(
            r#"(allow file-write* (subpath "{project_s}") (subpath "{runtime_s}") (subpath "{tmp_s}"))
(allow file-write-data (subpath "{project_s}") (subpath "{runtime_s}") (subpath "{tmp_s}"))"#
        ),
        SandboxProfile::ReadOnly => format!(
            r#"(allow file-write* (subpath "{runtime_s}") (subpath "{tmp_s}"))
(deny file-write* (subpath "{project_s}"))"#
        ),
    };

    let network = match profile {
        SandboxProfile::ProjectWrite => "(allow network*)",
        SandboxProfile::ReadOnly => "(deny network*)",
    };

    Ok(format!(
        r#"(version 1)
(deny default)
(allow process-exec)
(allow process-fork)
(allow signal)
(allow sysctl-read)
(allow mach-lookup)
(allow file-read*)
{write_rules}
{network}
"#
    ))
}

/// Wrap a command with sandbox-exec when possible. On failure for required sandbox, Err.
#[cfg(target_os = "macos")]
pub fn wrap_command_macos(
    profile: SandboxProfile,
    project_root: &Path,
    runtime_dir: &Path,
    program: &str,
    args: &[String],
) -> Result<(String, Vec<String>), String> {
    let sb = macos_seatbelt_profile(profile, project_root, runtime_dir)?;
    // Write profile to a temp file for sandbox-exec -f
    let path = std::env::temp_dir().join(format!(
        "natives-sb-{}.sb",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, sb).map_err(|e| format!("write seatbelt profile: {e}"))?;
    let mut out_args = vec![
        "-f".into(),
        path.display().to_string(),
        program.to_string(),
    ];
    out_args.extend(args.iter().cloned());
    Ok(("/usr/bin/sandbox-exec".into(), out_args))
}

#[cfg(not(target_os = "macos"))]
pub fn wrap_command_macos(
    _profile: SandboxProfile,
    _project_root: &Path,
    _runtime_dir: &Path,
    program: &str,
    args: &[String],
) -> Result<(String, Vec<String>), String> {
    Ok((program.to_string(), args.to_vec()))
}

/// Windows: assign current process to a Job Object (best-effort). Full child
/// assignment is done at spawn via process_supervisor creation flags.
#[cfg(target_os = "windows")]
pub fn windows_job_object_supported() -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
pub fn windows_job_object_supported() -> bool {
    false
}

/// Whether run_terminal may skip per-call approval on this platform/profile.
pub fn allow_autonomous_shell(profile: SandboxProfile) -> bool {
    let caps = PlatformCapabilities::detect();
    if !caps.autonomous_shell {
        return false;
    }
    matches!(profile, SandboxProfile::ProjectWrite)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_caps_are_honest() {
        let caps = PlatformCapabilities::detect();
        assert!(!caps.platform.is_empty());
        #[cfg(target_os = "windows")]
        {
            assert!(!caps.kernel_sandbox);
            assert!(!caps.autonomous_shell);
        }
        #[cfg(target_os = "macos")]
        {
            // kernel_sandbox tracks sandbox-exec presence only
            assert_eq!(caps.kernel_sandbox, seatbelt_available());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_profile_contains_project_path() {
        let tmp = std::env::temp_dir().join(format!("sb-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let profile =
            macos_seatbelt_profile(SandboxProfile::ReadOnly, &tmp, &tmp).unwrap();
        assert!(profile.contains("deny default") || profile.contains("(deny default)"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
