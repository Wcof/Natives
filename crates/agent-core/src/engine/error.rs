//! Engine error type shared by the loop, provider and tool seams.

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
    #[error("{message}")]
    Provider {
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    },
    #[error("cancelled")]
    Cancelled,
    /// The detector's verdict travels with the error. "doom loop detected" on
    /// its own tells a user nothing they can act on; the reason names the
    /// signal, the cycle length and the repeating steps.
    #[error("doom loop detected: {0}")]
    DoomLoop(crate::doom_loop::DoomLoopReason),
    #[error("max steps exceeded")]
    MaxSteps,
    /// The authoritative completion fact AND the uncertain ledger recording
    /// both failed to persist. The run must terminate with a `recovery_blocked`
    /// code so a later resume never assumes a side effect it cannot prove.
    #[error("recovery_blocked: {0}")]
    RecoveryBlocked(String),
    /// An observation-only post hook (PostToolUse / PostToolUseFailure /
    /// PostCompact) returned a decision that cannot be honoured after the
    /// observed outcome was committed. The refusal is surfaced loudly instead
    /// of being silently ignored (T03).
    #[error("hook_refused: {0}")]
    HookRefused(String),
}

impl EngineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Provider { code, .. } => code,
            Self::Cancelled => "cancelled",
            Self::DoomLoop(_) => "doom_loop",
            Self::MaxSteps => "max_steps",
            Self::Message(_) => "provider",
            Self::RecoveryBlocked(_) => "recovery_blocked",
            Self::HookRefused(_) => "hook_refused",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Provider {
                retryable: true,
                ..
            }
        )
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Provider { category, .. } if category == "RateLimit")
    }

    /// Provider-neutral context-overflow classification (TASK-011 / C03-I03).
    /// Adapters map overflow to `ProviderErrorCategory::ContextLengthExceeded`;
    /// the engine uses this to trigger a bounded one-shot compaction retry,
    /// never an unbounded retry loop.
    pub fn is_context_overflow(&self) -> bool {
        matches!(
            self,
            Self::Provider { category, .. }
                if category == "ContextLengthExceeded"
                    || category == "ContextWindowExceeded"
        )
    }

    /// Delay the provider asked us to wait, if it named one.
    ///
    /// This is the value the retry loop feeds into [`provider_backoff_ms`]; it
    /// used to be carried on the error and never read, which is how a 429 could
    /// be retried three times inside two seconds.
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::Provider { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        }
    }
}
