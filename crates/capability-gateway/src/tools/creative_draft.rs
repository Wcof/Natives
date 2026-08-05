//! Creative-session draft tools (ADR-0014 section 8).
//!
//! These four tools are the *only* write surface a creative session gets. They
//! exist so the model can shape a Workshop SPA without ever being handed
//! `write_file` / `apply_patch` / `run_terminal`, which is what keeps ADR-0014
//! invariant #3 ("the model cannot reach the real module directory") true by
//! construction rather than by policy.
//!
//! # Why these run without a per-call permission prompt
//!
//! Every path these tools can produce is `~/.natives/drafts/<draftId>/rev-<n>.html`.
//! `draftId` is validated against the same alphabet the host uses, and the file
//! name is generated from an integer — the model never supplies a path fragment.
//! The sandbox *is* the permission boundary, so the tools are `AlwaysAllowed`
//! rather than asking the user once per generated revision (which would make the
//! creation loop unusable). Widening `validate_draft_id` or letting a caller pass
//! a file name would silently void that argument.
//!
//! # Why the revision number comes from SQLite and never from the directory
//!
//! `rollback_draft_revision` moves the pointer back but keeps the newer files, so
//! the *highest* `rev-N.html` on disk is frequently the revision the user just
//! undid. Deriving "next revision" from a directory scan would resurrect it and
//! silently discard the rollback. `creative_drafts.current_revision` is the only
//! authority; the host's HTTP preview route resolves the same way.
//!
//! # Why the linter comes from the shared crate
//!
//! ADR-0014 invariant #2 requires drafts and publishes to be measured with one
//! ruler. `contract-linter` is that ruler, depended on by both this daemon-side
//! tool layer and the host's `write_generated_module`. Never fork the rules.

use crate::{
    PathScope, PermissionClass, SideEffect, Tool, ToolCallContext, ToolError, ToolHandler,
    ToolOutput,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Names of the creative-session surface. Kept here so the daemon's allowlist and
/// the tool definitions cannot drift apart.
pub const CREATIVE_DRAFT_TOOL_NAMES: &[&str] = &[
    "write_draft_module",
    "read_draft_module",
    "rollback_draft_revision",
    "lint_draft_module",
];

/// One generated SPA is a single HTML file; anything larger is a runaway
/// generation, not a module. Matches the ceiling in the architecture doc.
const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;

/// Same cap the host store enforces. Kept identical on purpose: the model is the
/// only writer in production, so if this side did not prune, the cap would never
/// actually apply to anything.
const MAX_DRAFT_REVISIONS: i64 = 50;

/// The host holds the same database. A writer that gives up immediately would
/// surface `SQLITE_BUSY` to the model as a spurious tool failure; waiting is
/// correct because every transaction here is a handful of small statements.
const DB_BUSY_TIMEOUT_MS: u64 = 5_000;

const REVISION_PREFIX: &str = "rev-";
const REVISION_SUFFIX: &str = ".html";

pub(crate) fn invalid_input(message: impl Into<String>) -> ToolError {
    ToolError {
        code: "invalid_input".into(),
        message: message.into(),
        retryable: false,
    }
}

fn draft_io_error(message: impl Into<String>) -> ToolError {
    ToolError {
        code: "draft_io_error".into(),
        message: message.into(),
        // Disk and database contention are transient; the model may retry.
        retryable: true,
    }
}

fn not_found(message: impl Into<String>) -> ToolError {
    ToolError {
        code: "draft_not_found".into(),
        message: message.into(),
        retryable: false,
    }
}

/// Must stay byte-for-byte equivalent to the host's `creative_draft::paths::validate_draft_id`.
///
/// This is the containment proof for the whole module: because the alphabet
/// excludes `/`, `\`, `.` and NUL, `drafts_root.join(draft_id)` cannot escape,
/// and no canonicalization pass is needed to prove it. If the host ever widens
/// its alphabet without widening this one a legitimate draft becomes unreachable;
/// if this one is widened first, the containment argument collapses.
pub fn validate_draft_id(draft_id: &str) -> Result<(), ToolError> {
    if draft_id.is_empty() || draft_id.len() > 64 {
        return Err(invalid_input("draft id must be 1-64 characters"));
    }
    if !draft_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(invalid_input(
            "draft id may only contain lowercase letters, digits and '-'",
        ));
    }
    // A leading dash reads as a CLI flag to anything that later shells out; a
    // trailing one is a typo magnet in URLs.
    if draft_id.starts_with('-') || draft_id.ends_with('-') {
        return Err(invalid_input("draft id may not start or end with '-'"));
    }
    Ok(())
}

