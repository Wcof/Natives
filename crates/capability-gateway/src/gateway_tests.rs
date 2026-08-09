//! Gateway unit tests (extracted from lib.rs).

use super::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod p0_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHandler(AtomicUsize);

    #[async_trait::async_trait]
    impl ToolHandler for CountingHandler {
        async fn execute(
            &self,
            _input: serde_json::Value,
            _context: &ToolCallContext,
        ) -> Result<ToolOutput, ToolError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput {
                result: serde_json::json!({"ok": true}),
                truncated: false,
                duration_ms: 0,
            })
        }
    }

    fn gateway(handler: Arc<dyn ToolHandler + Send + Sync>, timeout_ms: u64) -> CapabilityGateway {
        let mut gateway = CapabilityGateway::new();
        gateway.register(Tool {
            name: "p0_test",
            description: "test",
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}, "mode": {"type": "string", "enum": ["read"]}},
                "required": ["path"]
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::Any,
            timeout_ms,
            output_limit: 4096,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler,
        }).unwrap();
        gateway
    }

    fn context(cancel: CancellationToken) -> ToolCallContext {
        ToolCallContext::with_cancel(
            std::env::current_dir().unwrap(),
            "run-p0".into(),
            "conversation-p0".into(),
            "call-p0".into(),
            "readonly".into(),
            cancel,
        )
    }

    #[tokio::test]
    async fn schema_failure_never_reaches_handler() {
        let handler = Arc::new(CountingHandler(AtomicUsize::new(0)));
        let gateway = gateway(handler.clone(), 1000);
        let error = gateway
            .execute(
                "p0_test",
                serde_json::json!({"path": 7}),
                &context(CancellationToken::new()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_arguments");
        assert_eq!(handler.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn array_bounds_are_enforced_before_handler() {
        let handler = Arc::new(CountingHandler(AtomicUsize::new(0)));
        let mut gateway = CapabilityGateway::new();
        gateway.register(Tool {
            name: "bounded_array",
            description: "test",
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {"type": "array", "minItems": 1, "maxItems": 2, "items": {"type": "string"}}
                },
                "required": ["items"]
            }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::Any,
            timeout_ms: 1000,
            output_limit: 4096,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: handler.clone(),
        }).unwrap();
        let error = gateway
            .execute(
                "bounded_array",
                serde_json::json!({"items": []}),
                &context(CancellationToken::new()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_arguments");
        assert_eq!(handler.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn all_builtin_schemas_are_supported_by_validator() {
        let mut gateway = CapabilityGateway::new();
        let _ = gateway.register_builtins();
        gateway
            .validate_registered_schemas()
            .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    }

    #[test]
    fn every_tool_has_a_verifiable_mode_and_writes_are_not_parallel() {
        let mut gateway = CapabilityGateway::new();
        let _ = gateway.register_builtins();
        let capabilities = gateway.list_capabilities();
        assert!(!capabilities.is_empty(), "builtins must register");
        for capability in &capabilities {
            // Every tool has a real, explicit mode — never an inferred default
            // that could be mistaken for a missing declaration.
            match capability.execution_mode {
                ExecutionMode::ParallelSafe
                | ExecutionMode::Sequential
                | ExecutionMode::Exclusive => {}
            }
        }
        // Genuinely safe read-only file tools are explicitly parallel-safe.
        for name in ["read_file", "search_files", "list_dir", "grep"] {
            let cap = capabilities
                .iter()
                .find(|capability| capability.name == name)
                .unwrap_or_else(|| panic!("{name} must be registered"));
            assert_eq!(
                cap.execution_mode,
                ExecutionMode::ParallelSafe,
                "{name} must be explicitly parallel-safe"
            );
        }
        // write / shell / git / MCP / subagent tools must never be parallel.
        for name in [
            "write_file",
            "edit_file",
            "apply_patch",
            "run_terminal",
            "web_fetch",
            "task",
            "kill_task",
            "mcp_call",
            "write_draft_module",
            "rollback_draft_revision",
            "notification",
        ] {
            if let Some(cap) = capabilities
                .iter()
                .find(|capability| capability.name == name)
            {
                assert_ne!(
                    cap.execution_mode,
                    ExecutionMode::ParallelSafe,
                    "{name} must not be parallel-safe"
                );
            }
        }
    }

    #[tokio::test]
    async fn cancellation_wins_over_blocking_handler() {
        struct Blocking;
        #[async_trait::async_trait]
        impl ToolHandler for Blocking {
            async fn execute(
                &self,
                _: serde_json::Value,
                context: &ToolCallContext,
            ) -> Result<ToolOutput, ToolError> {
                tokio::select! {
                    _ = context.cancel.cancelled() => Err(ToolError {
                        code: "cancelled".into(),
                        message: "fake handler observed cancellation".into(),
                        retryable: false,
                    }),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => Ok(ToolOutput {
                        result: serde_json::json!({}),
                        truncated: false,
                        duration_ms: 0,
                    }),
                }
            }
        }
        let cancel = CancellationToken::new();
        let gateway = gateway(Arc::new(Blocking), 5000);
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            trigger.cancel();
        });
        let error = gateway
            .execute(
                "p0_test",
                serde_json::json!({"path": "ok"}),
                &context(cancel),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "cancelled");
    }
}

#[cfg(test)]
mod path_scope_preflight_tests {
    //! TASK-001 (N01): `preflight_write_paths` is the single authority that
    //! authorizes write paths for checkpoint I/O. Absolute, dotdot and symlink
    //! escapes must be rejected BEFORE any path is returned as trusted.
    use super::*;
    use std::path::Path;

    struct NoopHandler;
    #[async_trait::async_trait]
    impl ToolHandler for NoopHandler {
        async fn execute(
            &self,
            _input: serde_json::Value,
            _context: &ToolCallContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput {
                result: serde_json::json!({}),
                truncated: false,
                duration_ms: 0,
            })
        }
    }

    fn gateway_with_root(root: &Path) -> CapabilityGateway {
        // Production binds project roots from verified identity (canonical);
        // mirror that here so macOS `/var` → `/private/var` symlinks resolve.
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut gateway = CapabilityGateway::new();
        gateway.set_project_root(root.to_string_lossy().into_owned());
        let _ = gateway.register_builtins();
        gateway
    }

    fn context_for(root: &Path) -> ToolCallContext {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        ToolCallContext::with_cancel(
            root,
            "run-path".into(),
            "conv-path".into(),
            "call-path".into(),
            "autonomous".into(),
            CancellationToken::new(),
        )
    }

    fn assert_path_rejection(err: ToolError) {
        assert!(
            matches!(
                err.code.as_str(),
                "path_traversal" | "path_scope_denied" | "PATH_ESCAPE"
            ),
            "expected a path rejection code, got {}: {}",
            err.code,
            err.message
        );
    }

    #[test]
    fn test_path_scope_preflight_rejects_absolute_system_path() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "/etc/passwd", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_absolute_outside_project() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "TOP-SECRET").unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": secret.to_string_lossy(), "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_dotdot() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "../escape.txt", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret.txt");
        std::fs::write(&target, "TOP-SECRET").unwrap();
        let link = root.path().join("evil-link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "evil-link", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_rejects_missing_path_with_symlink_parent() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join("evil-dir");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "evil-dir/new.txt", "content": "x"}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_apply_patch_files_array_escape() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths(
                "apply_patch",
                &serde_json::json!({"files": [{"path": "/etc/passwd", "content": "x"}]}),
                &ctx,
            )
            .unwrap_err();
        assert_path_rejection(err);
    }

    #[test]
    fn test_path_scope_preflight_allows_project_relative() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let file = root.path().join("src").join("a.txt");
        std::fs::write(&file, "hi").unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let trusted = gateway
            .preflight_write_paths(
                "write_file",
                &serde_json::json!({"path": "src/a.txt", "content": "x"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(trusted.len(), 1);
        assert_eq!(
            trusted[0].canonical,
            file.canonicalize().unwrap(),
            "trusted path must be canonical"
        );
        assert_eq!(
            trusted[0].project_relative,
            std::path::PathBuf::from("src/a.txt"),
            "trusted path must carry the project-relative form"
        );
    }

    #[test]
    fn test_path_scope_preflight_non_write_tool_returns_empty() {
        let root = tempfile::tempdir().unwrap();
        let gateway = gateway_with_root(root.path());
        let ctx = context_for(root.path());
        let trusted = gateway
            .preflight_write_paths("grep", &serde_json::json!({"pattern": "x"}), &ctx)
            .unwrap();
        assert!(trusted.is_empty());
    }

    #[test]
    fn test_path_scope_preflight_scope_none_is_denied() {
        let root = tempfile::tempdir().unwrap();
        let mut gateway = gateway_with_root(root.path());
        gateway.register(Tool {
            name: "no_path_tool",
            description: "test",
            schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::None,
            timeout_ms: 1000,
            output_limit: 4096,
            cancellable: false,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(NoopHandler),
        }).unwrap();
        let ctx = context_for(root.path());
        let err = gateway
            .preflight_write_paths("no_path_tool", &serde_json::json!({"path": "a.txt"}), &ctx)
            .unwrap_err();
        assert_eq!(
            err.code, "path_scope_denied",
            "got {}: {}",
            err.code, err.message
        );
    }
}

#[cfg(test)]
mod progress_bounded_tests {
    //! TASK-007 (H03): live-output progress is bounded. Overflow is dropped at
    //! the producer and counted in `progress_dropped_bytes`, never buffered
    //! unboundedly, and never allowed to stall the handler.
    use super::*;
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc::error::TrySendError;

    #[tokio::test]
    async fn progress_channel_is_bounded_and_overflow_drops() {
        let capacity = 2usize;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ToolProgressChunk>(capacity);
        let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
        // Producer behavior mirrors tools/mod.rs run_terminal: try_send, and on
        // Full drop the chunk and count its bytes.
        for i in 0..100 {
            let chunk = ToolProgressChunk {
                stream: "out".into(),
                text: format!("line {i}"),
            };
            match tx.try_send(chunk) {
                Ok(()) => {}
                Err(TrySendError::Full(full)) => {
                    dropped.fetch_add(full.text.len() as u64, Ordering::Relaxed);
                }
                Err(TrySendError::Closed(_)) => break,
            }
        }
        // Slow consumer: the queue holds at most `capacity` — it never grows
        // with the producer.
        let mut drained = 0usize;
        while let Ok(chunk) = rx.try_recv() {
            let _ = chunk;
            drained += 1;
        }
        assert_eq!(drained, capacity, "queue is bounded at capacity");
        assert!(dropped.load(Ordering::Relaxed) > 0, "overflow is counted");
    }

    #[test]
    fn context_attaches_bounded_progress_and_counter() {
        let mut ctx = ToolCallContext::new(
            std::path::PathBuf::from("."),
            "r".into(),
            "c".into(),
            "t".into(),
            "full_access".into(),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel::<ToolProgressChunk>(4);
        let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
        ctx.set_progress(Some(tx), dropped.clone());
        assert!(ctx.progress.is_some());
        assert_eq!(ctx.progress_dropped_bytes.load(Ordering::Relaxed), 0);
    }
}

#[cfg(test)]
mod registry_validation_tests {
    //! TASK-012 (D01/D02): registration determinism and conflict-key exposure.
    use super::*;

    struct TestHandler;
    #[async_trait::async_trait]
    impl ToolHandler for TestHandler {
        async fn execute(
            &self,
            _input: serde_json::Value,
            _context: &ToolCallContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput {
                result: serde_json::json!({}),
                truncated: false,
                duration_ms: 0,
            })
        }
    }

    fn sample_tool(name: &'static str) -> Tool {
        Tool {
            name,
            description: "test",
            schema: serde_json::json!({ "type": "object", "properties": {}, "required": [] }),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::Any,
            timeout_ms: 1000,
            output_limit: 4096,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(TestHandler),
        }
    }

    /// D01: duplicate canonical tool names are rejected at registration.
    #[test]
    fn registry_rejects_duplicate_tool_names() {
        let mut gateway = CapabilityGateway::new();
        gateway.register(sample_tool("dup")).unwrap();
        let err = gateway.register(sample_tool("dup")).unwrap_err();
        assert!(err.contains("duplicate tool name 'dup'"), "{err}");
        // A distinct name is fine.
        gateway.register(sample_tool("other")).unwrap();
    }

    /// D02: conflict_key_for exposes the registered key so the daemon can hold
    /// a cross-run lease.
    #[test]
    fn registry_exposes_conflict_key_for() {
        let mut gateway = CapabilityGateway::new();
        let mut tool = sample_tool("exclusive");
        tool.conflict_key = Some("file:///x".into());
        gateway.register(tool).unwrap();
        assert_eq!(
            gateway.conflict_key_for("exclusive").as_deref(),
            Some("file:///x")
        );
        assert_eq!(gateway.conflict_key_for("missing"), None);
    }
}
