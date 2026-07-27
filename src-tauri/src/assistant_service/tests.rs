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
    assert!(is_host_owned_method("run.subscribe"));
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

#[tokio::test]
async fn provider_list_returns_mirrored_user_provider_models() {
    let store = Arc::new(DataStore::new(":memory:").unwrap());
    store
        .conn()
        .execute(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, default_model, health_status, created_at, updated_at)
             VALUES ('p1', 'openai_compatible', 'SenseNova', 'https://api.example/v1', 'deepseek-v4-flash', 'unknown', 'now', 'now')",
            [],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, created_at)
             VALUES ('k1', 'p1', 'enc', 'sk-…abcd', 'API Key', 1, 'now')",
            [],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES ('p1:deepseek-v4-flash', 'p1', 'deepseek-v4-flash', 'DeepSeek V4 Flash', '{}', 0, 0, 'manual', 'now')",
            [],
        )
        .unwrap();

    let response = dispatch_rpc(&store, "provider.list", &serde_json::json!({})).await;
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
    assert_eq!(providers[0]["models"][0]["id"], "deepseek-v4-flash");
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
    assert!(!daemon_owned_method("run.subscribe"));
    assert!(daemon_owned_method("provider.test"));
    assert!(daemon_owned_method("mcp.list"));
    assert!(daemon_owned_method("conversation.list"));
    assert!(daemon_owned_method("promptQueue.list"));
    assert!(!daemon_owned_method("artifact.open"));
}

#[tokio::test]
async fn implemented_daemon_method_is_not_rejected_by_legacy_dispatch() {
    let store = Arc::new(DataStore::new(":memory:").unwrap());
    let response = dispatch_rpc(&store, "mcp.list", &serde_json::json!({})).await;
    assert_ne!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("METHOD_NOT_FOUND")
    );
}

#[tokio::test]
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
    let store = Arc::new(DataStore::new(":memory:").unwrap());
    let attachment_path = std::path::PathBuf::from(format!(
        "/tmp/natives-assistant-test-{}.txt",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&attachment_path, "example attachment").unwrap();
    let created = dispatch_rpc(
        &store,
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
        &store,
        "conversation.get",
        &serde_json::json!({ "id": conversation_id }),
    )
    .await;
    assert!(got.success, "conversation.get failed: {:?}", got.error);
    assert_eq!(
        got.data.as_ref().unwrap()["permission_profile_id"],
        "readonly"
    );

    store.conn().execute("INSERT INTO assistant_provider_configs (id, provider_type, display_name, api_base_url, created_at, updated_at) VALUES ('provider', 'openai', 'Provider', 'https://example.com', datetime('now'), datetime('now'))", []).unwrap();
    store.conn().execute("INSERT INTO assistant_provider_keys (id, provider_id, encrypted_key, masked_key, created_at) VALUES ('key', 'provider', 'encrypted', '***', datetime('now'))", []).unwrap();
    store.conn().execute("INSERT INTO assistant_model_cache (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at) VALUES ('model-cache', 'provider', 'model', 'model', '{}', 0, 0, 'manual', datetime('now'))", []).unwrap();

    let started = dispatch_rpc(
        &store,
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

    let conversations = dispatch_rpc(&store, "conversation.list", &Value::Null)
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
        &store,
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
        &store,
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
    let store = Arc::new(DataStore::new(":memory:").unwrap());
    let created = dispatch_rpc(
        &store,
        "conversation.create",
        &serde_json::json!({
            "mode": "agent", "title": "Blocks", "provider_id": "p", "model_id": "m", "project_id": "/project/test"
        }),
    )
    .await;
    assert!(created.success, "create failed: {:?}", created.error);
    let conversation_id = created.data.unwrap()["id"].as_str().unwrap().to_string();
    let appended = dispatch_rpc(&store, "conversation.appendMessage", &serde_json::json!({
        "conversation_id": conversation_id,
        "role": "assistant",
        "blocks": [{ "type": "reasoning", "reasoning": "checked" }, { "type": "text", "text": "done" }]
    })).await;
    assert!(appended.success, "append failed: {:?}", appended.error);
    let messages = dispatch_rpc(
        &store,
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