/// Where the draft store lives for this process.
///
/// Resolved per call rather than captured at registration so a host restart that
/// relocates the data directory does not leave long-lived tools pointing at a
/// stale path. Tests inject an explicit location instead of mutating the
/// environment, which would race across parallel test threads.
#[derive(Debug, Clone)]
pub struct DraftPaths {
    data_dir: PathBuf,
    db_path: PathBuf,
}

impl DraftPaths {
    pub fn new(data_dir: impl Into<PathBuf>, db_path: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            db_path: db_path.into(),
        }
    }

    /// Production resolution. `NATIVES_DB_PATH` is set by the host before it
    /// spawns the daemon, so deriving the data directory from its parent keeps
    /// draft metadata and draft content in the same place even for relocated or
    /// portable installs.
    pub fn from_env() -> Self {
        let db_path =
            env_path("NATIVES_DB_PATH").unwrap_or_else(|| default_natives_dir().join("natives.db"));
        let data_dir = env_path("NATIVES_DATA_DIR")
            .or_else(|| db_path.parent().map(Path::to_path_buf))
            .unwrap_or_else(default_natives_dir);
        Self { data_dir, db_path }
    }

    fn drafts_root(&self) -> PathBuf {
        self.data_dir.join("drafts")
    }

    fn draft_dir(&self, draft_id: &str) -> Result<PathBuf, ToolError> {
        validate_draft_id(draft_id)?;
        Ok(self.drafts_root().join(draft_id))
    }

    fn revision_path(&self, draft_id: &str, revision: i64) -> Result<PathBuf, ToolError> {
        if revision < 1 {
            return Err(invalid_input("revision must be >= 1"));
        }
        Ok(self
            .draft_dir(draft_id)?
            .join(format!("{REVISION_PREFIX}{revision}{REVISION_SUFFIX}")))
    }

    fn open_db(&self) -> Result<Connection, ToolError> {
        if !self.db_path.exists() {
            return Err(not_found(format!(
                "natives database not found at {}",
                self.db_path.display()
            )));
        }
        let conn = Connection::open(&self.db_path)
            .map_err(|e| draft_io_error(format!("open natives.db failed: {e}")))?;
        conn.busy_timeout(std::time::Duration::from_millis(DB_BUSY_TIMEOUT_MS))
            .map_err(|e| draft_io_error(format!("set busy_timeout failed: {e}")))?;
        Ok(conn)
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
}

fn default_natives_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
}

