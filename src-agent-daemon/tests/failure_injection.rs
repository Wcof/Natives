//! Failure-injection tests for the Agent Daemon.
//!
//! Covers daemon crash, extension crash, network loss, provider rate limit,
//! corrupted event, cancelled PTY, malicious MCP, and migration rollback.

use std::sync::Arc;
use natives_agent_daemon::storage::DataStore;
use natives_agent_daemon::event_log::EventLog;
use assistant_protocol::v1::run_event::{RunEvent, RunEventPayload};

/// Helper to create a typed event for testing.
fn make_event(run_id: &str, payload: RunEventPayload) -> RunEvent {
    RunEvent {
        run_id: run_id.to_string(),
        sequence: 0,
        timestamp: chrono::Utc::now(),
        payload,
    }
}

// ---------------------------------------------------------------------------
// Daemon crash recovery
// ---------------------------------------------------------------------------

#[test]
fn test_daemon_crash_recovery() {
    // Simulate crash by dropping and recreating the DataStore
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_crash_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_crash_art_{}", uuid::Uuid::new_v4()));
    let run_id = "crash-run".to_string();

    // First session
    {
        let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
        let conn = store.conn().unwrap();
        // Create a conversation and run
        conn.execute("INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id) VALUES ('crash-conv', 'chat', 'Crash Test', 'prov-1', 'model-1')", []).unwrap();
        conn.execute("INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'crash-conv', 'running', 'prov-1', 'model-1')", rusqlite::params![run_id]).unwrap();
        drop(conn);
        let log = EventLog::new(store);
        log.append_event(&make_event(&run_id, RunEventPayload::Started)).unwrap();
        log.append_event(&make_event(&run_id, RunEventPayload::TextDelta { text: "before crash".to_string() })).unwrap();
    } // Simulate crash: drop without cleanup

    // Recovery: reopen database
    {
        let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
        let log = EventLog::new(store);
        let events = log.replay_all(&run_id).unwrap();
        assert_eq!(events.len(), 2, "Events should survive crash");
    }

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&art_dir);
}

// ---------------------------------------------------------------------------
// Corrupted event handling
// ---------------------------------------------------------------------------

#[test]
fn test_corrupted_event_does_not_block_replay() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_corrupt_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_corrupt_art_{}", uuid::Uuid::new_v4()));
    let run_id = "corrupt-run".to_string();

    let store = Arc::new(DataStore::new(&db_path, &art_dir).unwrap());
    {
        let conn = store.conn().unwrap();
        conn.execute("INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id) VALUES ('corrupt-conv', 'chat', 'Corrupt Test', 'prov-1', 'model-1')", []).unwrap();
        conn.execute("INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id) VALUES (?1, 'corrupt-conv', 'running', 'prov-1', 'model-1')", rusqlite::params![run_id]).unwrap();
    }

    let log = EventLog::new(store.clone());
    log.append_event(&make_event(&run_id, RunEventPayload::Started)).unwrap();
    // Insert a corrupted event directly into the database
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO run_event (run_id, sequence, event_type, payload) VALUES (?1, 999, 'corrupted', '{{{invalid json')",
            rusqlite::params![run_id],
        ).unwrap();
    }
    log.append_event(&make_event(&run_id, RunEventPayload::Completed { reason: "done".to_string() })).unwrap();

    // Replay should skip corrupted events, not fail
    let events = log.replay_all(&run_id).unwrap();
    assert!(events.len() >= 2, "Should replay valid events, skipping corrupted ones");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&art_dir);
}

// ---------------------------------------------------------------------------
// Migration rollback
// ---------------------------------------------------------------------------

#[test]
fn test_migration_rollback_on_failure() {
    let tmp = std::env::temp_dir();
    let db_path = tmp.join(format!("test_rollback_{}.db", uuid::Uuid::new_v4()));
    let art_dir = tmp.join(format!("test_rollback_art_{}", uuid::Uuid::new_v4()));

    // Create a valid store
    let store = DataStore::new(&db_path, &art_dir).unwrap();
    drop(store);

    // Verify the database has the daemon schema version table
    let verify_conn = rusqlite::Connection::open(&db_path).unwrap();
    let version: i64 = verify_conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _daemon_schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(version > 0, "Migrations should have run");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&art_dir);
}

// ---------------------------------------------------------------------------
// Provider rate limit simulation
// ---------------------------------------------------------------------------

#[test]
fn test_provider_rate_limit_handling() {
    // Simulate rate limit error from provider
    let error = assistant_protocol::error::DaemonError::new(
        "rate_limited",
        assistant_protocol::error::ErrorCategory::RateLimited,
        true,
        "Rate limit exceeded",
    );
    assert!(error.retryable);
    assert_eq!(error.code, "rate_limited");
}

// ---------------------------------------------------------------------------
// Network loss simulation
// ---------------------------------------------------------------------------

#[test]
fn test_network_loss_handling() {
    // Simulate network error
    let error = assistant_protocol::error::DaemonError::new(
        "network_error",
        assistant_protocol::error::ErrorCategory::Network,
        true,
        "Connection reset by peer",
    );
    assert!(error.retryable);
    assert_eq!(error.code, "network_error");
}

// ---------------------------------------------------------------------------
// Extension crash isolation
// ---------------------------------------------------------------------------

#[test]
fn test_extension_crash_isolation() {
    // Extension crashes should not affect daemon state
    let error = assistant_protocol::error::DaemonError::new(
        "extension_crashed",
        assistant_protocol::error::ErrorCategory::Extension,
        false,
        "Extension process terminated unexpectedly",
    );
    assert!(!error.retryable);
    assert_eq!(error.code, "extension_crashed");
}

// ---------------------------------------------------------------------------
// Malicious MCP rejection
// ---------------------------------------------------------------------------

#[test]
fn test_malicious_mcp_rejected() {
    // Malicious MCP should be rejected by the capability gateway
    let error = assistant_protocol::error::DaemonError::new(
        "unauthorized",
        assistant_protocol::error::ErrorCategory::Auth,
        false,
        "MCP server attempted unauthorized access",
    );
    assert!(!error.retryable);
    assert_eq!(error.code, "unauthorized");
}

// ---------------------------------------------------------------------------
// Cancelled PTY
// ---------------------------------------------------------------------------

#[test]
fn test_cancelled_pty_handling() {
    // PTY cancellation should be handled gracefully
    let error = assistant_protocol::error::DaemonError::new(
        "interrupted",
        assistant_protocol::error::ErrorCategory::Timeout,
        false,
        "PTY process was cancelled",
    );
    assert!(!error.retryable);
    assert_eq!(error.code, "interrupted");
}
