use crate::{Error, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use crate::AppState;

// ── Data types ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub description: String,
    pub content: String,
    pub source_url: String,
    pub item_type: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub total_items: i64,
    pub total_folders: i64,
    pub total_tags: i64,
    pub items_by_folder: Vec<CountByFolder>,
    pub recent_items: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountByFolder {
    pub folder_id: Option<String>,
    pub folder_name: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFolderInput {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFolderInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTagInput {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateItemInput {
    pub folder_id: Option<String>,
    pub title: String,
    pub description: String,
    pub content: String,
    pub source_url: String,
    pub item_type: String,
    pub status: String,
    pub tag_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateItemInput {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub description: String,
    pub content: String,
    pub source_url: String,
    pub status: String,
    pub tag_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    pub folder_id: Option<String>,
    pub tag_id: Option<String>,
    pub keyword: Option<String>,
    pub status: Option<String>,
    pub item_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchTagInput {
    pub item_ids: Vec<String>,
    pub tag_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMoveInput {
    pub item_ids: Vec<String>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteInput {
    pub item_ids: Vec<String>,
}

// ── Helpers ──

fn uuid_v4() -> String {
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

fn chrono_now() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Ensure library tables exist (called on first use, idempotent).
pub fn ensure_tables(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS library_folders (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            parent_id TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS library_tags (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            color TEXT NOT NULL DEFAULT '#6366f1',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS library_items (
            id TEXT PRIMARY KEY,
            folder_id TEXT REFERENCES library_folders(id) ON DELETE SET NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            source_url TEXT NOT NULL DEFAULT '',
            item_type TEXT NOT NULL DEFAULT 'note',
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS library_item_tags (
            item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
            tag_id TEXT NOT NULL REFERENCES library_tags(id) ON DELETE CASCADE,
            PRIMARY KEY (item_id, tag_id)
        );
        CREATE INDEX IF NOT EXISTS idx_library_items_folder
            ON library_items(folder_id);
        CREATE INDEX IF NOT EXISTS idx_library_items_status
            ON library_items(status);
        CREATE INDEX IF NOT EXISTS idx_library_items_created
            ON library_items(created_at);
        CREATE INDEX IF NOT EXISTS idx_library_folders_parent
            ON library_folders(parent_id);"
    )
}

// ── Folder Commands ──

#[tauri::command]
pub fn library_list_folders(
    state: State<'_, AppState>,
) -> Result<Vec<Folder>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, name, parent_id, sort_order, created_at, updated_at
         FROM library_folders ORDER BY sort_order, name"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let folders = stmt.query_map([], |row| {
        Ok(Folder {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            sort_order: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    }).map_err(|e| Error::Internal(e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(folders)
}

#[tauri::command]
pub fn library_create_folder(
    state: State<'_, AppState>,
    input: CreateFolderInput,
) -> Result<Folder> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let id = uuid_v4();
    let now = chrono_now();
    conn.execute(
        "INSERT INTO library_folders (id, name, parent_id, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        params![id, input.name, input.parent_id, now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(Folder {
        id,
        name: input.name,
        parent_id: input.parent_id,
        sort_order: 0,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn library_update_folder(
    state: State<'_, AppState>,
    input: UpdateFolderInput,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    let now = chrono_now();
    conn.execute(
        "UPDATE library_folders SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![input.name, now, input.id],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn library_delete_folder(
    state: State<'_, AppState>,
    id: String,
    move_items: bool,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;

    if move_items {
        // Move items to no folder before deleting
        conn.execute(
            "UPDATE library_items SET folder_id = NULL WHERE folder_id = ?1",
            params![id],
        ).map_err(|e| Error::Internal(e.to_string()))?;
    }
    // Cascade for sub-folders: set parent_id to NULL
    conn.execute(
        "UPDATE library_folders SET parent_id = NULL WHERE parent_id = ?1",
        params![id],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    conn.execute(
        "DELETE FROM library_folders WHERE id = ?1",
        params![id],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

// ── Tag Commands ──

#[tauri::command]
pub fn library_list_tags(
    state: State<'_, AppState>,
) -> Result<Vec<Tag>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, name, color, created_at FROM library_tags ORDER BY name"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let tags = stmt.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            created_at: row.get(3)?,
        })
    }).map_err(|e| Error::Internal(e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();
    Ok(tags)
}

#[tauri::command]
pub fn library_create_tag(
    state: State<'_, AppState>,
    input: CreateTagInput,
) -> Result<Tag> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let id = uuid_v4();
    let now = chrono_now();
    conn.execute(
        "INSERT INTO library_tags (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, input.name, input.color, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(Tag { id, name: input.name, color: input.color, created_at: now })
}

#[tauri::command]
pub fn library_delete_tag(
    state: State<'_, AppState>,
    id: String,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    conn.execute("DELETE FROM library_tags WHERE id = ?1", params![id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

// ── Item Commands ──

fn item_from_row(row: &rusqlite::Row) -> rusqlite::Result<LibraryItem> {
    Ok(LibraryItem {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        content: row.get(4)?,
        source_url: row.get(5)?,
        item_type: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        tags: vec![],
    })
}

fn load_tags_for_item(conn: &rusqlite::Connection, item_id: &str) -> Vec<Tag> {
    let mut stmt = match conn.prepare(
        "SELECT t.id, t.name, t.color, t.created_at
         FROM library_tags t
         JOIN library_item_tags it ON t.id = it.tag_id
         WHERE it.item_id = ?1
         ORDER BY t.name"
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params![item_id], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            created_at: row.get(3)?,
        })
    }).ok()
    .map(|iter| iter.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

#[tauri::command]
pub fn library_list_items(
    state: State<'_, AppState>,
    filter: ItemFilter,
) -> Result<Vec<LibraryItem>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let limit = filter.limit.unwrap_or(50).min(200);
    let offset = filter.offset.unwrap_or(0);

    // Build dynamic query
    let mut conditions: Vec<String> = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(fid) = &filter.folder_id {
        let idx = params_vec.len() + 1;
        conditions.push(format!("i.folder_id = ?{idx}"));
        params_vec.push(Box::new(fid.clone()));
    }
    if let Some(s) = &filter.status {
        let idx = params_vec.len() + 1;
        conditions.push(format!("i.status = ?{idx}"));
        params_vec.push(Box::new(s.clone()));
    }
    if let Some(t) = &filter.item_type {
        let idx = params_vec.len() + 1;
        conditions.push(format!("i.item_type = ?{idx}"));
        params_vec.push(Box::new(t.clone()));
    }
    if let Some(kw) = &filter.keyword {
        let idx = params_vec.len() + 1;
        conditions.push(format!("(i.title LIKE ?{idx} OR i.description LIKE ?{idx})"));
        params_vec.push(Box::new(format!("%{}%", kw)));
    }

    // If filtering by tag, join through item_tags
    let tag_join = if filter.tag_id.is_some() {
        " JOIN library_item_tags it2 ON i.id = it2.item_id"
    } else {
        ""
    };
    if let Some(tid) = &filter.tag_id {
        let idx = params_vec.len() + 1;
        conditions.push(format!("it2.tag_id = ?{idx}"));
        params_vec.push(Box::new(tid.clone()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT i.id, i.folder_id, i.title, i.description, i.content,
                i.source_url, i.item_type, i.status, i.created_at, i.updated_at
         FROM library_items i{tag_join}
         {where_clause}
         ORDER BY i.updated_at DESC
         LIMIT ?{lim} OFFSET ?{off}",
        tag_join = tag_join,
        where_clause = where_clause,
        lim = params_vec.len() + 1,
        off = params_vec.len() + 2,
    );
    params_vec.push(Box::new(limit));
    params_vec.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)
        .map_err(|e| Error::Internal(e.to_string()))?;

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter()
        .map(|p| p.as_ref()).collect();

    let items = stmt.query_map(param_refs.as_slice(), item_from_row)
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .map(|mut item| {
            item.tags = load_tags_for_item(&*conn, &item.id);
            item
        })
        .collect();

    Ok(items)
}

#[tauri::command]
pub fn library_get_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<LibraryItem>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT i.id, i.folder_id, i.title, i.description, i.content,
                i.source_url, i.item_type, i.status, i.created_at, i.updated_at
         FROM library_items i WHERE i.id = ?1"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let mut rows = stmt.query_map(params![id], item_from_row)
        .map_err(|e| Error::Internal(e.to_string()))?;

    if let Some(Ok(mut item)) = rows.next() {
        item.tags = load_tags_for_item(&*conn, &item.id);
        Ok(Some(item))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub fn library_create_item(
    state: State<'_, AppState>,
    input: CreateItemInput,
) -> Result<LibraryItem> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let id = uuid_v4();
    let now = chrono_now();
    conn.execute(
        "INSERT INTO library_items (id, folder_id, title, description, content, source_url, item_type, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, input.folder_id, input.title, input.description, input.content,
                input.source_url, input.item_type, input.status, now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    // Assign tags
    for tag_id in &input.tag_ids {
        conn.execute(
            "INSERT OR IGNORE INTO library_item_tags (item_id, tag_id) VALUES (?1, ?2)",
            params![id, tag_id],
        ).map_err(|e| Error::Internal(e.to_string()))?;
    }

    let tags: Vec<Tag> = input.tag_ids.iter().filter_map(|tid| {
        conn.query_row(
            "SELECT id, name, color, created_at FROM library_tags WHERE id = ?1",
            params![tid],
            |row| Ok(Tag { id: row.get(0)?, name: row.get(1)?, color: row.get(2)?, created_at: row.get(3)? }),
        ).ok()
    }).collect();

    Ok(LibraryItem {
        id,
        folder_id: input.folder_id,
        title: input.title,
        description: input.description,
        content: input.content,
        source_url: input.source_url,
        item_type: input.item_type,
        status: input.status,
        created_at: now.clone(),
        updated_at: now,
        tags,
    })
}

#[tauri::command]
pub fn library_update_item(
    state: State<'_, AppState>,
    input: UpdateItemInput,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    let now = chrono_now();

    conn.execute(
        "UPDATE library_items SET folder_id = ?1, title = ?2, description = ?3,
         content = ?4, source_url = ?5, status = ?6, updated_at = ?7
         WHERE id = ?8",
        params![input.folder_id, input.title, input.description, input.content,
                input.source_url, input.status, now, input.id],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    // Re-assign tags: delete all, then insert
    conn.execute("DELETE FROM library_item_tags WHERE item_id = ?1", params![input.id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    for tag_id in &input.tag_ids {
        conn.execute(
            "INSERT OR IGNORE INTO library_item_tags (item_id, tag_id) VALUES (?1, ?2)",
            params![input.id, tag_id],
        ).map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn library_delete_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    conn.execute("DELETE FROM library_items WHERE id = ?1", params![id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

// ── Bulk Operations ──

#[tauri::command]
pub fn library_batch_tag(
    state: State<'_, AppState>,
    input: BatchTagInput,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    for item_id in &input.item_ids {
        for tag_id in &input.tag_ids {
            conn.execute(
                "INSERT OR IGNORE INTO library_item_tags (item_id, tag_id) VALUES (?1, ?2)",
                params![item_id, tag_id],
            ).map_err(|e| Error::Internal(e.to_string()))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn library_batch_move(
    state: State<'_, AppState>,
    input: BatchMoveInput,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    for item_id in &input.item_ids {
        conn.execute(
            "UPDATE library_items SET folder_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![input.folder_id, chrono_now(), item_id],
        ).map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn library_batch_delete(
    state: State<'_, AppState>,
    input: BatchDeleteInput,
) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    for item_id in &input.item_ids {
        conn.execute("DELETE FROM library_items WHERE id = ?1", params![item_id])
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}

// ── Statistics ──

#[tauri::command]
pub fn library_get_stats(
    state: State<'_, AppState>,
) -> Result<LibraryStats> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let total_items: i64 = conn.query_row(
        "SELECT COUNT(*) FROM library_items", [], |row| row.get(0)
    ).unwrap_or(0);

    let total_folders: i64 = conn.query_row(
        "SELECT COUNT(*) FROM library_folders", [], |row| row.get(0)
    ).unwrap_or(0);

    let total_tags: i64 = conn.query_row(
        "SELECT COUNT(*) FROM library_tags", [], |row| row.get(0)
    ).unwrap_or(0);

    // Items created in last 7 days
    let recent_items: i64 = conn.query_row(
        "SELECT COUNT(*) FROM library_items WHERE created_at >= datetime('now', '-7 days')",
        [], |row| row.get(0)
    ).unwrap_or(0);

    // Items by folder
    let mut stmt = conn.prepare(
        "SELECT f.id, f.name, COUNT(i.id)
         FROM library_folders f
         LEFT JOIN library_items i ON i.folder_id = f.id
         GROUP BY f.id
         ORDER BY COUNT(i.id) DESC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let items_by_folder: Vec<CountByFolder> = stmt.query_map([], |row| {
        Ok(CountByFolder {
            folder_id: Some(row.get::<_, String>(0)?),
            folder_name: row.get(1)?,
            count: row.get(2)?,
        })
    }).map_err(|e| Error::Internal(e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();

    // Also count items without a folder
    let unassigned: i64 = conn.query_row(
        "SELECT COUNT(*) FROM library_items WHERE folder_id IS NULL",
        [], |row| row.get(0)
    ).unwrap_or(0);
    let mut all_by_folder = items_by_folder;
    if unassigned > 0 {
        all_by_folder.push(CountByFolder {
            folder_id: None,
            folder_name: "Unassigned".to_string(),
            count: unassigned,
        });
    }

    Ok(LibraryStats {
        total_items,
        total_folders,
        total_tags,
        items_by_folder: all_by_folder,
        recent_items,
    })
}