fn content_hash(html: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(html.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The bare URL always resolves to whatever the pointer currently names, so it
/// stays valid across further revisions and rollbacks.
fn preview_url(draft_id: &str) -> String {
    format!("/drafts/{draft_id}/")
}

pub(crate) fn str_arg<'a>(input: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid_input(format!("Missing '{key}' field")))
}

/// Row of `creative_drafts` this layer cares about.
struct DraftRow {
    conversation_id: Option<String>,
    current_revision: i64,
}

fn load_draft(conn: &Connection, draft_id: &str) -> Result<DraftRow, ToolError> {
    conn.query_row(
        "SELECT conversation_id, current_revision FROM creative_drafts WHERE draft_id = ?1",
        [draft_id],
        |row| {
            Ok(DraftRow {
                conversation_id: row.get(0)?,
                current_revision: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|e| draft_io_error(format!("read draft failed: {e}")))?
    .ok_or_else(|| not_found(format!("draft not found: {draft_id}")))
}

/// A run may only touch the draft its own conversation owns.
///
/// Without this a model that learned another draft's id from context could
/// overwrite a draft the user is editing in a different session. A draft whose
/// `conversation_id` is still NULL has not been bound yet, so it stays writable.
fn ensure_session_owns_draft(draft: &DraftRow, conversation_id: &str) -> Result<(), ToolError> {
    match draft.conversation_id.as_deref() {
        Some(owner) if owner != conversation_id => Err(ToolError {
            code: "draft_not_owned".into(),
            message: "draft belongs to another conversation".into(),
            retryable: false,
        }),
        _ => Ok(()),
    }
}

/// temp file → fsync → rename, so a crash mid-write can never leave the preview
/// serving a half-generated document.
fn atomic_write(path: &Path, contents: &str) -> Result<(), ToolError> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| invalid_input("revision path has no parent"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| draft_io_error(format!("create draft dir failed: {e}")))?;
    let tmp = path.with_extension("html.tmp");
    {
        let mut file = std::fs::File::create(&tmp)
            .map_err(|e| draft_io_error(format!("create temp revision failed: {e}")))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| draft_io_error(format!("write temp revision failed: {e}")))?;
        file.sync_all()
            .map_err(|e| draft_io_error(format!("fsync temp revision failed: {e}")))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        draft_io_error(format!("publish temp revision failed: {e}"))
    })
}

/// Drop the oldest revisions past the cap, always keeping rev-1 and the current
/// pointer, so both "undo" and "start over" keep working.
fn prune_revisions(
    tx: &rusqlite::Transaction<'_>,
    paths: &DraftPaths,
    draft_id: &str,
    current_revision: i64,
) -> Result<(), ToolError> {
    let mut stmt = tx
        .prepare(
            "SELECT revision FROM creative_draft_revisions
             WHERE draft_id = ?1 ORDER BY revision ASC",
        )
        .map_err(|e| draft_io_error(format!("list revisions failed: {e}")))?;
    let revisions: Vec<i64> = stmt
        .query_map([draft_id], |row| row.get(0))
        .map_err(|e| draft_io_error(format!("list revisions failed: {e}")))?
        .collect::<rusqlite::Result<Vec<i64>>>()
        .map_err(|e| draft_io_error(format!("list revisions failed: {e}")))?;
    drop(stmt);

    if (revisions.len() as i64) <= MAX_DRAFT_REVISIONS {
        return Ok(());
    }
    let excess = revisions.len() as i64 - MAX_DRAFT_REVISIONS;
    let doomed: Vec<i64> = revisions
        .into_iter()
        .filter(|r| *r != 1 && *r != current_revision)
        .take(excess as usize)
        .collect();

    for revision in doomed {
        if let Ok(path) = paths.revision_path(draft_id, revision) {
            // A missing file is not an error: the row is what we reconcile to.
            let _ = std::fs::remove_file(&path);
        }
        tx.execute(
            "DELETE FROM creative_draft_revisions WHERE draft_id = ?1 AND revision = ?2",
            rusqlite::params![draft_id, revision],
        )
        .map_err(|e| draft_io_error(format!("prune revision failed: {e}")))?;
    }
    Ok(())
}

/// Drop revisions the pointer can no longer reach.
///
/// After a rollback the pointer sits below the highest revision. Writing again
/// means "redo differently", so everything above the new revision is abandoned:
/// the pointer only ever steps *back*, so those revisions are unreachable for
/// good. Leaving them would collide with the `(draft_id, revision)` primary key
/// on the very next write and would leave the metadata claiming revisions the
/// user can never see again.
fn discard_abandoned_future(
    tx: &rusqlite::Transaction<'_>,
    paths: &DraftPaths,
    draft_id: &str,
    from_revision: i64,
) -> Result<(), ToolError> {
    let mut stmt = tx
        .prepare(
            "SELECT revision FROM creative_draft_revisions
             WHERE draft_id = ?1 AND revision >= ?2",
        )
        .map_err(|e| draft_io_error(format!("list abandoned revisions failed: {e}")))?;
    let abandoned: Vec<i64> = stmt
        .query_map(rusqlite::params![draft_id, from_revision], |row| row.get(0))
        .map_err(|e| draft_io_error(format!("list abandoned revisions failed: {e}")))?
        .collect::<rusqlite::Result<Vec<i64>>>()
        .map_err(|e| draft_io_error(format!("list abandoned revisions failed: {e}")))?;
    drop(stmt);

    for revision in abandoned {
        // `from_revision` itself is about to be overwritten in place, so leaving
        // its file alone keeps the window where no content exists at zero.
        if revision > from_revision {
            if let Ok(path) = paths.revision_path(draft_id, revision) {
                let _ = std::fs::remove_file(&path);
            }
        }
        tx.execute(
            "DELETE FROM creative_draft_revisions WHERE draft_id = ?1 AND revision = ?2",
            rusqlite::params![draft_id, revision],
        )
        .map_err(|e| draft_io_error(format!("discard abandoned revision failed: {e}")))?;
    }
    Ok(())
}

/// Outcome of a successful revision write.
struct WriteOutcome {
    revision: i64,
    content_hash: String,
}

/// The whole revision-append, run under one `BEGIN IMMEDIATE`.
///
/// # Concurrency
///
/// The Tauri host and this daemon are separate processes on one WAL database.
/// `IMMEDIATE` takes the write lock *before* the pointer is read, so the
/// read-modify-write of `current_revision` cannot interleave with the host's own
/// writes — two processes can never allocate the same `rev-N`. The file is
/// written while that lock is held for the same reason; a deferred transaction
/// would upgrade mid-way and deadlock instead of waiting out `busy_timeout`.
fn append_revision(
    paths: &DraftPaths,
    draft_id: &str,
    conversation_id: &str,
    html: &str,
    name: Option<&str>,
) -> Result<WriteOutcome, ToolError> {
    let mut conn = paths.open_db()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| draft_io_error(format!("begin draft transaction failed: {e}")))?;

    let draft = load_draft(&tx, draft_id)?;
    ensure_session_owns_draft(&draft, conversation_id)?;

    let next = draft.current_revision + 1;
    discard_abandoned_future(&tx, paths, draft_id, next)?;
    let path = paths.revision_path(draft_id, next)?;
    atomic_write(&path, html)?;

    let hash = content_hash(html);
    let ts = now_rfc3339();
    tx.execute(
        "INSERT INTO creative_draft_revisions (draft_id, revision, content_hash, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![draft_id, next, hash, ts],
    )
    .map_err(|e| draft_io_error(format!("record revision failed: {e}")))?;

    match name {
        Some(name) => tx.execute(
            "UPDATE creative_drafts SET current_revision = ?2, name = ?3, updated_at = ?4
             WHERE draft_id = ?1",
            rusqlite::params![draft_id, next, name, ts],
        ),
        None => tx.execute(
            "UPDATE creative_drafts SET current_revision = ?2, updated_at = ?3
             WHERE draft_id = ?1",
            rusqlite::params![draft_id, next, ts],
        ),
    }
    .map_err(|e| draft_io_error(format!("advance revision pointer failed: {e}")))?;

    prune_revisions(&tx, paths, draft_id, next)?;

    tx.commit()
        .map_err(|e| draft_io_error(format!("commit draft revision failed: {e}")))?;

    Ok(WriteOutcome {
        revision: next,
        content_hash: hash,
    })
}

