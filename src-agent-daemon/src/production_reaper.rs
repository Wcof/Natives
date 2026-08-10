//! Background subagent reaper and model context-window lookup (extracted from `production.rs`, task-01 structure).
//!
//! The reaper closes idle subagent sessions; `lookup_model_context_window` resolves the routed
//! model's context window from the daemon model cache for `start_run`.

use std::time::Duration;

/// Background reaper: close idle subagent sessions.
/// Safe to call without a Tokio runtime (unit tests / sync constructors): no-ops until a
/// runtime exists; production daemon always constructs under tokio::main.
#[cfg(not(test))]
pub(crate) fn spawn_subagent_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                if let Err(e) = reaper_tick().await {
                    eprintln!("[agent-daemon] subagent reaper: {e}");
                }
            }
        });
    });
}

#[cfg(not(test))]
async fn reaper_tick() -> Result<(), String> {
    let sessions = crate::subagent_store::list_active_for_reaper()?;
    let now = chrono::Utc::now();
    for sess in sessions {
        // Never close while a live task_output still says running.
        if let Some(rec) = crate::global_run_manager()
            .runtime
            .task_output(&sess.id)
            .await
        {
            if rec.status == "running" {
                continue;
            }
        }
        if sess.status == "running" {
            // Status open/running in DB but no live task record — fall through to idle timer.
        }

        let last = chrono::DateTime::parse_from_rfc3339(&sess.last_activity_at)
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or(now);
        let idle = now.signed_duration_since(last);

        // Parent heartbeat (frontend touch) fresh within 90s → 5min idle; else 2min.
        let parent_active =
            crate::subagent_store::parent_heartbeat_recent(&sess.parent_conversation_id, 90);
        let limit_secs = if parent_active { 300i64 } else { 120i64 };
        if idle.num_seconds() >= limit_secs {
            if let Some(rec) = crate::global_run_manager()
                .runtime
                .task_output(&sess.id)
                .await
            {
                if rec.status == "running" {
                    continue;
                }
            }
            let _ = crate::subagent_store::close_subagent_session(
                &sess.id,
                "closed",
                Some("idle timeout"),
            );
        }
    }
    Ok(())
}

/// Look up model context_window from daemon model_cache (best-effort).
pub(crate) fn lookup_model_context_window(provider_id: &str, model_id: &str) -> Option<u64> {
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(crate::default_assistant_db_path);
    let art = std::env::var("NATIVES_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home)
                .join(".natives")
                .join("runtime")
        })
        .join("artifacts");
    let store = crate::storage::DataStore::new(&db_path, &art).ok()?;
    let conn = store.conn().ok()?;
    conn.query_row(
        "SELECT context_window FROM model_cache
         WHERE provider_id = ?1 AND model_id = ?2
         LIMIT 1",
        rusqlite::params![provider_id, model_id],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .filter(|w| *w > 0)
    .map(|w| w as u64)
}
