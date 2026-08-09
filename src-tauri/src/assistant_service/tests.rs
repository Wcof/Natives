//! Facade and host-owned routing tests.
use super::*;
use std::sync::{Mutex, OnceLock};

/// Serialise tests that mutate NATIVES_DAEMON_MODE / NATIVES_ASSISTANT_DB_PATH.
fn daemon_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn host_owned_router_table() {
    assert!(is_host_owned_method("provider.list"));
    assert!(is_host_owned_method("daemon.getCapabilities"));
    assert!(is_host_owned_method("run.start"));
    assert!(!is_host_owned_method("permission.respond"));
    assert!(!is_host_owned_method("permission.listPending"));
    assert!(daemon_owned_method("permission.respond"));
    assert!(daemon_owned_method("permission.listPending"));
    assert!(is_host_owned_method("artifact.open"));
    assert!(is_host_owned_method("artifact.reveal"));
    assert!(is_host_owned_method("artifact.list"));
    assert!(!is_host_owned_method("run.cancel"));
    assert!(!is_host_owned_method("conversation.list"));
    assert!(!is_host_owned_method("promptQueue.list"));
    assert!(!is_host_owned_method("mcp.list"));
    assert!(daemon_owned_method("conversation.list"));
    assert!(daemon_owned_method("run.cancel"));
    assert!(!daemon_owned_method("run.start"));
    assert!(!daemon_owned_method("provider.list"));
}

#[test]
fn provider_list_is_host_owned_not_daemon_owned() {
    let previous = std::env::var("NATIVES_DAEMON_MODE").ok();
    std::env::set_var("NATIVES_DAEMON_MODE", "uds");
    assert!(
        !daemon_owned_method("provider.list"),
        "provider.list must stay host-owned so user configs are visible"
    );
    assert!(daemon_owned_method("provider.test"));
    if let Some(value) = previous {
        std::env::set_var("NATIVES_DAEMON_MODE", value);
    } else {
        std::env::remove_var("NATIVES_DAEMON_MODE");
    }
}

/// provider.list reads the natives.db Settings SoT (user_providers + active
/// keys) — the historical `assistant_*` mirror fallback is retired (MIG-004).
#[tokio::test]
async fn provider_list_reads_natives_db_settings_sot() {
    let _g = daemon_env_lock();
    let main_pool_dir =
        std::env::temp_dir().join(format!("natives-prov-main-{}.db", uuid::Uuid::new_v4()));
    let main_pool = crate::db::init_db_pool(&main_pool_dir).expect("init main pool");
    {
        let conn = main_pool.get().expect("main conn");
        conn.execute_batch(
            "ALTER TABLE user_providers ADD COLUMN default_model TEXT;
             ALTER TABLE provider_api_keys ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
             INSERT INTO user_providers
                 (id, preset_name, api_protocol, name, website_url, base_url,
                  default_model, created_at, updated_at)
             VALUES ('p1', 'openai_compatible', 'openai_chat_completions', 'SenseNova',
                     '', 'https://api.example/v1', 'deepseek-v4-flash',
                     datetime('now'), datetime('now'));
             INSERT INTO provider_api_keys
                 (id, provider_id, label, api_key_encrypted, dek_encrypted,
                  is_active, created_at)
             VALUES ('k1', 'p1', 'API Key', 'enc', 'dek', 1, datetime('now'));",
        )
        .expect("seed natives.db SoT provider");
    }
    crate::db::register_main_pool(main_pool);

    // W1: the provider mirror lives in the Host's own natives.db (created by
    // init_db_pool → ensure_host_owned_tables), so the test only registers the
    // main pool — the old assistant.db pool no longer exists.

    let response = dispatch_rpc("provider.list", &serde_json::json!({})).await;
    assert!(
        response.success,
        "provider.list failed: {:?}",
        response.error
    );
    let data = response.data.expect("provider.list data");
    let providers = data
        .get("providers")
        .and_then(|v| v.as_array())
        .expect("providers array");
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["id"], "p1");
    assert_eq!(providers[0]["has_active_key"], true);
    // Empty model cache → default_model surfaced (never a fabricated entry).
    assert_eq!(providers[0]["models"][0]["id"], "deepseek-v4-flash");

    crate::db::clear_main_pool_for_tests();
    let _ = std::fs::remove_file(&main_pool_dir);
}