fn read_current_revision(
    paths: &DraftPaths,
    draft_id: &str,
    conversation_id: &str,
) -> Result<(i64, String), ToolError> {
    let conn = paths.open_db()?;
    let draft = load_draft(&conn, draft_id)?;
    ensure_session_owns_draft(&draft, conversation_id)?;
    if draft.current_revision < 1 {
        return Err(not_found(format!(
            "draft {draft_id} has no revision yet; call write_draft_module first"
        )));
    }
    let path = paths.revision_path(draft_id, draft.current_revision)?;
    let html = std::fs::read_to_string(&path)
        .map_err(|e| draft_io_error(format!("read revision failed: {e}")))?;
    Ok((draft.current_revision, html))
}

/// Step the pointer back one revision, keeping every file.
///
/// Deleting the newer file would make the undo irreversible, and would also
/// leave the "next revision" arithmetic reusing a number the metadata still
/// remembers.
fn rollback_revision(
    paths: &DraftPaths,
    draft_id: &str,
    conversation_id: &str,
) -> Result<(i64, i64), ToolError> {
    let mut conn = paths.open_db()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| draft_io_error(format!("begin draft transaction failed: {e}")))?;

    let draft = load_draft(&tx, draft_id)?;
    ensure_session_owns_draft(&draft, conversation_id)?;
    if draft.current_revision <= 1 {
        return Err(invalid_input(
            "already at the earliest revision; nothing to roll back",
        ));
    }
    let target = draft.current_revision - 1;
    tx.execute(
        "UPDATE creative_drafts SET current_revision = ?2, updated_at = ?3 WHERE draft_id = ?1",
        rusqlite::params![draft_id, target, now_rfc3339()],
    )
    .map_err(|e| draft_io_error(format!("move revision pointer failed: {e}")))?;
    tx.commit()
        .map_err(|e| draft_io_error(format!("commit rollback failed: {e}")))?;

    Ok((draft.current_revision, target))
}

