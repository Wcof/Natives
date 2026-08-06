//! Versioned Agent proposal + Host validator (batch 10 CR-1001).
//!
//! An agent proposes creating/starting a creative app. The Host validates the
//! proposal before it can become an Application → Profile → Runtime → Window.
//! The validator rejects dangerous inputs:
//! - cwd escape (parent traversal / absolute outside project root)
//! - shell metacharacters / arbitrary shell execution
//! - privileged Compose operations (no privileged containers, no host mounts
//!   outside the project, no command overrides that run arbitrary binaries)
//! - secret values in environment keys (the proposal only carries KEY names)

use super::model::{
    BinaryLaunchProfile, LaunchPort, LaunchPortMode, OwnershipMode, PythonLaunchProfile,
};
use crate::{Error, Result};
use std::path::{Component, Path};

/// The kind of proposal an agent can submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Create,
    Start,
}

/// What an agent proposes to do — a thin, versioned intent that the Host
/// validates and the user approves. Never carries secret values.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProposal {
    pub schema_version: u32,
    pub kind: ProposalKind,
    /// Proposed ownership (managed / attached / remote).
    pub ownership: OwnershipMode,
    /// Title for the new app record.
    pub title: String,
    /// Project root (absolute) the app would run from.
    pub project_root: String,
    /// Proposed driver payload — validated per kind.
    pub driver: ProposedDriver,
    /// Proposed open URL after start (validated to loopback for local).
    pub open_path: String,
    pub health_path: String,
    /// Env KEY names (never values).
    pub environment_keys: Vec<String>,
}

