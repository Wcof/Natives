//! Quota 持久化操作。

use rusqlite::{params, Connection as DbConn, OptionalExtension};

use crate::ai::model::{AiResourcesSummary, QuotaSnapshot, QuotaStatus, QuotaWindow};
use crate::{Error, Result};

pub fn save_quota_snapshot(conn: &DbConn, snapshot: &QuotaSnapshot) -> Result<()> {
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;

    tx.execute(
        "INSERT OR REPLACE INTO ai_quota_snapshots
            (id, credential_id, provider_adapter, status, plan_name, error_category, error_message, fetched_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            snapshot.id,
            snapshot.credential_id,
            snapshot.provider_adapter,
            snapshot.status.as_str(),
            snapshot.plan_name,
            snapshot.error_category,
            snapshot.error_message,
            snapshot.fetched_at,
            snapshot.expires_at,
        ],
    )
    .map_err(Error::Database)?;

    tx.execute(
        "DELETE FROM ai_quota_windows WHERE snapshot_id = ?1",
        [&snapshot.id],
    )
    .map_err(Error::Database)?;

    for window in &snapshot.windows {
        tx.execute(
            "INSERT INTO ai_quota_windows
                (id, snapshot_id, label, remaining, limit_value, used, unit, reset_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                window.id,
                snapshot.id,
                window.label,
                window.remaining,
                window.limit_value,
                window.used,
                window.unit,
                window.reset_at,
            ],
        )
        .map_err(Error::Database)?;
    }

    tx.commit().map_err(Error::Database)?;
    Ok(())
}

pub fn get_quota_snapshot(conn: &DbConn, credential_id: &str) -> Result<Option<QuotaSnapshot>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, credential_id, provider_adapter, status, plan_name, error_category, error_message, fetched_at, expires_at
             FROM ai_quota_snapshots WHERE credential_id = ?1 ORDER BY fetched_at DESC LIMIT 1",
        )
        .map_err(Error::Database)?;

    let snapshot_opt = stmt
        .query_row([credential_id], |row| {
            let status_str: String = row.get(3)?;
            Ok(QuotaSnapshot {
                id: row.get(0)?,
                credential_id: row.get(1)?,
                provider_adapter: row.get(2)?,
                status: QuotaStatus::from_str(&status_str),
                plan_name: row.get(4)?,
                error_category: row.get(5)?,
                error_message: row.get(6)?,
                fetched_at: row.get(7)?,
                expires_at: row.get(8)?,
                windows: Vec::new(),
            })
        })
        .optional()
        .map_err(Error::Database)?;

    let Some(mut snapshot) = snapshot_opt else {
        return Ok(None);
    };

    let mut win_stmt = conn
        .prepare(
            "SELECT id, snapshot_id, label, remaining, limit_value, used, unit, reset_at
             FROM ai_quota_windows WHERE snapshot_id = ?1 ORDER BY id ASC",
        )
        .map_err(Error::Database)?;

    let windows = win_stmt
        .query_map([&snapshot.id], |row| {
            Ok(QuotaWindow {
                id: row.get(0)?,
                snapshot_id: row.get(1)?,
                label: row.get(2)?,
                remaining: row.get(3)?,
                limit_value: row.get(4)?,
                used: row.get(5)?,
                unit: row.get(6)?,
                reset_at: row.get(7)?,
            })
        })
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;

    snapshot.windows = windows;
    Ok(Some(snapshot))
}

pub fn get_ai_resources_summary(conn: &DbConn) -> Result<AiResourcesSummary> {
    let provider_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_providers", [], |r| r.get(0))
        .map_err(Error::Database)?;

    let connection_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_connections WHERE enabled = 1",
            [],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    let credential_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_credentials WHERE status = 'active'",
            [],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    let available_model_count: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT model_id) FROM ai_models WHERE availability = 'available'",
            [],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    Ok(AiResourcesSummary {
        provider_count: provider_count as usize,
        connection_count: connection_count as usize,
        credential_count: credential_count as usize,
        available_model_count: available_model_count as usize,
    })
}