/// Render lint failures as the text the model has to act on. The rules live in
/// `contract-linter`; only the presentation is here.
fn lint_failure(result: &contract_linter::LinterResult) -> ToolError {
    let mut message = String::from(
        "Contract Linter rejected this HTML, so no revision was written. Fix and retry:",
    );
    for error in &result.errors {
        message.push_str("\n- ");
        message.push_str(error);
    }
    ToolError {
        code: "lint_failed".into(),
        message,
        // The same bytes will fail again; only new content can pass.
        retryable: false,
    }
}

fn ensure_html_size(html: &str) -> Result<(), ToolError> {
    if html.len() > MAX_HTML_BYTES {
        return Err(invalid_input(format!(
            "htmlContent is {} bytes; the limit is {MAX_HTML_BYTES} bytes",
            html.len()
        )));
    }
    Ok(())
}

/// Handlers hold an optional explicit location; `None` means "resolve from the
/// environment on every call" (production). Tests pass an explicit one.
macro_rules! resolve_paths {
    ($self:expr) => {
        $self.paths.clone().unwrap_or_else(DraftPaths::from_env)
    };
}

/// Run blocking SQLite/filesystem work off the async runtime's reactor threads.
async fn in_blocking<T, F>(work: F) -> Result<T, ToolError>
where
    F: FnOnce() -> Result<T, ToolError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| draft_io_error(format!("draft worker failed: {e}")))?
}

pub struct WriteDraftModuleTool {
    paths: Option<DraftPaths>,
}

pub struct ReadDraftModuleTool {
    paths: Option<DraftPaths>,
}

pub struct RollbackDraftRevisionTool {
    paths: Option<DraftPaths>,
}

pub struct LintDraftModuleTool;

impl WriteDraftModuleTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for WriteDraftModuleTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadDraftModuleTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for ReadDraftModuleTool {
    fn default() -> Self {
        Self::new()
    }
}

