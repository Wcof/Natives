use serde::Serialize;

/// Standardized error response for all Daemon API calls.
#[derive(Debug, Clone, Serialize)]
pub struct DaemonError {
    /// Machine-readable error code.
    pub code: String,
    /// Error category for UI routing.
    pub category: ErrorCategory,
    /// Whether the operation can be retried.
    pub retryable: bool,
    /// i18n key for user-facing message.
    pub user_message_key: String,
    /// Technical detail (not shown to user).
    pub technical_message: String,
    /// Suggested recovery actions.
    pub recovery_actions: Vec<String>,
    /// Correlation ID for tracing.
    pub correlation_id: String,
}

/// Error category for routing to appropriate UI handling.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// Validation error (bad input).
    Validation,
    /// Authentication / authorization failure.
    Auth,
    /// Resource not found.
    NotFound,
    /// Conflict (e.g. duplicate, stale version).
    Conflict,
    /// Rate limit hit.
    RateLimited,
    /// Timeout.
    Timeout,
    /// Internal server error.
    Internal,
    /// Provider returned an error.
    Provider,
    /// Network error.
    Network,
    /// Permission denied.
    PermissionDenied,
    /// Unsupported feature/protocol version.
    Unsupported,
    /// Extension/plugin error.
    Extension,
}

impl DaemonError {
    pub fn new(
        code: impl Into<String>,
        category: ErrorCategory,
        retryable: bool,
        technical_message: impl Into<String>,
    ) -> Self {
        let code_str = code.into();
        DaemonError {
            user_message_key: format!("error.{}", code_str),
            correlation_id: uuid::Uuid::new_v4().to_string(),
            code: code_str,
            category,
            retryable,
            technical_message: technical_message.into(),
            recovery_actions: Vec::new(),
        }
    }

    pub fn with_recovery(mut self, actions: Vec<String>) -> Self {
        self.recovery_actions = actions;
        self
    }

    pub fn with_user_key(mut self, key: impl Into<String>) -> Self {
        self.user_message_key = key.into();
        self
    }
}

/// Common error codes.
pub mod error_codes {
    pub const INVALID_INPUT: &str = "invalid_input";
    pub const NOT_FOUND: &str = "not_found";
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const PERMISSION_DENIED: &str = "permission_denied";
    pub const RATE_LIMITED: &str = "rate_limited";
    pub const TIMEOUT: &str = "timeout";
    pub const INTERNAL_ERROR: &str = "internal_error";
    pub const PROVIDER_ERROR: &str = "provider_error";
    pub const NETWORK_ERROR: &str = "network_error";
    pub const PROTOCOL_INCOMPATIBLE: &str = "protocol_incompatible";
    pub const RUN_NOT_FOUND: &str = "run_not_found";
    pub const CONVERSATION_NOT_FOUND: &str = "conversation_not_found";
    pub const RUN_INVALID_STATE: &str = "run_invalid_state";
    pub const EXTENSION_CRASHED: &str = "extension_crashed";
    pub const TOOL_EXECUTION_FAILED: &str = "tool_execution_failed";
    pub const CONTEXT_OVERFLOW: &str = "context_overflow";
    pub const BUDGET_EXCEEDED: &str = "budget_exceeded";
    pub const MIGRATION_IN_PROGRESS: &str = "migration_in_progress";
}