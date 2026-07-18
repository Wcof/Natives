use serde::{Deserialize, Serialize};
use std::fmt;

/// Protocol version identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ProtocolVersion {
    /// Wire protocol major for Agent Daemon RPC (aligned with `v2::PROTOCOL_V2`).
    pub const CURRENT: ProtocolVersion = ProtocolVersion {
        major: 2,
        minor: 0,
        patch: 0,
    };

    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        ProtocolVersion { major, minor, patch }
    }

    /// Returns true if `self` is compatible with `other` (same major version).
    pub fn is_compatible_with(&self, other: &ProtocolVersion) -> bool {
        self.major == other.major
    }

    /// Returns true if `self` is strictly greater than `other`.
    pub fn is_newer_than(&self, other: &ProtocolVersion) -> bool {
        (self.major, self.minor, self.patch) > (other.major, other.minor, other.patch)
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl From<&str> for ProtocolVersion {
    fn from(s: &str) -> Self {
        let parts: Vec<&str> = s.split('.').collect();
        ProtocolVersion {
            major: parts.get(0).and_then(|p| p.parse().ok()).unwrap_or(0),
            minor: parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0),
            patch: parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0),
        }
    }
}

/// Result of version negotiation between client and daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionNegotiation {
    pub client_version: ProtocolVersion,
    pub daemon_version: ProtocolVersion,
    pub compatible: bool,
    pub upgrade_required: Option<String>,
}

/// Version negotiation logic.
pub fn negotiate(client_version: &ProtocolVersion, daemon_version: &ProtocolVersion) -> VersionNegotiation {
    let compatible = client_version.is_compatible_with(daemon_version);
    let upgrade_required = if !compatible {
        Some(format!(
            "Client protocol v{} is incompatible with daemon v{}. Please upgrade.",
            client_version, daemon_version
        ))
    } else if daemon_version.is_newer_than(client_version) {
        Some(format!(
            "Daemon protocol v{} is newer than client v{}. Consider upgrading the client.",
            daemon_version, client_version
        ))
    } else {
        None
    };

    VersionNegotiation {
        client_version: client_version.clone(),
        daemon_version: daemon_version.clone(),
        compatible,
        upgrade_required,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compatible_versions() {
        let v1 = ProtocolVersion::new(2, 0, 0);
        let v2 = ProtocolVersion::new(2, 1, 0);
        assert!(v1.is_compatible_with(&v2));
        assert!(v2.is_compatible_with(&v1));
    }

    #[test]
    fn test_incompatible_major_versions() {
        let v1 = ProtocolVersion::new(2, 0, 0);
        let v2 = ProtocolVersion::new(1, 0, 0);
        assert!(!v1.is_compatible_with(&v2));
    }

    #[test]
    fn test_negotiation_compatible() {
        let result = negotiate(&ProtocolVersion::new(2, 0, 0), &ProtocolVersion::new(2, 0, 0));
        assert!(result.compatible);
        assert!(result.upgrade_required.is_none());
    }

    #[test]
    fn test_negotiation_incompatible() {
        let result = negotiate(&ProtocolVersion::new(0, 1, 0), &ProtocolVersion::new(2, 0, 0));
        assert!(!result.compatible);
        assert!(result.upgrade_required.is_some());
    }

    #[test]
    fn test_display_format() {
        let v = ProtocolVersion::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_from_str() {
        let v: ProtocolVersion = "2.0.0".into();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn current_matches_v2_constant() {
        assert_eq!(ProtocolVersion::CURRENT.to_string(), "2.0.0");
    }
}