impl RollbackDraftRevisionTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for RollbackDraftRevisionTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ToolHandler for WriteDraftModuleTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = str_arg(&input, "draftId")?.to_string();
        validate_draft_id(&draft_id)?;
        let html = str_arg(&input, "htmlContent")?.to_string();
        ensure_html_size(&html)?;
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Lint first, before anything is opened or created: a rejected revision
        // must leave the draft byte-identical to what the user is previewing.
        let lint = contract_linter::lint_html(&html);
        if !lint.passed {
            return Err(lint_failure(&lint));
        }

        let paths = resolve_paths!(self);
        let conversation_id = context.conversation_id.clone();
        let draft_for_worker = draft_id.clone();
        let outcome = in_blocking(move || {
            append_revision(
                &paths,
                &draft_for_worker,
                &conversation_id,
                &html,
                name.as_deref(),
            )
        })
        .await?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "revision": outcome.revision,
                "contentHash": outcome.content_hash,
                "previewUrl": preview_url(&draft_id),
                "warnings": lint.warnings,
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[async_trait::async_trait]
impl ToolHandler for ReadDraftModuleTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = str_arg(&input, "draftId")?.to_string();
        validate_draft_id(&draft_id)?;

        let paths = resolve_paths!(self);
        let conversation_id = context.conversation_id.clone();
        let draft_for_worker = draft_id.clone();
        let (revision, html) =
            in_blocking(move || read_current_revision(&paths, &draft_for_worker, &conversation_id))
                .await?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "revision": revision,
                "content": html,
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[async_trait::async_trait]
impl ToolHandler for RollbackDraftRevisionTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = str_arg(&input, "draftId")?.to_string();
        validate_draft_id(&draft_id)?;

        let paths = resolve_paths!(self);
        let conversation_id = context.conversation_id.clone();
        let draft_for_worker = draft_id.clone();
        let (previous, revision) =
            in_blocking(move || rollback_revision(&paths, &draft_for_worker, &conversation_id))
                .await?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "revision": revision,
                "rolledBackFrom": previous,
                "previewUrl": preview_url(&draft_id),
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

