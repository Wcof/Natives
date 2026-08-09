//! Sub-agent domain types — config, status, and failure policy.

/// Failure propagation for child batches (task-11, T05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailurePolicy {
    /// Parent receives child failure; siblings continue (default).
    #[default]
    Isolate,
    /// One failure cancels siblings; parent fails.
    FailFast,
    /// Wait for all; any failure makes aggregate fail.
    RequireAll,
    /// Re-queue a failed child up to `max_retries`; then isolate.
    Retry,
}

impl FailurePolicy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "fail_fast" | "failfast" => Self::FailFast,
            "require_all" | "requireall" => Self::RequireAll,
            "retry" | "retry_failed" => Self::Retry,
            _ => Self::Isolate,
        }
    }

    /// Stable wire name for persistence (inverse of [`Self::parse`]).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Isolate => "isolate",
            Self::FailFast => "fail_fast",
            Self::RequireAll => "require_all",
            Self::Retry => "retry",
        }
    }

    /// What a terminal child *failure* must do to the parent run.
    ///
    /// This is the single decision the production watcher consumes (T05).
    /// `all_siblings_terminal` is true when every sibling child of the same
    /// parent has already settled — only [`FailurePolicy::RequireAll`] cares.
    /// `retries_remaining` is `max_retries.saturating_sub(retry_count)`.
    pub fn on_child_failed(
        &self,
        all_siblings_terminal: bool,
        retries_remaining: u32,
    ) -> ChildFailureEffect {
        match self {
            FailurePolicy::Isolate => ChildFailureEffect::Isolate,
            FailurePolicy::FailFast => ChildFailureEffect::FailParent,
            FailurePolicy::RequireAll if all_siblings_terminal => ChildFailureEffect::FailParent,
            FailurePolicy::RequireAll => ChildFailureEffect::Isolate,
            FailurePolicy::Retry if retries_remaining > 0 => ChildFailureEffect::Retry,
            FailurePolicy::Retry => ChildFailureEffect::Isolate,
        }
    }
}

/// Concrete parent-side action a child failure triggers under a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildFailureEffect {
    /// Parent observes `SubagentFailed` and continues; siblings unaffected.
    Isolate,
    /// Parent run fails now (and sibling children are cancelled).
    FailParent,
    /// Re-queue the child on the same hidden conversation.
    Retry,
}

#[derive(Debug, Clone)]
pub struct SubAgentConfig {
    /// Backward-compatible alias for [`Self::max_concurrent_global`].
    pub max_concurrent: u32,
    pub max_concurrent_global: u32,
    pub max_concurrent_per_parent: u32,
    pub max_tasks_per_parent_total: u32,
    pub max_depth: u32,
    pub max_tokens_per_sub: u64,
    pub max_tokens_per_child: u64,
    pub max_tokens_per_tree: u64,
    pub max_tool_calls_per_child: u32,
    pub max_tool_calls_per_tree: u32,
    pub child_timeout_ms: u64,
    pub failure_policy: FailurePolicy,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        SubAgentConfig {
            max_concurrent: 3,
            max_concurrent_global: 3,
            max_concurrent_per_parent: 3,
            max_tasks_per_parent_total: 32,
            max_depth: 5,
            max_tokens_per_sub: 100_000,
            max_tokens_per_child: 100_000,
            max_tokens_per_tree: 500_000,
            max_tool_calls_per_child: 200,
            max_tool_calls_per_tree: 1_000,
            child_timeout_ms: 600_000,
            failure_policy: FailurePolicy::Isolate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: String,
    pub run_id: String,
    pub parent_run_id: String,
    pub agent_profile_id: Option<String>,
    pub provider_id: String,
    pub key_id: String,
    pub model_id: String,
    pub permission_profile: String,
    pub tool_allowlist: Vec<String>,
    pub task: String,
    pub status: SubAgentStatus,
    pub depth: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub isolation_mode: String,
    pub working_directory: Option<String>,
    pub resume_from: Option<String>,
}

/// Sub-agent status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentStatus {
    Queued,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}
