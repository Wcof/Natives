//! Tool contract types for the capability gateway.
//!
//! Registration metadata every tool must declare (schema, side effect,
//! permission class, path scope, timeout, output limit, cancellation) plus the
//! handler seam and its result/error types.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::context::ToolCallContext;

/// A registered tool.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: serde_json::Value,
    pub side_effect: SideEffect,
    pub permission_class: PermissionClass,
    pub path_scope: PathScope,
    pub timeout_ms: u64,
    pub output_limit: u64,
    pub cancellable: bool,
    /// Explicit scheduling declaration. `false` by default so a tool is
    /// Sequential unless its author proves it safe to run concurrently with
    /// other tools (read-only file search/read tools). Never inferred from
    /// `SideEffect` — "read-only" does not imply parallel-safe.
    pub parallel_safe: bool,
    /// Tools that must not run concurrently with each other share a key.
    pub conflict_key: Option<String>,
    /// Idempotency contract (W5): whether a retry after an uncertain outcome is
    /// safe. `None` means the tool declares nothing — the engine MUST NOT retry
    /// unknown outcomes automatically (side-effect ledger rule).
    pub idempotency: Option<Idempotency>,
    /// Per-call resource ceiling (W5): bytes of output a single call may write.
    /// The gateway truncates above this bound and marks `ToolOutput.truncated`.
    pub per_call_resource: Option<PerCallResource>,
    pub handler: Arc<dyn ToolHandler + Send + Sync>,
}

/// Idempotency contract (W5). Unknown outcomes are never auto-retried unless
/// the tool declares `Idempotent` (or `RetrySafeOnFailure` for failures only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Idempotency {
    /// Re-running with the same input is safe (result identical).
    Idempotent,
    /// Re-running after a *failed* attempt is safe; unknown outcomes are not.
    RetrySafeOnFailure,
}

/// Per-call resource budget (W5): output byte cap plus an optional explicit
/// concurrency weight used by the resource scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerCallResource {
    pub max_output_bytes: u64,
    #[serde(default)]
    pub concurrency_weight: u8,
}

/// Tool side effect classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideEffect {
    ReadOnly,
    Write,
    Destructive,
    Network,
    Process,
}

/// Scheduling declaration owned by the Gateway. Core consumes this metadata
/// but never infers concurrency from tool names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    ParallelSafe,
    Sequential,
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCapability {
    pub name: String,
    pub schema: serde_json::Value,
    pub execution_mode: ExecutionMode,
    pub side_effect: SideEffect,
    pub conflict_key: Option<String>,
}

/// Permission class for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionClass {
    AlwaysAllowed,
    ProjectRead,
    ProjectWrite,
    ExternalWrite,
    Credentials,
    Elevation,
    DestructiveCommand,
    PrivacyResource,
}

/// Path scope restriction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathScope {
    None,
    Project(String),
    Glob(String),
    Any,
}

/// Tool handler trait.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError>;
}

/// Tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub result: serde_json::Value,
    pub truncated: bool,
    pub duration_ms: u64,
}

/// Tool error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