#[async_trait::async_trait]
impl ToolHandler for LintDraftModuleTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let html = str_arg(&input, "htmlContent")?;
        ensure_html_size(html)?;
        let lint = contract_linter::lint_html(html);
        // A failed pre-flight is a successful tool call: the model asked a
        // question and got the answer it needs to fix the content.
        Ok(ToolOutput {
            result: serde_json::json!({
                "passed": lint.passed,
                "errors": lint.errors,
                "warnings": lint.warnings,
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

pub struct CreateCreativeDraftTool {
    paths: Option<DraftPaths>,
}

impl CreateCreativeDraftTool {
    pub fn new() -> Self {
        Self { paths: None }
    }
    pub fn with_paths(paths: DraftPaths) -> Self {
        Self { paths: Some(paths) }
    }
}

impl Default for CreateCreativeDraftTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Ordinary-assistant handoff (batch 3): create a draft row the creative
/// surface can continue. The user still publishes via a Host command — the
/// assistant only ever creates a draft, never an Application.
#[async_trait::async_trait]
impl ToolHandler for CreateCreativeDraftTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let intent = str_arg(&input, "intent")?.trim().to_string();
        if intent.is_empty() || intent.chars().count() > 4_000 {
            return Err(invalid_input(
                "intent must be 1-4000 characters describing the app idea",
            ));
        }
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Untitled creation".to_string());

        let paths = self.paths.clone().unwrap_or_else(DraftPaths::from_env);
        let draft_id = format!("draft-{}", uuid::Uuid::new_v4().simple());
        validate_draft_id(&draft_id)?;

        let conn = paths.open_db()?;
        let t = now_rfc3339();
        conn.execute(
            "INSERT INTO creative_drafts
                (draft_id, name, intent, conversation_id, origin_module_id,
                 current_revision, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, NULL, 0, 'drafting', ?4, ?4)",
            rusqlite::params![draft_id, name, intent, t],
        )
        .map_err(|e| draft_io_error(format!("create draft row failed: {e}")))?;
        // Ensure the draft directory exists so creative-session tools can write
        // revisions without a race.
        std::fs::create_dir_all(paths.draft_dir(&draft_id)?)
            .map_err(|e| draft_io_error(format!("create draft dir failed: {e}")))?;

        Ok(ToolOutput {
            result: serde_json::json!({
                "draftId": draft_id,
                "name": name,
                "status": "draft_created",
                "message": "Draft created. Continue it in the Personal Creations surface; publishing is a user action, not available to the assistant.",
                "previewUrl": preview_url(&draft_id),
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

/// The ordinary-assistant handoff tool (batch 3). Distinct from the
/// creative-session surface so a general session can create a draft without
/// ever gaining the draft-writing tools.
pub fn creative_handoff_tool() -> Tool {
    Tool {
        name: "create_creative_draft",
        description: "Create a draft of a personal creation (a small web app) from a one-line idea. Returns a draftId the user can continue in the Personal Creations surface. Publishing to the app catalog is a user action, never available to the assistant.",
        schema: serde_json::json!({
            "type":"object",
            "properties":{
                "intent":{"type":"string","description":"One-line description of the app the user wants"},
                "name":{"type":"string","description":"Optional suggested app name"}
            },
            "required":["intent"]
        }),
        side_effect: SideEffect::Write,
        path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
        permission_class: PermissionClass::AlwaysAllowed,
        timeout_ms: 10_000,
        output_limit: 16_000,
        cancellable: true,
        parallel_safe: false,
        conflict_key: None,
        handler: Arc::new(CreateCreativeDraftTool::new()),
    }
}

/// The creative-session tool surface.
///
/// Deliberately *not* part of [`super::builtin_tools`]: a general session has no
/// business writing drafts, and keeping these out of the default surface means
/// they can only appear where an allowlist names them.
pub fn creative_draft_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "write_draft_module",
            description: "Write a new revision of a creative draft's HTML. Runs the Contract Linter first; on failure nothing is written and the errors are returned so you can fix them.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "draftId":{"type":"string","description":"The draft id given by the session"},
                    "htmlContent":{"type":"string","description":"Complete standalone HTML document"},
                    "name":{"type":"string","description":"Optional new draft name"}
                },
                "required":["draftId","htmlContent"]
            }),
            side_effect: SideEffect::Write,
            // Scope is the drafts root, not the project: drafts deliberately live
            // outside any workspace. Declared for audit — the gateway's path
            // policy never sees these inputs because none of them is a path key,
            // which is the point: the model supplies an id, never a path.
            path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
            // AlwaysAllowed because containment, not consent, is what makes this
            // safe (ADR-0014 section 8). Every iteration of the generate loop would
            // otherwise raise a prompt.
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 15_000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(WriteDraftModuleTool::new()),
        },
        Tool {
            name: "read_draft_module",
            description: "Read the creative draft's current revision HTML, so edits start from what the user is actually previewing.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{"draftId":{"type":"string"}},
                "required":["draftId"]
            }),
            side_effect: SideEffect::ReadOnly,
            path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 10_000,
            // One revision may be up to 5 MiB; JSON escaping needs headroom.
            output_limit: 8_388_608,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(ReadDraftModuleTool::new()),
        },
        Tool {
            name: "rollback_draft_revision",
            description: "Move the draft back to its previous revision. Revision files are kept, so a rollback can itself be rolled forward by writing again.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{"draftId":{"type":"string"}},
                "required":["draftId"]
            }),
            side_effect: SideEffect::Write,
            path_scope: PathScope::Glob("**/.natives/drafts/**".into()),
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 10_000,
            output_limit: 16_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(RollbackDraftRevisionTool::new()),
        },
        Tool {
            name: "lint_draft_module",
            description: "Check HTML against the Contract Linter without writing anything. Same rules that gate publishing.",
            schema: serde_json::json!({
                "type":"object",
                "properties":{"htmlContent":{"type":"string"}},
                "required":["htmlContent"]
            }),
            side_effect: SideEffect::ReadOnly,
            // Touches no path at all.
            path_scope: PathScope::None,
            permission_class: PermissionClass::AlwaysAllowed,
            timeout_ms: 5_000,
            output_limit: 64_000,
            cancellable: true,
            parallel_safe: false,
            conflict_key: None,
            handler: Arc::new(LintDraftModuleTool),
        },
    ]
}

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
        let builtins: Vec<&str> = super::super::builtin_tools()
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
        let builtins: Vec<&str> = super::super::builtin_tools()
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
