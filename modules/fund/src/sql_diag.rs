//! Test-only Fund SQL counters (NATIVES_SQL_TRACE=1). Counts only; no SQL text,
//! parameters, paths or business values are stored.
use rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

static QUERIES: AtomicU64 = AtomicU64::new(0);
static CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static SQL_NANOS: AtomicU64 = AtomicU64::new(0);
static MAX_SQL_NANOS: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    std::env::var("NATIVES_SQL_TRACE").ok().as_deref() == Some("1")
}

pub fn instrument(conn: &mut Connection) {
    if !enabled() {
        return;
    }
    CONNECTIONS.fetch_add(1, Ordering::Relaxed);
    conn.profile(Some(record_profile));
}

fn record_profile(_sql: &str, duration: std::time::Duration) {
    let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
    QUERIES.fetch_add(1, Ordering::Relaxed);
    SQL_NANOS.fetch_add(nanos, Ordering::Relaxed);
    MAX_SQL_NANOS.fetch_max(nanos, Ordering::Relaxed);
}

pub fn snapshot() -> Value {
    json!({
        "enabled": enabled(),
        "sqlQueries": QUERIES.load(Ordering::Relaxed),
        "dbConnectionsOpened": CONNECTIONS.load(Ordering::Relaxed),
        "sqlNanos": SQL_NANOS.load(Ordering::Relaxed),
        "maxSqlNanos": MAX_SQL_NANOS.load(Ordering::Relaxed),
        "timingResolution": "sqlite-milliseconds",
        "connectionCountKind": "opened-since-process-start",
    })
}

pub fn reset() {
    QUERIES.store(0, Ordering::Relaxed);
    SQL_NANOS.store(0, Ordering::Relaxed);
    MAX_SQL_NANOS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_counts_finished_statements_without_exporting_sql() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.profile(Some(super::record_profile));
        conn.query_row("SELECT 'private-value'", [], |_| Ok(()))
            .unwrap();
        let snapshot = super::snapshot();
        assert!(snapshot["sqlQueries"].as_u64().unwrap() >= 1);
        assert_eq!(snapshot["timingResolution"], "sqlite-milliseconds");
        assert!(!snapshot.to_string().contains("private-value"));
        assert!(
            snapshot["maxSqlNanos"].as_u64().unwrap() <= snapshot["sqlNanos"].as_u64().unwrap()
        );
    }
}
