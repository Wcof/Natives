//! Creative-session draft tool tests.

use super::store::*;
use super::tools::*;
use super::*;
use crate::{SideEffect, ToolCallContext, ToolError, ToolHandler, ToolOutput};
use rusqlite::Connection;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    const CONVERSATION: &str = "conv-1";

    /// Mirrors the host's v10 migration. Duplicated only in test setup so the
    /// production path keeps a single owner of the schema.
    const SCHEMA: &str = "
        CREATE TABLE creative_drafts (
            draft_id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            intent TEXT NOT NULL,
            conversation_id TEXT,
            origin_module_id TEXT,
            current_revision INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL DEFAULT 'drafting'
                CHECK(state IN ('drafting','generating','ready','publishing','published','archived')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE creative_draft_revisions (
            draft_id TEXT NOT NULL REFERENCES creative_drafts(draft_id) ON DELETE CASCADE,
            revision INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (draft_id, revision)
        );
    ";

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: DraftPaths,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let db_path = dir.path().join("natives.db");
            let conn = Connection::open(&db_path).expect("open db");
            conn.execute_batch(SCHEMA).expect("schema");
            let paths = DraftPaths::new(dir.path(), &db_path);
            Self { _dir: dir, paths }
        }

        fn create_draft(&self, draft_id: &str, conversation_id: Option<&str>) {
            let conn = Connection::open(&self.paths.db_path).expect("open db");
            conn.execute(
                "INSERT INTO creative_drafts
                    (draft_id, name, intent, conversation_id, origin_module_id,
                     current_revision, state, created_at, updated_at)
                 VALUES (?1, 'App', 'intent', ?2, NULL, 0, 'drafting', ?3, ?3)",
                rusqlite::params![draft_id, conversation_id, now_rfc3339()],
            )
            .expect("insert draft");
        }

        fn current_revision(&self, draft_id: &str) -> i64 {
            let conn = Connection::open(&self.paths.db_path).expect("open db");
            conn.query_row(
                "SELECT current_revision FROM creative_drafts WHERE draft_id = ?1",
                [draft_id],
                |row| row.get(0),
            )
            .expect("read pointer")
        }

        fn revision_rows(&self, draft_id: &str) -> i64 {
            let conn = Connection::open(&self.paths.db_path).expect("open db");
            conn.query_row(
                "SELECT COUNT(*) FROM creative_draft_revisions WHERE draft_id = ?1",
                [draft_id],
                |row| row.get(0),
            )
            .expect("count revisions")
        }
    }

    fn context() -> ToolCallContext {
        ToolCallContext::with_cancel(
            PathBuf::from("/tmp"),
            "run-1".into(),
            CONVERSATION.into(),
            "call-1".into(),
            "ask".into(),
            CancellationToken::new(),
        )
    }

    async fn write(fx: &Fixture, draft_id: &str, html: &str) -> Result<ToolOutput, ToolError> {
        WriteDraftModuleTool::with_paths(fx.paths.clone())
            .execute(
                serde_json::json!({"draftId": draft_id, "htmlContent": html}),
                &context(),
            )
            .await
    }

    async fn read(fx: &Fixture, draft_id: &str) -> Result<ToolOutput, ToolError> {
        ReadDraftModuleTool::with_paths(fx.paths.clone())
            .execute(serde_json::json!({"draftId": draft_id}), &context())
            .await
    }

    async fn rollback(fx: &Fixture, draft_id: &str) -> Result<ToolOutput, ToolError> {
        RollbackDraftRevisionTool::with_paths(fx.paths.clone())
            .execute(serde_json::json!({"draftId": draft_id}), &context())
            .await
    }

    #[test]
    fn draft_id_rules_match_the_host() {
        for bad in [
            "..",
            "../etc",
            "a/b",
            "a\\b",
            "a\0b",
            "A",
            "under_score",
            "",
            "-lead",
            "trail-",
        ] {
            assert!(
                validate_draft_id(bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
        assert!(validate_draft_id(&"a".repeat(65)).is_err());
        assert!(validate_draft_id(&"a".repeat(64)).is_ok());
        assert!(validate_draft_id("draft-01h9zk").is_ok());
    }

    #[tokio::test]
    async fn traversal_shaped_draft_id_is_rejected_before_any_io() {
        let fx = Fixture::new();
        for bad in ["../../etc/passwd", "a/b", "..", "A-B"] {
            let err = write(&fx, bad, "<html><div>ok</div></html>")
                .await
                .expect_err("traversal id must be rejected");
            assert_eq!(err.code, "invalid_input", "id {bad:?}");
        }
        // Nothing was created outside the draft root — in fact nothing at all.
        assert!(!fx.paths.drafts_root().exists());
    }

    #[tokio::test]
    async fn write_appends_revision_and_advances_pointer() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));

        let out = write(&fx, "draft-1", "<html><div>one</div></html>")
            .await
            .expect("write");
        assert_eq!(out.result["revision"], 1);
        assert_eq!(out.result["previewUrl"], "/drafts/draft-1/");
        assert_eq!(fx.current_revision("draft-1"), 1);
        assert!(fx.paths.revision_path("draft-1", 1).expect("path").exists());

        let out = write(&fx, "draft-1", "<html><div>two</div></html>")
            .await
            .expect("write 2");
        assert_eq!(out.result["revision"], 2);
        assert_eq!(fx.current_revision("draft-1"), 2);
    }

    /// The invariant that makes the generate loop safe: a rejected revision costs
    /// the user nothing.
    #[tokio::test]
    async fn lint_failure_writes_nothing_and_leaves_the_pointer_alone() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        write(&fx, "draft-1", "<html><div>good</div></html>")
            .await
            .expect("baseline");

        let err = write(
            &fx,
            "draft-1",
            r#"<html><script>eval("boom")</script></html>"#,
        )
        .await
        .expect_err("linter must reject eval");
        assert_eq!(err.code, "lint_failed");
        assert!(err.message.contains("eval"), "got: {}", err.message);

        assert_eq!(fx.current_revision("draft-1"), 1);
        assert_eq!(fx.revision_rows("draft-1"), 1);
        assert!(!fx.paths.revision_path("draft-1", 2).expect("path").exists());
        assert_eq!(
            read(&fx, "draft-1").await.expect("read").result["content"],
            "<html><div>good</div></html>"
        );
    }

    #[tokio::test]
    async fn remote_script_is_rejected_by_the_same_ruler_as_publishing() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        let err = write(
            &fx,
            "draft-1",
            r#"<html><script src="https://cdn.example.com/x.js"></script></html>"#,
        )
        .await
        .expect_err("remote script must be rejected");
        assert_eq!(err.code, "lint_failed");
        assert_eq!(fx.current_revision("draft-1"), 0);
    }

    #[tokio::test]
    async fn read_errors_when_the_draft_has_no_revision() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        let err = read(&fx, "draft-1").await.expect_err("no revision yet");
        assert_eq!(err.code, "draft_not_found");
        assert!(err.message.contains("no revision"), "got: {}", err.message);
    }

    #[tokio::test]
    async fn read_errors_when_the_draft_does_not_exist() {
        let fx = Fixture::new();
        let err = read(&fx, "draft-missing").await.expect_err("unknown draft");
        assert_eq!(err.code, "draft_not_found");
    }

    #[tokio::test]
    async fn rollback_moves_the_pointer_and_keeps_the_newer_file() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        write(&fx, "draft-1", "<html><div>one</div></html>")
            .await
            .expect("rev1");
        write(&fx, "draft-1", "<html><div>two</div></html>")
            .await
            .expect("rev2");

        let out = rollback(&fx, "draft-1").await.expect("rollback");
        assert_eq!(out.result["revision"], 1);
        assert_eq!(out.result["rolledBackFrom"], 2);
        assert_eq!(
            read(&fx, "draft-1").await.expect("read").result["content"],
            "<html><div>one</div></html>"
        );
        // Undo must be undoable, so rev-2 stays on disk and in the metadata.
        assert!(fx.paths.revision_path("draft-1", 2).expect("path").exists());
        assert_eq!(fx.revision_rows("draft-1"), 2);
    }

    #[tokio::test]
    async fn rollback_refuses_at_the_first_revision() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        write(&fx, "draft-1", "<html><div>one</div></html>")
            .await
            .expect("rev1");

        let err = rollback(&fx, "draft-1")
            .await
            .expect_err("rev-1 is the floor");
        assert_eq!(err.code, "invalid_input");
        assert_eq!(fx.current_revision("draft-1"), 1);
    }

    #[tokio::test]
    async fn rollback_refuses_when_no_revision_exists() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        assert!(rollback(&fx, "draft-1").await.is_err());
    }

    /// A rolled-back pointer means the highest file on disk is *not* the current
    /// revision. Deriving the next number from the directory would resurrect the
    /// undone revision; deriving it from the pointer overwrites it, which is what
    /// "redo differently" has to mean.
    #[tokio::test]
    async fn next_revision_follows_the_pointer_not_the_directory() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        write(&fx, "draft-1", "<html><div>one</div></html>")
            .await
            .expect("rev1");
        write(&fx, "draft-1", "<html><div>two</div></html>")
            .await
            .expect("rev2");
        write(&fx, "draft-1", "<html><div>three</div></html>")
            .await
            .expect("rev3");
        rollback(&fx, "draft-1").await.expect("rollback to 2");
        rollback(&fx, "draft-1").await.expect("rollback to 1");

        // rev-2.html and rev-3.html still exist on disk, yet the pointer says 1.
        let out = write(&fx, "draft-1", "<html><div>redone</div></html>")
            .await
            .expect("rev2 again");
        assert_eq!(out.result["revision"], 2);
        assert_eq!(
            read(&fx, "draft-1").await.expect("read").result["content"],
            "<html><div>redone</div></html>"
        );
        // The abandoned branch is gone rather than lingering as unreachable rows.
        assert_eq!(fx.revision_rows("draft-1"), 2);
        assert!(!fx.paths.revision_path("draft-1", 3).expect("path").exists());
    }

    #[tokio::test]
    async fn another_conversations_draft_is_not_writable() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some("someone-else"));

        let err = write(&fx, "draft-1", "<html><div>ok</div></html>")
            .await
            .expect_err("cross-session write must be denied");
        assert_eq!(err.code, "draft_not_owned");
        assert_eq!(fx.current_revision("draft-1"), 0);

        assert_eq!(
            read(&fx, "draft-1")
                .await
                .expect_err("cross-session read denied")
                .code,
            "draft_not_owned"
        );
    }

    /// A draft created before the assistant session exists has no owner yet.
    #[tokio::test]
    async fn unbound_draft_is_writable() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", None);
        assert!(write(&fx, "draft-1", "<html><div>ok</div></html>")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn oversized_html_is_rejected_before_linting() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        let huge = "a".repeat(MAX_HTML_BYTES + 1);
        let err = write(&fx, "draft-1", &huge).await.expect_err("size cap");
        assert_eq!(err.code, "invalid_input");
        assert_eq!(fx.current_revision("draft-1"), 0);
    }

    #[tokio::test]
    async fn write_updates_the_name_when_given() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        WriteDraftModuleTool::with_paths(fx.paths.clone())
            .execute(
                serde_json::json!({
                    "draftId": "draft-1",
                    "htmlContent": "<html><div>ok</div></html>",
                    "name": "Pomodoro"
                }),
                &context(),
            )
            .await
            .expect("write");

        let conn = Connection::open(&fx.paths.db_path).expect("open db");
        let name: String = conn
            .query_row(
                "SELECT name FROM creative_drafts WHERE draft_id = ?1",
                ["draft-1"],
                |row| row.get(0),
            )
            .expect("name");
        assert_eq!(name, "Pomodoro");
    }

    #[tokio::test]
    async fn lint_tool_reports_failures_without_touching_disk() {
        let fx = Fixture::new();
        let out = LintDraftModuleTool
            .execute(
                serde_json::json!({"htmlContent": r#"<html><script>eval("x")</script></html>"#}),
                &context(),
            )
            .await
            .expect("lint is never an error");
        assert_eq!(out.result["passed"], false);
        assert!(!out.result["errors"].as_array().expect("errors").is_empty());
        assert!(!fx.paths.drafts_root().exists());
    }

    #[tokio::test]
    async fn lint_tool_passes_clean_html_and_surfaces_warnings() {
        let out = LintDraftModuleTool
            .execute(
                serde_json::json!({"htmlContent": r#"<html><button onclick="go()">x</button></html>"#}),
                &context(),
            )
            .await
            .expect("lint");
        assert_eq!(out.result["passed"], true);
        assert!(!out.result["warnings"]
            .as_array()
            .expect("warnings")
            .is_empty());
    }

    #[tokio::test]
    async fn missing_arguments_are_rejected() {
        let fx = Fixture::new();
        assert_eq!(
            WriteDraftModuleTool::with_paths(fx.paths.clone())
                .execute(serde_json::json!({"draftId": "draft-1"}), &context())
                .await
                .expect_err("missing html")
                .code,
            "invalid_input"
        );
        assert_eq!(
            LintDraftModuleTool
                .execute(serde_json::json!({}), &context())
                .await
                .expect_err("missing html")
                .code,
            "invalid_input"
        );
    }

    #[test]
    fn surface_names_match_the_registered_tools() {
        let registered: Vec<&str> = creative_draft_tools().iter().map(|t| t.name).collect();
        assert_eq!(registered, CREATIVE_DRAFT_TOOL_NAMES);
    }

    /// The creative surface must not smuggle in a general write tool.
    #[test]
    fn surface_contains_no_general_purpose_write_tools() {
        for tool in creative_draft_tools() {
            assert!(
                !matches!(
                    tool.name,
                    "write_file" | "edit_file" | "apply_patch" | "run_terminal"
                ),
                "unexpected tool on the creative surface: {}",
                tool.name
            );
            assert!(
                !matches!(tool.side_effect, SideEffect::Process | SideEffect::Network),
                "{} must not run processes or reach the network",
                tool.name
            );
        }
    }

    #[test]
    fn draft_tools_are_absent_from_the_default_builtin_surface() {
        let builtins: Vec<&str> = crate::tools::builtin_tools()
            .iter()
            .map(|t| t.name)
            .collect();
        for name in CREATIVE_DRAFT_TOOL_NAMES {
            assert!(
                !builtins.contains(name),
                "{name} must be opt-in via allowlist only"
            );
        }
    }

    /// Batch 3: the ordinary assistant surface admits the draft *handoff* tool
    /// (create only) while the draft-writing tools stay allowlist-only.
    #[test]
    fn ordinary_surface_admits_create_draft_handoff_only() {
        let builtins: Vec<&str> = crate::tools::builtin_tools()
            .iter()
            .map(|t| t.name)
            .collect();
        assert!(
            builtins.contains(&"create_creative_draft"),
            "ordinary assistants must be able to create a draft"
        );
        for name in CREATIVE_DRAFT_TOOL_NAMES {
            assert!(
                !builtins.contains(name),
                "{name} must never reach the ordinary surface"
            );
        }
    }

    #[tokio::test]
    async fn create_creative_draft_creates_row_and_dir() {
        let fx = Fixture::new();
        let out = CreateCreativeDraftTool::with_paths(fx.paths.clone())
            .execute(
                serde_json::json!({"intent": "A pomodoro timer app", "name": "Pomo"}),
                &context(),
            )
            .await
            .expect("create succeeds");
        let draft_id = out
            .result
            .get("draftId")
            .and_then(|v| v.as_str())
            .expect("draftId")
            .to_string();
        let conn = Connection::open(&fx.paths.db_path).expect("open db");
        let (name, intent, state): (String, String, String) = conn
            .query_row(
                "SELECT name, intent, state FROM creative_drafts WHERE draft_id = ?1",
                [&draft_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("draft row");
        assert_eq!(name, "Pomo");
        assert_eq!(intent, "A pomodoro timer app");
        assert_eq!(state, "drafting");
        assert_eq!(
            out.result.get("status").and_then(|v| v.as_str()),
            Some("draft_created")
        );
        assert!(
            fx.paths.draft_dir(&draft_id).unwrap().is_dir(),
            "draft dir must exist so the creative session can write revisions"
        );
    }

    #[tokio::test]
    async fn create_creative_draft_rejects_empty_intent() {
        let fx = Fixture::new();
        let err = CreateCreativeDraftTool::with_paths(fx.paths.clone())
            .execute(serde_json::json!({"intent": "   "}), &context())
            .await
            .expect_err("empty intent must fail closed");
        assert_eq!(err.code, "invalid_input");
    }

    #[tokio::test]
    async fn prune_keeps_rev_one_and_the_current_pointer() {
        let fx = Fixture::new();
        fx.create_draft("draft-1", Some(CONVERSATION));
        for i in 0..(MAX_DRAFT_REVISIONS + 3) {
            write(&fx, "draft-1", &format!("<html><div>{i}</div></html>"))
                .await
                .expect("write");
        }
        let current = fx.current_revision("draft-1");
        assert_eq!(current, MAX_DRAFT_REVISIONS + 3);
        assert_eq!(fx.revision_rows("draft-1"), MAX_DRAFT_REVISIONS);
        assert!(fx.paths.revision_path("draft-1", 1).expect("path").exists());
        assert!(fx
            .paths
            .revision_path("draft-1", current)
            .expect("path")
            .exists());
    }
}
