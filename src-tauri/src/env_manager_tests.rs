#![cfg(test)]

use crate::db;
use crate::env_manager::{
    create_profile, delete_profile, get_variables, init_env_encryption_key, inject_env,
    list_profiles, reset_env_key_cache_for_tests, set_default_profile, set_variable,
};
use crate::Error;
use lazy_static::lazy_static;
use rusqlite::Connection;
use std::sync::Mutex;

const ENCRYPTION_KEY_SETTING: &str = "env_encryption_key";

lazy_static! {
    static ref TEST_MUTEX: Mutex<()> = Mutex::new(());
}

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS env_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            is_default INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS env_variables (
            profile_id INTEGER NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_encrypted TEXT NOT NULL,
            UNIQUE(profile_id, key)
        );",
    )
    .unwrap();
    conn
}

#[test]
fn test_init_env_encryption_key_stores_key_in_sqlite() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_env_key_cache_for_tests();
    let conn = setup_test_db();
    let key = init_env_encryption_key(&conn).unwrap();
    assert_eq!(hex::decode(&key).unwrap().len(), 32);
    let stored = db::get_setting(&conn, ENCRYPTION_KEY_SETTING)
        .unwrap()
        .unwrap();
    assert_eq!(stored, key);
}

#[test]
fn test_init_env_encryption_key_reuses_sqlite_key() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_env_key_cache_for_tests();
    let conn = setup_test_db();
    let existing = "aa".repeat(32);
    db::set_setting(&conn, ENCRYPTION_KEY_SETTING, &existing).unwrap();
    assert_eq!(init_env_encryption_key(&conn).unwrap(), existing);
}

#[test]
fn test_init_env_encryption_key_rejects_invalid_sqlite_key() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_env_key_cache_for_tests();
    let conn = setup_test_db();
    db::set_setting(&conn, ENCRYPTION_KEY_SETTING, "not-hex").unwrap();
    let err = init_env_encryption_key(&conn).unwrap_err();
    assert!(err.to_string().contains("invalid encryption key hex"));
}

#[test]
fn metadata_never_contains_secret_or_ciphertext() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_env_key_cache_for_tests();
    let conn = setup_test_db();
    let key = init_env_encryption_key(&conn).unwrap();
    create_profile(&conn, "local").unwrap();
    let profile_id: i64 = conn
        .query_row(
            "SELECT id FROM env_profiles WHERE name = 'local'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    set_variable(&conn, profile_id, "API_TOKEN", "fixture-secret", &key).unwrap();

    let ciphertext: String = conn
        .query_row(
            "SELECT value_encrypted FROM env_variables WHERE profile_id = ?1 AND key = 'API_TOKEN'",
            rusqlite::params![profile_id],
            |row| row.get(0),
        )
        .unwrap();
    let serialized = serde_json::to_string(&list_profiles(&conn).unwrap()).unwrap();
    assert!(!serialized.contains("fixture-secret"));
    assert!(!serialized.contains(&ciphertext));
    assert!(!serialized.contains("value_encrypted"));
    assert!(serialized.contains("API_TOKEN"));
    assert!(serialized.contains("masked"));
}

#[test]
fn injection_is_host_only_and_decryption_errors_are_explicit() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_env_key_cache_for_tests();
    let conn = setup_test_db();
    let key = init_env_encryption_key(&conn).unwrap();
    create_profile(&conn, "local").unwrap();
    let profile_id: i64 = conn
        .query_row(
            "SELECT id FROM env_profiles WHERE name = 'local'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    set_variable(&conn, profile_id, "API_TOKEN", "fixture-secret", &key).unwrap();
    set_variable(&conn, profile_id, "SECOND_TOKEN", "second-secret", &key).unwrap();

    let mut env = std::collections::HashMap::new();
    inject_env(&conn, profile_id, &key, &mut env).unwrap();
    assert_eq!(env.get("API_TOKEN"), Some(&"fixture-secret".to_string()));

    conn.execute(
        "UPDATE env_variables SET value_encrypted = 'v2:00' WHERE profile_id = ?1 AND key = 'API_TOKEN'",
        rusqlite::params![profile_id],
    )
    .unwrap();
    let error = get_variables(&conn, profile_id, &key).unwrap_err();
    assert!(error.to_string().contains("failed to decrypt"));
    let mut failed_env =
        std::collections::HashMap::from([("EXISTING".to_string(), "preserved".to_string())]);
    assert!(inject_env(&conn, profile_id, &key, &mut failed_env).is_err());
    assert_eq!(
        failed_env,
        std::collections::HashMap::from([("EXISTING".to_string(), "preserved".to_string())])
    );
    assert!(get_variables(&conn, profile_id + 999, &key).is_err());
}

#[test]
fn profile_and_key_validation_and_default_delete_invariant() {
    let _guard = TEST_MUTEX.lock().unwrap();
    reset_env_key_cache_for_tests();
    let conn = setup_test_db();
    let key = init_env_encryption_key(&conn).unwrap();
    assert!(create_profile(&conn, "").is_err());
    assert!(create_profile(&conn, "bad\nname").is_err());
    create_profile(&conn, "local").unwrap();
    create_profile(&conn, "ci").unwrap();
    assert!(set_default_profile(&conn, "missing").is_err());
    set_default_profile(&conn, "local").unwrap();
    assert!(matches!(
        delete_profile(&conn, "local"),
        Err(Error::Conflict(_))
    ));
    assert!(set_variable(&conn, 1, "bad-key", "value", &key).is_err());
    assert!(set_variable(&conn, 999, "VALID_KEY", "value", &key).is_err());
    set_default_profile(&conn, "ci").unwrap();
    delete_profile(&conn, "local").unwrap();
}