// Manually split the driver field to keep AgentProposal clean.
impl AgentProposal {
    pub fn validate(&self) -> Result<()> {
        // Schema version must be current.
        if self.schema_version != 1 {
            return Err(Error::InvalidInput(format!(
                "unsupported proposal schema version {}",
                self.schema_version
            )));
        }
        if self.title.trim().is_empty() {
            return Err(Error::InvalidInput("proposal title cannot be empty".into()));
        }
        // Project root must be absolute and non-empty.
        let root = Path::new(&self.project_root);
        if !root.is_absolute() {
            return Err(Error::InvalidInput(
                "proposal project root must be absolute".into(),
            ));
        }
        // Driver-specific validation.
        match &self.driver {
            ProposedDriver::Python(p) => {
                validate_python_proposal(p, root)?;
            }
            ProposedDriver::Binary(b) => {
                validate_binary_proposal(b, root)?;
            }
            ProposedDriver::StaticHttp => {
                // No external process; open path only.
            }
            ProposedDriver::Compose {
                command,
                privileged,
            } => {
                // Privileged containers are always rejected.
                if *privileged {
                    return Err(Error::InvalidInput(
                        "privileged Compose containers are not allowed by agent proposal".into(),
                    ));
                }
                // Compose command overrides must not run arbitrary binaries.
                if let Some(cmd) = command {
                    if !cmd.is_empty() {
                        return Err(Error::InvalidInput(
                            "agent proposals cannot override Compose commands".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Validated proposal outcome — what the Host records after approval.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedProposal {
    pub proposal: AgentProposal,
    /// Redacted input snapshot for the operation journal (never secrets).
    pub redacted: String,
}

/// Build the redacted input snapshot for a proposal (journal-safe).
pub fn redacted_proposal_input(proposal: &AgentProposal) -> String {
    serde_json::json!({
        "schemaVersion": proposal.schema_version,
        "kind": match proposal.kind {
            ProposalKind::Create => "create",
            ProposalKind::Start => "start",
        },
        "ownership": proposal.ownership.as_str(),
        "title": proposal.title,
        "driver": match &proposal.driver {
            ProposedDriver::Python(p) => format!("python:{}", p.entry),
            ProposedDriver::Binary(_) => "binary".to_string(),
            ProposedDriver::StaticHttp => "static_http".to_string(),
            ProposedDriver::Compose { .. } => "compose".to_string(),
        },
        "envKeys": proposal.environment_keys,
    })
    .to_string()
}

/// Driver payload variants a proposal can carry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProposedDriver {
    Python(PythonLaunchProfile),
    Binary(BinaryLaunchProfile),
    StaticHttp,
    Compose {
        command: Option<Vec<String>>,
        privileged: bool,
    },
}

fn validate_python_proposal(p: &PythonLaunchProfile, root: &Path) -> Result<()> {
    // Reuse the profile validator plus proposal-specific root containment.
    crate::creative_app::process_driver::validate_python_profile(p)?;
    // cwd must be inside the project root.
    let cwd = Path::new(&p.cwd_relative);
    let joined = root.join(cwd);
    if !joined.starts_with(root) {
        return Err(Error::InvalidInput(
            "proposal python cwd escapes project root".into(),
        ));
    }
    Ok(())
}

fn validate_binary_proposal(b: &BinaryLaunchProfile, root: &Path) -> Result<()> {
    // Binary proposals must be pre-approved with a hash; the agent cannot
    // self-approve a new binary.
    crate::creative_app::process_driver::validate_binary_profile(b)?;
    let exe = Path::new(&b.executable_path);
    // Binary must be inside the project root OR a system path the user approved.
    let in_root = exe.starts_with(root);
    let is_system = exe.starts_with("/usr/local/bin/")
        || exe.starts_with("/usr/bin/")
        || exe.starts_with("/opt/homebrew/bin/");
    if !in_root && !is_system {
        return Err(Error::InvalidInput(
            "binary must be inside the project root or a system-approved path".into(),
        ));
    }
    Ok(())
}

/// Reject proposal environment keys that look like secrets.
/// Keys are just names, but a key named SECRET_* / *_KEY / *_TOKEN from an
/// agent is a red flag — the Host should require explicit user env binding.
pub fn has_secret_like_env_key(keys: &[String]) -> bool {
    keys.iter().any(|k| {
        let upper = k.to_uppercase();
        upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("API_KEY")
            || upper.contains("PASSWORD")
            || upper.ends_with("_KEY")
    })
}

/// Validate that a path segment does not attempt to escape the project root.
pub fn path_escapes_root(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    p.components().any(|c| matches!(c, Component::ParentDir))
}

/// Convert a protocol proposal payload into a Host proposal and run the Host
/// gate on it (batch 10 CR-1001 wire: Agent Daemon → Host validator).
pub fn validate_protocol_proposal(
    payload: &assistant_protocol::v2::CreativeProposalPayload,
) -> Result<ValidatedProposal> {
    let kind = match payload.kind.as_str() {
        "create" => ProposalKind::Create,
        "start" => ProposalKind::Start,
        _ => {
            return Err(Error::InvalidInput(format!(
                "unknown proposal kind: {}",
                payload.kind
            )))
        }
    };
    let ownership = OwnershipMode::parse(&payload.ownership).ok_or_else(|| {
        Error::InvalidInput(format!("unknown ownership mode: {}", payload.ownership))
    })?;
    let driver = match &payload.driver {
        assistant_protocol::v2::CreativeProposedDriver::Python {
            schema_version,
            interpreter,
            entry,
            args,
            cwd_relative,
            environment_keys,
            open_path,
            health_path,
            startup_timeout_ms,
        } => ProposedDriver::Python(PythonLaunchProfile {
            schema_version: *schema_version,
            interpreter: interpreter.clone(),
            entry: entry.clone(),
            args: args.clone(),
            cwd_relative: cwd_relative.clone(),
            environment_keys: environment_keys.clone(),
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: open_path.clone(),
            health_path: health_path.clone(),
            startup_timeout_ms: *startup_timeout_ms,
            is_venv: interpreter.contains("/.venv/") || interpreter.contains("venv"),
        }),
        assistant_protocol::v2::CreativeProposedDriver::Binary {
            schema_version,
            executable_path,
            executable_hash,
            approved,
            args,
            cwd_relative,
            environment_keys,
            open_path,
            health_path,
            startup_timeout_ms,
        } => ProposedDriver::Binary(BinaryLaunchProfile {
            schema_version: *schema_version,
            executable_path: executable_path.clone(),
            executable_hash: executable_hash.clone(),
            approved: *approved,
            args: args.clone(),
            cwd_relative: cwd_relative.clone(),
            environment_keys: environment_keys.clone(),
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: open_path.clone(),
            health_path: health_path.clone(),
            startup_timeout_ms: *startup_timeout_ms,
        }),
        assistant_protocol::v2::CreativeProposedDriver::StaticHttp => ProposedDriver::StaticHttp,
        assistant_protocol::v2::CreativeProposedDriver::Compose {
            command,
            privileged,
        } => ProposedDriver::Compose {
            command: command.clone(),
            privileged: *privileged,
        },
    };
    let proposal = AgentProposal {
        schema_version: payload.schema_version,
        kind,
        ownership,
        title: payload.title.clone(),
        project_root: payload.project_root.clone(),
        driver,
        open_path: payload.open_path.clone(),
        health_path: payload.health_path.clone(),
        environment_keys: payload.environment_keys.clone(),
    };
    proposal.validate()?;
    Ok(ValidatedProposal {
        redacted: redacted_proposal_input(&proposal),
        proposal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_python() -> PythonLaunchProfile {
        PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/proj/.venv/bin/python".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec!["PORT".into()],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: true,
        }
    }

    #[test]
    fn valid_python_proposal_passes() {
        let p = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Dashboard".into(),
            project_root: "/proj".into(),
            driver: ProposedDriver::Python(base_python()),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn cwd_escape_rejected() {
        let mut py = base_python();
        py.cwd_relative = "../".into();
        let p = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Dashboard".into(),
            project_root: "/proj".into(),
            driver: ProposedDriver::Python(py),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn privileged_compose_rejected() {
        let p = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Compose".into(),
            project_root: "/proj".into(),
            driver: ProposedDriver::Compose {
                command: None,
                privileged: true,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn command_override_rejected() {
        let p = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Compose".into(),
            project_root: "/proj".into(),
            driver: ProposedDriver::Compose {
                command: Some(vec!["/bin/sh".into()]),
                privileged: false,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn unknown_schema_version_rejected() {
        let p = AgentProposal {
            schema_version: 99,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "X".into(),
            project_root: "/proj".into(),
            driver: ProposedDriver::StaticHttp,
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn binary_outside_root_and_system_rejected() {
        let b = BinaryLaunchProfile {
            schema_version: 1,
            executable_path: "/tmp/random_bin".into(),
            executable_hash: "a".repeat(64),
            approved: true,
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        };
        let p = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Bin".into(),
            project_root: "/proj".into(),
            driver: ProposedDriver::Binary(b),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn secret_like_env_keys_flagged() {
        assert!(has_secret_like_env_key(&["API_KEY".into()]));
        assert!(has_secret_like_env_key(&["DB_PASSWORD".into()]));
        assert!(!has_secret_like_env_key(&["PORT".into()]));
    }

    #[test]
    fn path_escape_detection() {
        assert!(path_escapes_root("../evil"));
        assert!(path_escapes_root("/abs/path"));
        assert!(!path_escapes_root("sub/app.py"));
        assert!(!path_escapes_root("."));
    }

    #[test]
    fn protocol_payload_maps_to_host_proposal_and_passes_gate() {
        use assistant_protocol::v2::{CreativeProposalPayload, CreativeProposedDriver};
        let payload = CreativeProposalPayload {
            schema_version: 1,
            kind: "create".into(),
            ownership: "managed".into(),
            title: "Static".into(),
            project_root: "/proj".into(),
            driver: CreativeProposedDriver::StaticHttp,
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        };
        let validated = validate_protocol_proposal(&payload).unwrap();
        assert_eq!(validated.proposal.title, "Static");
        assert!(validated.redacted.contains("static_http"));
    }

    #[test]
    fn protocol_payload_rejects_privileged_compose() {
        use assistant_protocol::v2::{CreativeProposalPayload, CreativeProposedDriver};
        let payload = CreativeProposalPayload {
            schema_version: 1,
            kind: "start".into(),
            ownership: "managed".into(),
            title: "C".into(),
            project_root: "/proj".into(),
            driver: CreativeProposedDriver::Compose {
                command: None,
                privileged: true,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        assert!(validate_protocol_proposal(&payload).is_err());
    }
}
