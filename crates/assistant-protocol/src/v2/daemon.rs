//! Daemon sidecar status, readiness, recovery codes and protocol-mismatch
//! contract (W2 freeze, contract §11.3.1 / §11.3.6).
//!
//! Rules:
//! - `DaemonStatusV2` must NEVER carry database paths or secrets. Readiness is
//!   decomposed (no single-boolean health gate) so the Host can decide which
//!   subsystem is degraded.
//! - `RecoveryCode` is the typed R0–R4 recovery ladder used by the Host
//!   supervisor; the Daemon never writes run status through the Host.
//! - `ProtocolMismatch` is explicit: old Host/new Daemon or new Host/old
//!   Daemon must never silently fall back to an unsafe field interpretation.

use serde::{Deserialize, Serialize};

/// Readiness of one daemon subsystem. Each is reported independently so a
/// degraded credential broker does not masquerade as a healthy daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessItem {
    Ready,
    Degraded(String),
    Unavailable(String),
}

impl ReadinessItem {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Sidecar status returned by `daemon.getStatus` (v2).
///
/// Deliberately contains NO db path, NO key id, NO secret. The Host uses
/// `instance_id` + `protocol_version` to detect a stale or foreign sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatusV2 {
    pub instance_id: String,
    pub protocol_version: String,
    pub health: ReadinessItem,
    pub active_runs: u32,
    pub storage_ready: ReadinessItem,
    pub credential_broker_ready: ReadinessItem,
    /// Subsystem detail list (redacted); empty when everything is ready.
    #[serde(default)]
    pub degraded: Vec<String>,
}

/// Typed recovery ladder (R0–R4). `RecoveryCode` is the machine-readable
/// contract between the Host supervisor and the Daemon's run authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryCode {
    /// Watch stream stale but process/ping healthy: resubscribe from cursor.
    R0WatchStale,
    /// Daemon healthy but active-run invariant inconsistent: user decides
    /// wait / diagnose / terminate. Never auto-cancel on missing text tokens.
    R1RunStalled,
    /// RPC stale: one bounded reconnect, then one bounded restart.
    R2RpcStale,
    /// Daemon process exited: restart, mark runs interrupted, recover via
    /// RunManager/EventLog authority.
    R3ProcessExited,
    /// Recovery budget exhausted: Faulted, keep diagnostics and explicit
    /// termination entry.
    R4Exhausted,
}

impl RecoveryCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R0WatchStale => "r0_watch_stale",
            Self::R1RunStalled => "r1_run_stalled",
            Self::R2RpcStale => "r2_rpc_stale",
            Self::R3ProcessExited => "r3_process_exited",
            Self::R4Exhausted => "r4_exhausted",
        }
    }
}

/// Explicit protocol mismatch between Host and Daemon versions. Returning this
/// instead of a generic error forces the caller to surface an upgrade path;
/// it is never silently absorbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolMismatch {
    pub host_protocol: String,
    pub daemon_protocol: String,
    pub compatible: bool,
    /// Human-readable reason (no secrets, no paths).
    pub reason: String,
}

impl ProtocolMismatch {
    pub fn new(
        host_protocol: impl Into<String>,
        daemon_protocol: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let host = host_protocol.into();
        let daemon = daemon_protocol.into();
        let compatible = host.split('.').next() == daemon.split('.').next();
        Self {
            host_protocol: host,
            daemon_protocol: daemon,
            compatible,
            reason: reason.into(),
        }
    }
}
