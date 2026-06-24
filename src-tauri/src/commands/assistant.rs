use rusqlite::params;
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
    project_id: Option<String>,
) -> Result<Vec<AssistantSession>, String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let query = match project_id {
        Some(_) => "SELECT s.id, s.project_id, s.title, s.model_id, s.provider_id, s.created_at, s.updated_at, s.summary, s.token_used, s.status,
                    (SELECT COUNT(*) FROM assistant_messages m WHERE m.session_id = s.id) as message_count
             FROM assistant_sessions s
             WHERE s.project_id = ?1
             ORDER BY s.updated_at DESC",
        None => "SELECT s.id, s.project_id, s.title, s.model_id, s.provider_id, s.created_at, s.updated_at, s.summary, s.token_used, s.status,
                    (SELECT COUNT(*) FROM assistant_messages m WHERE m.session_id = s.id) as message_count
             FROM assistant_sessions s
             WHERE s.project_id IS NULL
             ORDER BY s.updated_at DESC",
    };

    let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;

    let session_rows = if let Some(ref pid) = project_id {
        stmt.query_map(params![pid], row_to_session)
    } else {
        stmt.query_map([], row_to_session)
    }.map_err(|e| e.to_string())?;

    Ok(session_rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
pub fn assistant_get_messages(
    session_id: String,
) -> Result<Vec<AssistantMessage>, String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, content, tool_calls, tool_result, status, token_count, created_at, sequence
         FROM assistant_messages
         WHERE session_id = ?1
         ORDER BY sequence ASC"
    ).map_err(|e| e.to_string())?;

    let rows = stmt.query_map(params![session_id], |row| {
        Ok(AssistantMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: row.get(2)?,
            content: row.get(3)?,
            tool_calls: row.get(4)?,
            tool_result: row.get(5)?,
            status: row.get(6)?,
            token_count: row.get(7)?,
            created_at: row.get(8)?,
            sequence: row.get(9)?,
        })
    }).map_err(|e| e.to_string())?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
pub fn assistant_create_session(
    project_id: Option<String>,
    title: String,
    model_id: String,
    provider_id: String,
    runtime_override: Option<String>,
) -> Result<AssistantSession, String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let id = uuid_v4();
    let now = chrono_now();

    conn.execute(
        "INSERT INTO assistant_sessions (id, project_id, title, model_id, provider_id, runtime_override, created_at, updated_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active')",
        params![id, project_id, title, model_id, provider_id, runtime_override, now, now],
    ).map_err(|e| e.to_string())?;

    Ok(AssistantSession {
        id,
        project_id,
        title,
        model_id,
        provider_id,
        created_at: now.clone(),
        updated_at: now,
        summary: String::new(),
        token_used: 0,
        status: "active".to_string(),
        message_count: 0,
    })
}

#[tauri::command]
pub fn assistant_delete_session(
    session_id: String,
) -> Result<(), String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM assistant_sessions WHERE id = ?1",
        params![session_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn assistant_save_message(
    session_id: String,
    role: String,
    content: String,
    tool_calls: Option<String>,
    tool_result: Option<String>,
    status: String,
    token_count: i64,
) -> Result<AssistantMessage, String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let id = uuid_v4();
    let now = chrono_now();

    // Get next sequence number for this session
    let max_seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM assistant_messages WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    conn.execute(
        "INSERT INTO assistant_messages (id, session_id, role, content, tool_calls, tool_result, status, token_count, created_at, sequence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, session_id, role, content, tool_calls, tool_result, status, token_count, now, max_seq],
    ).map_err(|e| e.to_string())?;

    // Update session's updated_at
    conn.execute(
        "UPDATE assistant_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now, session_id],
    ).map_err(|e| e.to_string())?;

    Ok(AssistantMessage {
        id,
        session_id,
        role,
        content,
        tool_calls,
        tool_result,
        status,
        token_count,
        created_at: now,
        sequence: max_seq,
    })
}

#[tauri::command]
pub fn assistant_update_message_status(
    message_id: String,
    status: String,
    tool_result: Option<String>,
) -> Result<(), String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE assistant_messages SET status = ?1, tool_result = COALESCE(?2, tool_result) WHERE id = ?3",
        params![status, tool_result, message_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn assistant_update_session_title(
    session_id: String,
    title: String,
) -> Result<(), String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;

    let now = chrono_now();
    conn.execute(
        "UPDATE assistant_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, session_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn assistant_update_session_model(
    session_id: String,
    model_id: String,
    provider_id: String,
) -> Result<(), String> {
    let conn = crate::db::get_assistant_db_conn().map_err(|e| e.to_string())?;
    let now = chrono_now();
    conn.execute(
        "UPDATE assistant_sessions SET model_id = ?1, provider_id = ?2, updated_at = ?3 WHERE id = ?4",
        params![model_id, provider_id, now, session_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
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

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<AssistantSession> {
    Ok(AssistantSession {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        model_id: row.get(3)?,
        provider_id: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        summary: row.get(7)?,
        token_used: row.get(8)?,
        status: row.get(9)?,
        message_count: row.get(10)?,
    })
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
        assert!(now.ends_with('Z'));
    }
}
