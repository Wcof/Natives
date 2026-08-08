//! Legacy assistant_* CRUD commands — RETIRED (T202, P1-004/P1-005).
//!
//! Host no longer reads/writes the legacy `assistant_sessions` /
//! `assistant_messages` tables: conversations/messages/runs are canonical in
//! the Agent Daemon (`assistant.db`). These commands fail closed so any stale
//! caller surfaces an explicit error instead of touching retired tables. The
//! `rpc_server.rs` orphan (dead code, 0 references) was physically deleted.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSession {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub model_id: String,
    pub provider_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub summary: String,
    pub token_used: i64,
    pub status: String,
    pub message_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_result: Option<String>,
    pub status: String,
    pub token_count: i64,
    pub created_at: String,
    pub sequence: i64,
}

#[tauri::command]
pub fn assistant_list_sessions(
    _project_id: Option<String>,
) -> Result<Vec<AssistantSession>, String> {
    Err("assistant_list_sessions retired: sessions are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_get_messages(_session_id: String) -> Result<Vec<AssistantMessage>, String> {
    Err("assistant_get_messages retired: messages are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_create_session(
    _project_id: Option<String>,
    _title: String,
    _model_id: String,
    _provider_id: String,
    _runtime_override: Option<String>,
) -> Result<AssistantSession, String> {
    Err("assistant_create_session retired: sessions are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_delete_session(_session_id: String) -> Result<(), String> {
    Err("assistant_delete_session retired: sessions are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_save_message(
    _session_id: String,
    _role: String,
    _content: String,
    _tool_calls: Option<String>,
    _tool_result: Option<String>,
    _status: String,
    _token_count: i64,
) -> Result<AssistantMessage, String> {
    Err("assistant_save_message retired: messages are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_update_message_status(
    _message_id: String,
    _status: String,
    _tool_result: Option<String>,
) -> Result<(), String> {
    Err("assistant_update_message_status retired: messages are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_update_session_title(_session_id: String, _title: String) -> Result<(), String> {
    Err("assistant_update_session_title retired: sessions are Daemon-canonical (T202)".into())
}

#[tauri::command]
pub fn assistant_update_session_model(
    _session_id: String,
    _model_id: String,
    _provider_id: String,
) -> Result<(), String> {
    Err("assistant_update_session_model retired: sessions are Daemon-canonical (T202)".into())
}

pub fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    let mut buf = bytes;
    buf[6] = (buf[6] & 0x0f) | 0x40;
    buf[8] = (buf[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[0], buf[1], buf[2], buf[3],
        buf[4], buf[5],
        buf[6], buf[7],
        buf[8], buf[9],
        buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
    )
}

pub fn chrono_now() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assistant_session_serialization() {
        let session = AssistantSession {
            id: "test-id".to_string(),
            project_id: Some("proj-1".to_string()),
            title: "Test Session".to_string(),
            model_id: "gpt-4".to_string(),
            provider_id: "prov-1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            summary: "".to_string(),
            token_used: 0,
            status: "active".to_string(),
            message_count: 0,
        };

        let json = serde_json::to_value(&session).unwrap();
        assert_eq!(json["id"], "test-id");
        assert_eq!(json["projectId"], "proj-1");
        assert_eq!(json["title"], "Test Session");
        assert_eq!(json["modelId"], "gpt-4");
    }

    #[test]
    fn test_assistant_message_serialization() {
        let msg = AssistantMessage {
            id: "msg-1".to_string(),
            session_id: "session-1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_result: None,
            status: "done".to_string(),
            token_count: 10,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            sequence: 1,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["sequence"], 1);
        assert_eq!(json["status"], "done");
    }

    #[test]
    fn test_uuid_v4_format() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|&c| c == '-').count(), 4);
        assert_eq!(&id[14..15], "4");
    }

    #[test]
    fn test_chrono_now_format() {
        let now = chrono_now();
        assert!(now.len() >= 20);
    }

    #[test]
    fn retired_commands_fail_closed() {
        assert!(assistant_list_sessions(None).is_err());
        assert!(assistant_get_messages("s".into()).is_err());
        assert!(assistant_create_session(None, "t".into(), "m".into(), "p".into(), None).is_err());
        assert!(assistant_delete_session("s".into()).is_err());
        assert!(assistant_save_message(
            "s".into(), "user".into(), "c".into(), None, None, "done".into(), 0
        )
        .is_err());
        assert!(assistant_update_message_status("m".into(), "done".into(), None).is_err());
        assert!(assistant_update_session_title("s".into(), "t".into()).is_err());
        assert!(assistant_update_session_model("s".into(), "m".into(), "p".into()).is_err());
    }
}
