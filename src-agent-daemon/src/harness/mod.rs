//! Harness control plane — the Daemon half.
//!
//! `harness-core` owns what a Harness *is*. This module owns where it is
//! stored, how it is resolved for a Run, and how it is projected onto the
//! `harness.*` protocol surface.
//!
//! ## What this module exists to answer
//!
//! Two user questions drove the design (`docs/superpowers/specs/
//! 2026-07-26-native-harness-control-plane-design.md`):
//!
//! 1. *What does the execution engine look like at each stage?* →
//!    [`projection::topology`], which pairs the code-fixed
//!    `harness_core::topology` with the Hooks actually attached to each point.
//! 2. *Which stage uses a Hook, and which Hook?* → [`projection::hook_catalog`],
//!    a projection of `HookRegistry::describe()` — the same data dispatch
//!    consults, so the catalog cannot misreport what runs.
//!
//! Everything a caller sees traces back to one of three real sources: the
//! constant topology, on-disk Hook discovery, or a row in `assistant.db`.
//! Nothing is synthesised to make a screen look complete.
//!
//! ## What is deliberately absent
//!
//! Per-invocation telemetry — durations, exit codes, output previews — is
//! Phase 3 by the design's own sequencing (第 19 节) and is not stubbed here.
//! `harness.run.getSnapshot` reports honestly that a Run has no snapshot until
//! [`control_plane::resolve_run`] is called from the Run start path; see that
//! function's docs for the single remaining wiring step.

pub mod control_plane;
pub mod projection;
pub mod repository;

use assistant_protocol::error::ErrorCategory;
use serde_json::Value;

/// Structured failure, mapped to a protocol error by the RPC layer.
///
/// Codes are lowercase snake_case to match every other wire code in
/// `assistant_protocol::error::error_codes`; the design's `HARNESS_DRAFT_CONFLICT`
/// spelling is the logical name of the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError {
    pub code: &'static str,
    pub category: ErrorCategory,
    pub message: String,
}

impl HarnessError {
    fn new(code: &'static str, category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            code,
            category,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", ErrorCategory::Validation, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", ErrorCategory::NotFound, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", ErrorCategory::Internal, message)
    }

    pub fn draft_conflict(message: impl Into<String>) -> Self {
        Self::new("harness_draft_conflict", ErrorCategory::Conflict, message)
    }

    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self::new(
            "harness_validation_failed",
            ErrorCategory::Validation,
            message,
        )
    }

    pub fn snapshot_persist_failed(message: impl Into<String>) -> Self {
        Self::new(
            "harness_snapshot_persist_failed",
            ErrorCategory::Internal,
            message,
        )
    }
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Dispatch one `harness.*` method.
///
/// Kept as a single entry point so `rpc.rs` needs one arm for the whole family
/// and the advertised set and the dispatch table cannot drift apart.
///
/// The work runs on a blocking pool because it is genuinely blocking on two
/// counts: Hook discovery reads up to a dozen files, and SQLite is opened with
/// `busy_timeout=30000`, so a contended write can park the caller for half a
/// minute. Doing that on a Tokio worker would stall every other connection the
/// runtime is serving, which is a far worse failure than one slow RPC.
pub async fn request(method: &str, params: Value) -> Result<Value, HarnessError> {
    let method = method.to_string();
    tokio::task::spawn_blocking(move || control_plane::request(&method, params))
        .await
        .unwrap_or_else(|e| Err(HarnessError::internal(format!("harness task failed: {e}"))))
}

// ── shared parameter helpers ────────────────────────────────────────────────

/// Read an optional string parameter, accepting snake_case and camelCase.
pub(crate) fn opt_str(params: &Value, aliases: &[&str]) -> Option<String> {
    for key in aliases {
        if let Some(value) = params.get(*key).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Read a required string parameter.
pub(crate) fn req_str(params: &Value, aliases: &[&str]) -> Result<String, HarnessError> {
    opt_str(params, aliases)
        .ok_or_else(|| HarnessError::invalid(format!("{} is required", aliases[0])))
}

/// Read a bounded integer parameter with a default.
pub(crate) fn opt_i64(params: &Value, aliases: &[&str], default: i64, max: i64) -> i64 {
    for key in aliases {
        if let Some(value) = params.get(*key).and_then(Value::as_i64) {
            return value.clamp(1, max);
        }
    }
    default
}
