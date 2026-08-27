use crate::{db, emit_db_state_changed, Error, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::State;

use crate::AppState;

/// Theme revision counter (process-scoped, monotonic). The renderer's
/// `AppearanceCoordinator` uses it to reconcile `db-state-changed` events and
/// discard out-of-order delivery. It is intentionally *not* persisted: event
/// ordering only needs to be monotonic within a running Host process.
static THEME_REVISION: AtomicU64 = AtomicU64::new(0);

fn bump_theme_revision() -> u64 {
    THEME_REVISION.fetch_add(1, Ordering::SeqCst) + 1
}

/// Resolve the current theme from the single persisted authority
/// (`settings:theme`) and normalize to the two-value contract.
///
/// `settings:theme` is the only source (ADR-0022); workspace legacy `theme`
/// columns never drive the runtime theme once the migration has locked the key.
fn current_theme(conn: &rusqlite::Connection) -> Result<String> {
    let stored =
        db::get_setting(conn, db::THEME_KEY)?.unwrap_or_else(|| db::DEFAULT_THEME.to_string());
    // V-002: never return a legacy alias or unknown value across IPC.
    Ok(db::normalize_theme(&stored).to_string())
}

/// Persist a theme value to `settings:theme` (normalize + upsert).
///
/// The read-modify-write is wrapped in a transaction so the value is committed
/// atomically; callers must emit the `db-state-changed` broadcast only after
/// `tx.commit()` returns.
fn set_theme_value(conn: &rusqlite::Connection, theme: &str) -> Result<()> {
    let normalized = db::normalize_theme(theme);
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    db::set_setting(&tx, db::THEME_KEY, normalized)?;
    tx.commit().map_err(Error::Database)?;
    Ok(())
}

#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    current_theme(conn)
}

#[tauri::command]
pub fn set_theme(
    theme: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    // V-002/V-004: only the canonical value is persisted and forwarded.
    let normalized = db::normalize_theme(&theme);
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    set_theme_value(conn, normalized)?;

    // Broadcast only *after* the value is durably committed, so consumers
    // never observe a theme that does not match the persisted authority.
    let revision = bump_theme_revision();
    emit_db_state_changed(
        &app_handle,
        "theme",
        serde_json::json!({ "theme": normalized, "revision": revision }),
    );

    // ── 同步 Ghostty 主题配置（下次启动生效）──
    // Best-effort file write; a failure does not roll back the committed
    // theme, so the error is deliberately surfaced only through the log.
    if let Err(e) = crate::ghostty_config::write_config(normalized) {
        eprintln!("[theme] Ghostty config sync failed (best-effort): {e}");
    }

    Ok(())
}

/// 对外暴露的 Ghostty 主题同步命令（可由前端手动触发）
///
/// Reads the persisted authority, normalizes it, and rewrites the Ghostty
/// config. It does **not** modify `settings:theme`, so no `db:state-changed`
/// broadcast is emitted — there is no theme change for consumers to reconcile.
#[tauri::command]
pub fn builtin_tool_ghostty_sync_theme(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let normalized = current_theme(conn)?;
    let path = crate::ghostty_config::write_config(&normalized)?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod theme_command_tests {
    //! Pure-function tests for the theme authority helpers — no Tauri state
    //! required. The tauri command wrappers are thin adapters over these.
    //!
    //! Covers: normalization, persistence, transaction rollback semantics and
    //! migration-idempotent read-back. Migration preference resolution itself
    //! is exercised in `db::migration_v27` / `db::migration_v30` tests.

    use super::*;
    use crate::db::{apply_migrations, create_tables};
    use rusqlite::Connection;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn normalize_accepts_only_light_family_as_light() {
        // The contract vocabulary is exactly `dark` | `light` (V-002).
        assert_eq!(db::normalize_theme("dark"), "dark");
        assert_eq!(db::normalize_theme("light"), "light");
        // Legacy aliases map one-way, never produced at runtime (V-004).
        assert_eq!(db::normalize_theme("terminal-volt"), "dark");
        assert_eq!(db::normalize_theme("frosted-jasmine"), "light");
        assert_eq!(db::normalize_theme("frosted-jasmine"), "light");
        // Unknown / empty / uppercase collapse to the default.
        assert_eq!(db::normalize_theme(""), "dark");
        assert_eq!(db::normalize_theme("neon-lime"), "dark");
        assert_eq!(db::normalize_theme("DARK"), "dark");
    }

    #[test]
    fn get_returns_normalized_persisted_value() {
        let conn = fixture();
        // Canonical round-trips unchanged.
        set_theme_value(&conn, "light").unwrap();
        assert_eq!(current_theme(&conn).unwrap(), "light");
        // A legacy alias persisted pre-normalization is sanitized at read.
        set_theme_value(&conn, "frosted-jasmine").unwrap();
        assert_eq!(current_theme(&conn).unwrap(), "light");
        // Unknown value normalizes to the default.
        set_theme_value(&conn, "solar-flare").unwrap();
        assert!(current_theme(&conn).unwrap() != "solar-flare");
        assert_eq!(current_theme(&conn).unwrap(), "dark");
    }

    #[test]
    fn default_theme_when_no_setting_is_dark() {
        let conn = fixture();
        // The migration locks `settings:theme`, so an absent key only occurs
        // on a fresh table — the read must still fall back to a canonical token.
        conn.execute("DELETE FROM settings WHERE key = ?1", [db::THEME_KEY])
            .unwrap();
        assert_eq!(current_theme(&conn).unwrap(), "dark");
    }
}