#[test]
fn run_start_is_always_host_owned() {
    let previous = std::env::var("NATIVES_DAEMON_MODE").ok();
    std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
    assert!(!daemon_owned_method("run.start"));
    std::env::set_var("NATIVES_DAEMON_MODE", "uds");
    assert!(!daemon_owned_method("run.start"));
    if let Some(value) = previous {
        std::env::set_var("NATIVES_DAEMON_MODE", value);
    } else {
        std::env::remove_var("NATIVES_DAEMON_MODE");
    }
    assert!(daemon_owned_method("run.list"));
    assert!(daemon_owned_method("run.cancel"));
    assert!(daemon_owned_method("provider.test"));
    assert!(daemon_owned_method("mcp.list"));
    assert!(daemon_owned_method("conversation.list"));
    assert!(daemon_owned_method("promptQueue.list"));
    assert!(!daemon_owned_method("artifact.open"));
}

#[tokio::test]
async fn implemented_daemon_method_is_not_rejected_by_legacy_dispatch() {
    let response = dispatch_rpc("mcp.list", &serde_json::json!({})).await;
    assert_ne!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("METHOD_NOT_FOUND")
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)] // 序列化 env 变更，避免并行测试互相覆盖
async fn conversation_permission_and_attachments_round_trip() {
    // Serialise env mutations so parallel natives tests cannot clobber the
    // embedded daemon DB path mid-flight.
    let _env_guard = daemon_env_lock();
    let previous_daemon_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
    let previous_db = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
    let tmp_db =
        std::env::temp_dir().join(format!("natives-asst-test-{}.db", uuid::Uuid::new_v4()));
    std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
    std::env::set_var(
        "NATIVES_ASSISTANT_DB_PATH",
        tmp_db.to_string_lossy().as_ref(),
    );
    crate::daemon_authority::reset_authority_cache().await;
    // S3 (EXECUTION-POLICY-V1): run.start resolves the execution policy from
    // the main DB Settings authority (P0-13 fail-closed — no silent defaults),
    // so the test must provide a real main DB pool.
    let main_pool_dir =
        std::env::temp_dir().join(format!("natives-asst-main-{}.db", uuid::Uuid::new_v4()));
    let main_pool = crate::db::init_db_pool(&main_pool_dir).expect("init main pool");
    // Seed the natives.db SoT provider surface the S3 policy resolver and the
    // run.start preflight (`provider_model_pair_available`) read. Fresh
    // natives.db lacks the migrated columns, so add them idempotently.
    {
        let conn = main_pool.get().expect("main conn");
        conn.execute_batch(
            "ALTER TABLE user_providers ADD COLUMN default_model TEXT;
             ALTER TABLE provider_api_keys ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
             INSERT INTO user_providers
                 (id, preset_name, api_protocol, name, website_url, base_url,
                  default_model, created_at, updated_at)
             VALUES ('provider', 'Provider', 'openai_chat_completions', 'Provider',
                     '', '', 'model', datetime('now'), datetime('now'));
             INSERT INTO provider_api_keys
                 (id, provider_id, label, api_key_encrypted, dek_encrypted,
                  is_active, created_at)
             VALUES ('key', 'provider', '', 'enc', 'dek', 1, datetime('now'));",
        )
        .expect("seed natives.db SoT provider");
    }
    crate::db::register_main_pool(main_pool);
    let attachment_path = std::path::PathBuf::from(format!(
        "/tmp/natives-assistant-test-{}.txt",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&attachment_path, "example attachment").unwrap();
    let created = dispatch_rpc(
        "conversation.create",
        &serde_json::json!({
            "mode": "agent",
            "title": "Attachment test",
            "provider_id": "provider",
            "model_id": "model",
            "project_id": "/project/test",
            "permission_profile_id": "readonly"
        }),
    )
    .await;
    assert!(created.success, "create failed: {:?}", created.error);
    let created_data = created.data.as_ref().expect("create data");
    let conversation_id = created_data["id"].as_str().unwrap().to_string();
    assert_eq!(created_data["permission_profile_id"], "readonly");

    let got = dispatch_rpc(
        "conversation.get",
        &serde_json::json!({ "id": conversation_id }),
    )
    .await;
    assert!(got.success, "conversation.get failed: {:?}", got.error);
    assert_eq!(
        got.data.as_ref().unwrap()["permission_profile_id"],
        "readonly"
    );

    let started = dispatch_rpc(
        "run.start",
        &serde_json::json!({
            "conversation_id": conversation_id,
            "provider_id": "provider",
            "model_id": "model",
            "project_path": "/tmp",
            "content": "Inspect this file",
            "attachments": [{
                "path": attachment_path.to_string_lossy().to_string(),
                "name": "example.txt",
                "mime_type": "text/plain",
                "size": 12
            }]
        }),
    )
    .await;
    assert!(started.success, "run.start failed: {:?}", started.error);
    let run = started.data.as_ref().unwrap();
    assert_eq!(run["permission_profile"], "readonly", "run payload: {run}");
    let run_id = run["id"].as_str().unwrap();

    let conversations = dispatch_rpc("conversation.list", &Value::Null)
        .await
        .data
        .unwrap();
    let listed = conversations.as_array().cloned().unwrap_or_else(|| {
        conversations
            .get("conversations")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    });
    let found = listed.iter().find(|c| c["id"] == conversation_id);
    assert!(found.is_some(), "conversation list missing id: {listed:?}");
    assert_eq!(found.unwrap()["permission_profile_id"], "readonly");

    let runs = dispatch_rpc(
        "run.list",
        &serde_json::json!({ "conversation_id": conversation_id }),
    )
    .await
    .data
    .unwrap();
    let run_list = runs
        .get("runs")
        .and_then(|v| v.as_array())
        .or_else(|| runs.as_array())
        .expect("runs array");
    assert!(!run_list.is_empty(), "expected at least one run: {runs}");
    assert_eq!(run_list[0]["permission_profile"], "readonly");
    assert_eq!(run_list[0]["id"], run_id);

    let messages = dispatch_rpc(
        "conversation.getMessages",
        &serde_json::json!({ "conversation_id": conversation_id }),
    )
    .await
    .data
    .unwrap();
    let msg_list = messages.as_array().cloned().unwrap_or_else(|| {
        messages
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    });
    assert!(!msg_list.is_empty(), "expected user message: {messages}");

    let _ = std::fs::remove_file(&attachment_path);
    let _ = std::fs::remove_file(&tmp_db);
    crate::daemon_authority::reset_authority_cache().await;
    crate::db::clear_main_pool_for_tests();
    if let Some(mode) = previous_daemon_mode {
        std::env::set_var("NATIVES_DAEMON_MODE", mode);
    } else {
        std::env::remove_var("NATIVES_DAEMON_MODE");
    }
    if let Some(db) = previous_db {
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", db);
    } else {
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)] // 序列化 env 变更，避免并行测试互相覆盖
async fn structured_assistant_blocks_round_trip() {
    let _env_guard = daemon_env_lock();
    let previous_daemon_mode = std::env::var("NATIVES_DAEMON_MODE").ok();
    let previous_db = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
    let tmp_db =
        std::env::temp_dir().join(format!("natives-blocks-test-{}.db", uuid::Uuid::new_v4()));
    std::env::set_var("NATIVES_DAEMON_MODE", "embedded");
    std::env::set_var(
        "NATIVES_ASSISTANT_DB_PATH",
        tmp_db.to_string_lossy().as_ref(),
    );
    crate::daemon_authority::reset_authority_cache().await;
    let created = dispatch_rpc(
        "conversation.create",
        &serde_json::json!({
            "mode": "agent", "title": "Blocks", "provider_id": "p", "model_id": "m", "project_id": "/project/test"
        }),
    )
    .await;
    assert!(created.success, "create failed: {:?}", created.error);
    let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();
    let appended = dispatch_rpc(
        "conversation.appendMessage",
        &serde_json::json!({
            "conversation_id": conversation_id,
            "role": "assistant",
            "blocks": [{ "type": "reasoning", "reasoning": "checked" }, { "type": "text", "text": "done" }]
        }),
    )
    .await;
    assert!(appended.success, "append failed: {:?}", appended.error);
    let messages = dispatch_rpc(
        "conversation.getMessages",
        &serde_json::json!({ "conversation_id": conversation_id }),
    )
    .await
    .data
    .unwrap();
    let msg_list = messages.as_array().cloned().unwrap_or_else(|| {
        messages
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    });
    assert!(!msg_list.is_empty(), "messages: {messages}");
    assert_eq!(
        msg_list[0]["content_blocks"][0]["content"]["reasoning"],
        "checked"
    );
    assert_eq!(msg_list[0]["content_blocks"][1]["content"]["text"], "done");
    let _ = std::fs::remove_file(&tmp_db);
    crate::daemon_authority::reset_authority_cache().await;
    if let Some(mode) = previous_daemon_mode {
        std::env::set_var("NATIVES_DAEMON_MODE", mode);
    } else {
        std::env::remove_var("NATIVES_DAEMON_MODE");
    }
    if let Some(db) = previous_db {
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", db);
    } else {
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
    }
}
