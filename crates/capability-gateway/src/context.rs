//! Tool execution context and verified-path types.
//!
//! `ToolCallContext` is the verified identity + run identity handed to every
//! tool handler; `TrustedPath` is the only path representation a tool may
//! consume (a raw caller path cannot be represented). The bounded progress
//! channel types live here with the context that owns them.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use super::contract::ToolError;

/// Bounded live-output channel capacity for long-running handlers (H03). A
/// slow consumer must not grow memory unboundedly: overflow is dropped at the
/// producer and counted in `ToolCallContext::progress_dropped_bytes`.
pub const TERMINAL_PROGRESS_CAPACITY: usize = 4096;

/// Best-effort output emitted by a long-running handler. The Gateway owns the
/// process/MCP safety boundary; the caller owns persistence and rate limiting.
#[derive(Debug, Clone)]
pub struct ToolProgressChunk {
    pub stream: String,
    pub text: String,
}

/// Context passed to tool handlers during execution.
///
/// Must be constructed from a verified ProjectIdentity (task-10). Do not
/// Context for a tool call execution.
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    /// The project root directory (canonical, absolute).
    pub project_root: PathBuf,
    /// The current working directory for the tool call.
    pub working_dir: PathBuf,
    /// The run ID for this execution.
    pub run_id: String,
    /// The conversation ID.
    pub conversation_id: String,
    /// The tool call ID (from provider).
    pub tool_call_id: String,
    /// Permission profile for this run.
    pub permission_profile: String,
    /// Stable project identity UUID (required for privileged tools).
    pub project_id: Option<String>,
    /// Identity version at verification time.
    pub project_identity_version: Option<u32>,
    /// Shared run cancellation token (task-03). Tools/MCP must select on this.
    pub cancel: CancellationToken,
    /// Optional live output channel for handlers that can stream progress.
    /// Bounded (`TERMINAL_PROGRESS_CAPACITY`); overflow is dropped and counted
    /// in `progress_dropped_bytes`. `None` keeps lightweight/test handlers
    /// allocation-free.
    pub progress: Option<Sender<ToolProgressChunk>>,
    /// Bytes of live progress dropped because the bounded channel was full.
    /// Progress is non-authoritative; overflow must never stall the handler.
    pub progress_dropped_bytes: Arc<AtomicU64>,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
}

impl ToolCallContext {
    /// Create a new context with required fields.
    ///
    /// Prefer [`Self::from_verified_identity`] when a ProjectIdentity is available.
    pub fn new(
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
    ) -> Self {
        Self::with_cancel(
            project_root,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            CancellationToken::new(),
        )
    }

    /// Context bound to a registry-owned cancel token.
    pub fn with_cancel(
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
        cancel: CancellationToken,
    ) -> Self {
        let working_dir = project_root.clone();
        Self {
            project_root,
            working_dir,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            project_id: None,
            project_identity_version: None,
            cancel,
            progress: None,
            progress_dropped_bytes: Arc::new(AtomicU64::new(0)),
            turn_id: None,
            message_id: None,
        }
    }

    /// Attach a bounded live-output channel and its dropped-bytes counter. The
    /// counter is shared with the daemon so overflow is observable.
    pub fn set_progress(
        &mut self,
        progress: Option<Sender<ToolProgressChunk>>,
        dropped_bytes: Arc<AtomicU64>,
    ) {
        self.progress = progress;
        self.progress_dropped_bytes = dropped_bytes;
    }

    /// Construct context from a verified project identity (task-10).
    pub fn from_verified_identity(
        project_id: impl Into<String>,
        identity_version: u32,
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
    ) -> Self {
        Self::from_verified_identity_with_cancel(
            project_id,
            identity_version,
            project_root,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            CancellationToken::new(),
        )
    }

    /// Verified identity + registry-owned cancel token.
    #[allow(clippy::too_many_arguments)] // public API: identity fields are fixed
    pub fn from_verified_identity_with_cancel(
        project_id: impl Into<String>,
        identity_version: u32,
        project_root: PathBuf,
        run_id: String,
        conversation_id: String,
        tool_call_id: String,
        permission_profile: String,
        cancel: CancellationToken,
    ) -> Self {
        let working_dir = project_root.clone();
        Self {
            project_root,
            working_dir,
            run_id,
            conversation_id,
            tool_call_id,
            permission_profile,
            project_id: Some(project_id.into()),
            project_identity_version: Some(identity_version),
            cancel,
            progress: None,
            progress_dropped_bytes: Arc::new(AtomicU64::new(0)),
            turn_id: None,
            message_id: None,
        }
    }

    /// Resolve a path relative to working_dir, then validate it's within project_root.
    pub fn resolve_path(&self, input_path: &str) -> Result<PathBuf, ToolError> {
        use std::path::Path;
        let path = Path::new(input_path);
        let resolved = if path.is_relative() {
            self.working_dir.join(path)
        } else {
            path.to_path_buf()
        };
        // Canonicalize if exists, otherwise check parent
        let canonical = if resolved.exists() {
            resolved.canonicalize().map_err(|e| ToolError {
                code: "PATH_ERROR".into(),
                message: format!("Failed to canonicalize path: {e}"),
                retryable: false,
            })?
        } else {
            // For new files, canonicalize parent and append filename
            let parent = resolved.parent().unwrap_or(Path::new("."));
            let file_name = resolved.file_name().ok_or_else(|| ToolError {
                code: "PATH_ERROR".into(),
                message: "Invalid path: no filename".into(),
                retryable: false,
            })?;
            let canonical_parent = parent.canonicalize().map_err(|e| ToolError {
                code: "PATH_ERROR".into(),
                message: format!("Failed to canonicalize parent path: {e}"),
                retryable: false,
            })?;
            canonical_parent.join(file_name)
        };
        // Verify within project root
        if !canonical.starts_with(&self.project_root) {
            return Err(ToolError {
                code: "PATH_ESCAPE".into(),
                message: format!(
                    "Path {} escapes project root {}",
                    canonical.display(),
                    self.project_root.display()
                ),
                retryable: false,
            });
        }
        Ok(canonical)
    }
}

/// A path authorized by the Gateway for checkpoint/rewind I/O.
///
/// Produced only by [`CapabilityGateway::preflight_write_paths`]: `canonical`
/// is verified to be inside the project root, and `project_relative` is the
/// in-root form used as the checkpoint key and rewind path. Checkpoint must
/// consume only this type — a raw caller-supplied path cannot be represented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPath {
    /// Absolute canonical path on disk.
    pub canonical: PathBuf,
    /// Path relative to the project root.
    pub project_relative: PathBuf,
}

impl TrustedPath {
    pub fn new(canonical: PathBuf, project_relative: PathBuf) -> Self {
        Self {
            canonical,
            project_relative,
        }
    }
}
