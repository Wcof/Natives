//! Draft persistence and validation machinery.
//!
//! Owns `DraftPaths`, the SQLite revision store, atomic writes and the size /
//! lint gates shared by every draft tool. Path safety comes from validating
//! `draftId` against the host alphabet and generating `rev-N.html` from an
//! integer — the model never supplies a path fragment.

use crate::ToolError;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
pub(super) const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;

/// Same cap the host store enforces. Kept identical on purpose: the model is the
/// only writer in production, so if this side did not prune, the cap would never
/// actually apply to anything.
pub(super) const MAX_DRAFT_REVISIONS: i64 = 50;

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

pub(super) fn draft_io_error(message: impl Into<String>) -> ToolError {
    ToolError {
        code: "draft_io_error".into(),
        message: message.into(),
        // Disk and database contention are transient; the model may retry.
        retryable: true,
    }
}

pub(super) fn not_found(message: impl Into<String>) -> ToolError {
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
    pub(super) data_dir: PathBuf,
    pub(super) db_path: PathBuf,
}

impl DraftPaths {
    #[cfg(test)]
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

    pub(super) fn drafts_root(&self) -> PathBuf {
        self.data_dir.join("drafts")
    }

    pub(super) fn draft_dir(&self, draft_id: &str) -> Result<PathBuf, ToolError> {
        validate_draft_id(draft_id)?;
        Ok(self.drafts_root().join(draft_id))
    }

    pub(super) fn revision_path(
        &self,
        draft_id: &str,
        revision: i64,
    ) -> Result<PathBuf, ToolError> {
        if revision < 1 {
            return Err(invalid_input("revision must be >= 1"));
        }
        Ok(self
            .draft_dir(draft_id)?
            .join(format!("{REVISION_PREFIX}{revision}{REVISION_SUFFIX}")))
    }

    pub(super) fn open_db(&self) -> Result<Connection, ToolError> {
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

pub(super) fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
}

pub(super) fn default_natives_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".natives")
}

pub(super) fn content_hash(html: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(html.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The bare URL always resolves to whatever the pointer currently names, so it
/// stays valid across further revisions and rollbacks.
pub(super) fn preview_url(draft_id: &str) -> String {
    format!("/drafts/{draft_id}/")
}

pub(crate) fn str_arg<'a>(input: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid_input(format!("Missing '{key}' field")))
}

/// Row of `creative_drafts` this layer cares about.
pub(super) struct DraftRow {
    conversation_id: Option<String>,
    current_revision: i64,
}

pub(super) fn load_draft(conn: &Connection, draft_id: &str) -> Result<DraftRow, ToolError> {
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
pub(super) fn ensure_session_owns_draft(
    draft: &DraftRow,
    conversation_id: &str,
) -> Result<(), ToolError> {
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
pub(super) fn atomic_write(path: &Path, contents: &str) -> Result<(), ToolError> {
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
pub(super) fn prune_revisions(
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
pub(super) fn discard_abandoned_future(
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
pub(super) struct WriteOutcome {
    pub(super) revision: i64,
    pub(super) content_hash: String,
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
pub(super) fn append_revision(
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

pub(super) fn read_current_revision(
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
pub(super) fn rollback_revision(
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
pub(super) fn lint_failure(result: &contract_linter::LinterResult) -> ToolError {
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

pub(super) fn ensure_html_size(html: &str) -> Result<(), ToolError> {
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

pub(super) use resolve_paths;

/// Run blocking SQLite/filesystem work off the async runtime's reactor threads.
pub(super) async fn in_blocking<T, F>(work: F) -> Result<T, ToolError>
where
    F: FnOnce() -> Result<T, ToolError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| draft_io_error(format!("draft worker failed: {e}")))?
}
