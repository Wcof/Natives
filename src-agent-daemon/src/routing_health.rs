use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::RouteTarget;

pub(crate) const FAILURE_THRESHOLD: u32 = 3;
pub(crate) const COOLDOWN: Duration = Duration::from_secs(60);

static CIRCUIT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static HEALTH_STATE_INITIALIZED: OnceLock<()> = OnceLock::new();

pub(crate) fn circuit_write_lock() -> &'static Mutex<()> {
    CIRCUIT_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn route_key(target: &RouteTarget) -> String {
    format!(
        "{}:{}:{}:{}",
        target.provider_id,
        target.credential_kind,
        target.credential_id.as_deref().unwrap_or(""),
        target.model_id
    )
}

pub(crate) fn route_health_connection() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(crate::natives_db_broker::default_assistant_db_path())?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS provider_route_health (route_key TEXT PRIMARY KEY, consecutive_failures INTEGER NOT NULL DEFAULT 0, open_until_ms INTEGER, half_open_in_flight INTEGER NOT NULL DEFAULT 0, in_flight INTEGER NOT NULL DEFAULT 0, last_selected_at TEXT, last_error TEXT, updated_at TEXT NOT NULL DEFAULT (datetime('now')))")?;
    let columns = conn
        .prepare("PRAGMA table_info(provider_route_health)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    for (name, sql) in [
        (
            "half_open_in_flight",
            "ALTER TABLE provider_route_health ADD COLUMN half_open_in_flight INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "in_flight",
            "ALTER TABLE provider_route_health ADD COLUMN in_flight INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "last_selected_at",
            "ALTER TABLE provider_route_health ADD COLUMN last_selected_at TEXT",
        ),
        (
            "last_error",
            "ALTER TABLE provider_route_health ADD COLUMN last_error TEXT",
        ),
    ] {
        if !columns.iter().any(|column| column == name) {
            conn.execute(sql, [])?;
        }
    }
    HEALTH_STATE_INITIALIZED.get_or_init(|| {
        let _ = conn.execute(
            "UPDATE provider_route_health SET in_flight=0, half_open_in_flight=0",
            [],
        );
    });
    Ok(conn)
}

pub(crate) fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(crate) fn circuit_open(target: &RouteTarget) -> bool {
    let Ok(conn) = route_health_connection() else {
        return false;
    };
    let open_until = conn
        .query_row(
            "SELECT open_until_ms FROM provider_route_health WHERE route_key=?1",
            [route_key(target)],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .ok()
        .flatten()
        .flatten();
    match open_until {
        Some(until) if until > epoch_ms() => true,
        Some(_) => conn
            .execute(
                "UPDATE provider_route_health SET half_open_in_flight=1, updated_at=datetime('now') WHERE route_key=?1 AND half_open_in_flight=0",
                [route_key(target)],
            )
            .map(|changed| changed == 0)
            .unwrap_or(true),
        None => false,
    }
}

pub(crate) fn record_selected(target: &RouteTarget) {
    if let Ok(conn) = route_health_connection() {
        let _ = conn.execute(
            "INSERT INTO provider_route_health(route_key, last_selected_at, updated_at) VALUES(?1,datetime('now'),datetime('now')) ON CONFLICT(route_key) DO UPDATE SET last_selected_at=datetime('now'),updated_at=datetime('now')",
            [route_key(target)],
        );
    }
}

pub(crate) fn record_inflight(key: &str, value: u32) {
    if let Ok(conn) = route_health_connection() {
        let _ = conn.execute(
            "INSERT INTO provider_route_health(route_key, in_flight, updated_at)
             VALUES(?1,?2,datetime('now'))
             ON CONFLICT(route_key) DO UPDATE SET in_flight=excluded.in_flight,updated_at=excluded.updated_at",
            params![key, value],
        );
    }
}

pub(crate) fn record_failure(target: &RouteTarget) {
    let Ok(_guard) = circuit_write_lock().lock() else {
        return;
    };
    let Ok(conn) = route_health_connection() else {
        return;
    };
    let key = route_key(target);
    let failures = conn
        .query_row(
            "SELECT consecutive_failures FROM provider_route_health WHERE route_key=?1",
            [&key],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0)
        .saturating_add(1);
    let open_until = (failures >= i64::from(FAILURE_THRESHOLD))
        .then(|| epoch_ms().saturating_add(COOLDOWN.as_millis().min(i64::MAX as u128) as i64));
    let _ = conn.execute(
        "INSERT INTO provider_route_health (route_key, consecutive_failures, open_until_ms, half_open_in_flight, last_error, updated_at) VALUES (?1,?2,?3,0,'request_failed',datetime('now')) ON CONFLICT(route_key) DO UPDATE SET consecutive_failures=excluded.consecutive_failures, open_until_ms=excluded.open_until_ms, half_open_in_flight=0, last_error='request_failed', updated_at=excluded.updated_at",
        params![key, failures, open_until],
    );
}

pub(crate) fn record_success(target: &RouteTarget) {
    if let Ok(conn) = route_health_connection() {
        let _ = conn.execute(
            "INSERT INTO provider_route_health(route_key, consecutive_failures, open_until_ms, half_open_in_flight, last_error, updated_at)
             VALUES(?1,0,NULL,0,NULL,datetime('now'))
             ON CONFLICT(route_key) DO UPDATE SET consecutive_failures=0,open_until_ms=NULL,half_open_in_flight=0,last_error=NULL,updated_at=datetime('now')",
            [route_key(target)],
        );
    }
}
