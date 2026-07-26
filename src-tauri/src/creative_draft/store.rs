//! Draft persistence: SQLite metadata + on-disk revision content.
//!
//! The split is deliberate. Revision HTML is a file so the sandbox preview can
//! serve it directly and the Agent Daemon can write it without any database
//! access (ADR-0014 section 8.1). SQLite holds only what needs to be queried or kept
//! consistent — state, revision pointer, conversation link.

use super::model::{CreativeDraft, DraftRevision, DraftState, WriteRevisionOutcome};
use super::paths;
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Keep the newest revisions plus rev-1 (ADR-0014 section 10). Linear undo only ever
/// needs adjacent revisions, and rev-1 is what "start over" falls back to.
pub const MAX_DRAFT_REVISIONS: i64 = 50;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn content_hash(html: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(html.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn row_to_draft(row: &rusqlite::Row) -> rusqlite::Result<CreativeDraft> {
    let state_raw: String = row.get("state")?;
    Ok(CreativeDraft {
        draft_id: row.get("draft_id")?,
        name: row.get("name")?,
        intent: row.get("intent")?,
        conversation_id: row.get("conversation_id")?,
        origin_module_id: row.get("origin_module_id")?,
        current_revision: row.get("current_revision")?,
        // A row whose state does not parse means the CHECK constraint was
        // bypassed; surfacing it as Drafting would hide real corruption.
        state: DraftState::parse(&state_raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn create_draft(
    conn: &Connection,
    draft_id: &str,
    name: &str,
    intent: &str,
    conversation_id: Option<&str>,
    origin_module_id: Option<&str>,
) -> Result<CreativeDraft> {
    paths::validate_draft_id(draft_id)?;
    let ts = now();
    conn.execute(
        "INSERT INTO creative_drafts
            (draft_id, name, intent, conversation_id, origin_module_id,
             current_revision, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 'drafting', ?6, ?6)",
        rusqlite::params![draft_id, name, intent, conversation_id, origin_module_id, ts],
    )?;
    get_draft(conn, draft_id)
}

pub fn get_draft(conn: &Connection, draft_id: &str) -> Result<CreativeDraft> {
    conn.query_row(
        "SELECT * FROM creative_drafts WHERE draft_id = ?1",
        [draft_id],
        row_to_draft,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("draft not found: {draft_id}")))
}

pub fn list_drafts(conn: &Connection) -> Result<Vec<CreativeDraft>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM creative_drafts
         WHERE state != 'archived'
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_draft)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Move the draft to `to`, rejecting transitions the state machine forbids.
pub fn set_state(conn: &Connection, draft_id: &str, to: DraftState) -> Result<()> {
    let draft = get_draft(conn, draft_id)?;
    draft.state.ensure_transition(to)?;
    conn.execute(
        "UPDATE creative_drafts SET state = ?2, updated_at = ?3 WHERE draft_id = ?1",
        rusqlite::params![draft_id, to.as_str(), now()],
    )?;
    Ok(())
}

/// Append a revision: write the file first, then advance the pointer.
///
/// The caller is responsible for having linted `html` — this function does not
/// re-lint, because the lint gate belongs to the tool layer where a failure can
/// be handed back to the model verbatim. What this function guarantees is that
/// a revision file and its metadata row never disagree.
pub fn append_revision(
    conn: &Connection,
    data_dir: &Path,
    draft_id: &str,
    html: &str,
) -> Result<WriteRevisionOutcome> {
    let draft = get_draft(conn, draft_id)?;
    let next = draft.current_revision + 1;
    let path = paths::revision_path(data_dir, draft_id, next)?;
    let dir = paths::draft_dir(data_dir, draft_id)?;
    std::fs::create_dir_all(&dir).map_err(Error::Io)?;

    // Writing after an undo would otherwise collide with the revision the user
    // rolled back past. The pointer only ever steps back one at a time, so
    // anything above it is unreachable forever — drop it before reusing the slot.
    discard_unreachable_revisions(conn, data_dir, draft_id, draft.current_revision)?;

    crate::module_manager::atomic_write(&path, html)?;

    let hash = content_hash(html);
    let ts = now();
    conn.execute(
        "INSERT INTO creative_draft_revisions (draft_id, revision, content_hash, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![draft_id, next, hash, ts],
    )?;
    conn.execute(
        "UPDATE creative_drafts SET current_revision = ?2, updated_at = ?3 WHERE draft_id = ?1",
        rusqlite::params![draft_id, next, ts],
    )?;

    prune_revisions(conn, data_dir, draft_id)?;

    Ok(WriteRevisionOutcome {
        revision: next,
        content_hash: hash,
    })
}

/// Read the revision the draft currently points at.
pub fn read_current(conn: &Connection, data_dir: &Path, draft_id: &str) -> Result<String> {
    let draft = get_draft(conn, draft_id)?;
    if draft.current_revision < 1 {
        return Err(Error::NotFound(format!(
            "draft {draft_id} has no revision yet"
        )));
    }
    let path = paths::revision_path(data_dir, draft_id, draft.current_revision)?;
    std::fs::read_to_string(&path).map_err(Error::Io)
}

/// Step the pointer back one revision. Files are kept, so an undo can be undone.
pub fn rollback(conn: &Connection, draft_id: &str) -> Result<i64> {
    let draft = get_draft(conn, draft_id)?;
    if draft.current_revision <= 1 {
        return Err(Error::InvalidInput(
            "already at the earliest revision".into(),
        ));
    }
    let target = draft.current_revision - 1;
    conn.execute(
        "UPDATE creative_drafts SET current_revision = ?2, updated_at = ?3 WHERE draft_id = ?1",
        rusqlite::params![draft_id, target, now()],
    )?;
    Ok(target)
}

pub fn list_revisions(conn: &Connection, draft_id: &str) -> Result<Vec<DraftRevision>> {
    let mut stmt = conn.prepare(
        "SELECT revision, content_hash, created_at
         FROM creative_draft_revisions WHERE draft_id = ?1 ORDER BY revision ASC",
    )?;
    let rows = stmt.query_map([draft_id], |row| {
        Ok(DraftRevision {
            revision: row.get(0)?,
            content_hash: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Drop revisions above `current`, which an undo has made unreachable.
///
/// Undo deliberately keeps files so it can itself be undone, but that only holds
/// until the user writes again: the new revision takes the same number, and the
/// old row would collide on the primary key.
fn discard_unreachable_revisions(
    conn: &Connection,
    data_dir: &Path,
    draft_id: &str,
    current: i64,
) -> Result<()> {
    let doomed: Vec<i64> = list_revisions(conn, draft_id)?
        .into_iter()
        .map(|r| r.revision)
        .filter(|r| *r > current)
        .collect();
    for revision in doomed {
        let path = paths::revision_path(data_dir, draft_id, revision)?;
        let _ = std::fs::remove_file(&path);
        conn.execute(
            "DELETE FROM creative_draft_revisions WHERE draft_id = ?1 AND revision = ?2",
            rusqlite::params![draft_id, revision],
        )?;
    }
    Ok(())
}

/// Drop the oldest revisions past the cap, always keeping rev-1 and the current
/// pointer. Deleting from the middle keeps "start over" and "undo" both working.
fn prune_revisions(conn: &Connection, data_dir: &Path, draft_id: &str) -> Result<()> {
    let revisions = list_revisions(conn, draft_id)?;
    if (revisions.len() as i64) <= MAX_DRAFT_REVISIONS {
        return Ok(());
    }
    let draft = get_draft(conn, draft_id)?;
    let excess = revisions.len() as i64 - MAX_DRAFT_REVISIONS;

    let doomed: Vec<i64> = revisions
        .iter()
        .map(|r| r.revision)
        .filter(|r| *r != 1 && *r != draft.current_revision)
        .take(excess as usize)
        .collect();

    for revision in doomed {
        let path = paths::revision_path(data_dir, draft_id, revision)?;
        // A missing file is not an error: the row is what we are reconciling to.
        let _ = std::fs::remove_file(&path);
        conn.execute(
            "DELETE FROM creative_draft_revisions WHERE draft_id = ?1 AND revision = ?2",
            rusqlite::params![draft_id, revision],
        )?;
    }
    Ok(())
}

/// Publish a draft's current revision as a real module.
///
/// Kept here rather than in the Tauri command so the failure path is testable:
/// everything that makes a module safe already lives in
/// `module_manager::write_generated_module` (KI-3 lint gate, kernel-computed
/// KI-1 contract_id, atomic write, previous-content snapshot, hot sync). This
/// function must never grow a second copy of those rules — it only feeds the
/// draft into that one gate and keeps the draft's own state honest.
pub fn publish(
    conn: &Connection,
    data_dir: &Path,
    modules_dir: &Path,
    draft_id: &str,
    module_id: &str,
    name: &str,
    permissions: &[String],
) -> Result<crate::module_manager::WriteModuleOutcome> {
    let html = read_current(conn, data_dir, draft_id)?;
    set_state(conn, draft_id, DraftState::Publishing)?;

    match crate::module_manager::write_generated_module(
        conn,
        modules_dir,
        module_id,
        name,
        &html,
        permissions,
    ) {
        Ok(outcome) => {
            set_state(conn, draft_id, DraftState::Published)?;
            set_state(conn, draft_id, DraftState::Archived)?;
            Ok(outcome)
        }
        Err(err) => {
            // A rejected publish must never cost the user their work: fall back
            // to Ready with the draft fully intact and let the error text tell
            // them (or the model) what to fix.
            set_state(conn, draft_id, DraftState::Ready)?;
            Err(err)
        }
    }
}

/// Remove a draft's row and its directory. Used after publish and by cleanup.
pub fn delete_draft(conn: &Connection, data_dir: &Path, draft_id: &str) -> Result<()> {
    let dir = paths::draft_dir(data_dir, draft_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(Error::Io)?;
    }
    conn.execute(
        "DELETE FROM creative_drafts WHERE draft_id = ?1",
        [draft_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let conn = Connection::open_in_memory().expect("open db");
        crate::db::create_tables(&conn).expect("base tables");
        crate::db::apply_migrations(&conn).expect("migrations");
        (conn, dir)
    }

    #[test]
    fn create_and_fetch_draft() {
        let (conn, dir) = setup();
        let d = create_draft(&conn, "draft-1", "Pomodoro", "做一个番茄钟", None, None)
            .expect("create");
        assert_eq!(d.current_revision, 0);
        assert_eq!(d.state, DraftState::Drafting);
        assert_eq!(get_draft(&conn, "draft-1").expect("fetch").name, "Pomodoro");
        drop(dir);
    }

    #[test]
    fn append_revision_writes_file_and_advances_pointer() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");

        let out = append_revision(&conn, dir.path(), "draft-1", "<html>one</html>")
            .expect("append");
        assert_eq!(out.revision, 1);

        let path = paths::revision_path(dir.path(), "draft-1", 1).expect("path");
        assert!(path.exists());
        assert_eq!(
            read_current(&conn, dir.path(), "draft-1").expect("read"),
            "<html>one</html>"
        );
    }

    #[test]
    fn rollback_moves_pointer_without_deleting_files() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html>one</html>").expect("rev1");
        append_revision(&conn, dir.path(), "draft-1", "<html>two</html>").expect("rev2");

        assert_eq!(rollback(&conn, "draft-1").expect("rollback"), 1);
        assert_eq!(
            read_current(&conn, dir.path(), "draft-1").expect("read"),
            "<html>one</html>"
        );
        // rev-2 file survives, so the undo can itself be undone.
        assert!(paths::revision_path(dir.path(), "draft-1", 2)
            .expect("path")
            .exists());
    }

    #[test]
    fn rollback_refuses_at_first_revision() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html>one</html>").expect("rev1");
        assert!(rollback(&conn, "draft-1").is_err());
    }

    #[test]
    fn illegal_state_transition_is_rejected() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        // Publishing straight from Drafting is allowed (a seeded draft is already
        // publishable), but skipping to a terminal state is not.
        assert!(set_state(&conn, "draft-1", DraftState::Published).is_err());
        assert!(set_state(&conn, "draft-1", DraftState::Archived).is_err());
        assert!(set_state(&conn, "draft-1", DraftState::Generating).is_ok());
        // And a run in flight must not be publishable.
        assert!(set_state(&conn, "draft-1", DraftState::Publishing).is_err());
        drop(dir);
    }

    #[test]
    fn read_current_errors_before_any_revision() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        assert!(read_current(&conn, dir.path(), "draft-1").is_err());
    }

    #[test]
    fn delete_removes_row_and_directory() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html>one</html>").expect("rev1");

        delete_draft(&conn, dir.path(), "draft-1").expect("delete");
        assert!(get_draft(&conn, "draft-1").is_err());
        assert!(!paths::draft_dir(dir.path(), "draft-1")
            .expect("path")
            .exists());
    }

    /// The draft must survive a rejected publish — this is the invariant that
    /// makes "just try publishing" safe for the user.
    #[test]
    fn failed_publish_keeps_draft_intact() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        // eval() is a KI-3 hard failure, so the linter rejects this at the gate.
        append_revision(
            &conn,
            dir.path(),
            "draft-1",
            r#"<html><script>eval("boom")</script></html>"#,
        )
        .expect("rev1");
        set_state(&conn, "draft-1", DraftState::Generating).expect("to generating");
        set_state(&conn, "draft-1", DraftState::Ready).expect("to ready");

        let modules = dir.path().join("modules");
        let err = publish(
            &conn,
            dir.path(),
            &modules,
            "draft-1",
            "my-app",
            "My App",
            &[],
        )
        .expect_err("linter must reject eval");
        assert!(format!("{err}").contains("eval"), "got: {err}");

        let draft = get_draft(&conn, "draft-1").expect("draft still exists");
        assert_eq!(draft.state, DraftState::Ready);
        assert_eq!(draft.current_revision, 1);
        assert!(read_current(&conn, dir.path(), "draft-1").is_ok());
        // Nothing reached the module directory.
        assert!(!modules.join("my-app").join("index.html").exists());
    }

    #[test]
    fn successful_publish_archives_draft_and_writes_module() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html><div>ok</div></html>")
            .expect("rev1");
        set_state(&conn, "draft-1", DraftState::Generating).expect("to generating");
        set_state(&conn, "draft-1", DraftState::Ready).expect("to ready");

        let modules = dir.path().join("modules");
        let outcome = publish(
            &conn,
            dir.path(),
            &modules,
            "draft-1",
            "my-app",
            "My App",
            &["db:read".to_string()],
        )
        .expect("publish");

        assert!(!outcome.contract_id.is_empty());
        assert!(modules.join("my-app").join("index.html").exists());
        assert_eq!(
            get_draft(&conn, "draft-1").expect("draft").state,
            DraftState::Archived
        );
        // Archived drafts drop out of the catalog projection.
        assert!(list_drafts(&conn).expect("list").is_empty());
    }

    /// The real flow never walks a draft through Generating/Ready — nothing in
    /// the host drives those. Publishing must work from the state a freshly
    /// created draft is actually in, which earlier tests hid by stepping the
    /// state machine by hand.
    #[test]
    fn publish_works_from_the_state_a_new_draft_is_actually_in() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html><div>ok</div></html>")
            .expect("rev1");
        assert_eq!(
            get_draft(&conn, "draft-1").expect("draft").state,
            DraftState::Drafting,
            "nothing moves a draft out of Drafting today"
        );

        let modules = dir.path().join("modules");
        publish(&conn, dir.path(), &modules, "draft-1", "my-app", "My App", &[])
            .expect("publish must work straight from Drafting");
        assert!(modules.join("my-app").join("index.html").exists());
    }

    /// Undo keeps newer files so it can be undone — until the next write reuses
    /// that revision number, which used to collide on the primary key.
    #[test]
    fn writing_after_an_undo_reuses_the_revision_slot() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html>one</html>").expect("rev1");
        append_revision(&conn, dir.path(), "draft-1", "<html>two</html>").expect("rev2");
        rollback(&conn, "draft-1").expect("undo");

        let out = append_revision(&conn, dir.path(), "draft-1", "<html>three</html>")
            .expect("writing after undo must not collide");

        assert_eq!(out.revision, 2, "the pointer, not the directory, picks the slot");
        assert_eq!(
            read_current(&conn, dir.path(), "draft-1").expect("read"),
            "<html>three</html>"
        );
        assert_eq!(list_revisions(&conn, "draft-1").expect("list").len(), 2);
    }

    #[test]
    fn publish_refuses_a_draft_with_no_revision() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        let modules = dir.path().join("modules");
        assert!(publish(&conn, dir.path(), &modules, "draft-1", "my-app", "My App", &[]).is_err());
    }

    #[test]
    fn revisions_cascade_when_draft_is_deleted() {
        let (conn, dir) = setup();
        create_draft(&conn, "draft-1", "App", "intent", None, None).expect("create");
        append_revision(&conn, dir.path(), "draft-1", "<html>one</html>").expect("rev1");
        delete_draft(&conn, dir.path(), "draft-1").expect("delete");
        assert!(list_revisions(&conn, "draft-1").expect("list").is_empty());
    }
}
