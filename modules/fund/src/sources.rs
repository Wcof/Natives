//! 信息源与投资论点（F06）：链接收藏与用户摘录的 CRUD。
//! 边界：不做自动抓取；kind='link' 仅收藏 URL，kind='excerpt' 为用户已取得
//! 的正文摘录（含取得时间）。来源与时间必须真实可见，不用静态内容充当接入。

use crate::assets::is_valid_calendar_date;
use crate::finance::now_secs;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    Conflict,
    NotFound(String),
    Invalid(String),
    Db(String),
}

impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            SourceError::Conflict => "SRC_CONFLICT",
            SourceError::NotFound(_) => "SRC_NOT_FOUND",
            SourceError::Invalid(_) => "SRC_INVALID",
            SourceError::Db(_) => "SRC_DB",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SourceError::Conflict => "记录已被其他窗口修改，请刷新后重新确认".into(),
            SourceError::NotFound(m) => format!("记录不存在：{m}"),
            SourceError::Invalid(m) => format!("输入无效：{m}"),
            SourceError::Db(e) => format!("数据库错误：{e}"),
        }
    }
}

impl From<rusqlite::Error> for SourceError {
    fn from(e: rusqlite::Error) -> Self {
        SourceError::Db(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoSourceRow {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub kind: String,
    pub related_symbol: String,
    pub thesis_id: Option<i64>,
    pub publisher: String,
    pub published_at: Option<String>,
    pub excerpt_text: String,
    pub excerpt_fetched_at: Option<String>,
    pub read_status: bool,
    pub favorite: bool,
    pub enabled: bool,
    pub archived: bool,
}

fn row_to_source(r: &rusqlite::Row) -> rusqlite::Result<InfoSourceRow> {
    Ok(InfoSourceRow {
        id: r.get(0)?,
        name: r.get(1)?,
        url: r.get(2)?,
        kind: r.get(3)?,
        related_symbol: r.get(4)?,
        thesis_id: r.get(5)?,
        publisher: r.get(6)?,
        published_at: r.get(7)?,
        excerpt_text: r.get(8)?,
        excerpt_fetched_at: r.get(9)?,
        read_status: r.get::<_, i64>(10)? == 1,
        favorite: r.get::<_, i64>(11)? == 1,
        enabled: r.get::<_, i64>(12)? == 1,
        archived: r.get::<_, i64>(13)? == 1,
    })
}

const SOURCE_COLS: &str = "id, name, url, kind, related_symbol, thesis_id, publisher, \
                           published_at, excerpt_text, excerpt_fetched_at, read_status, \
                           favorite, enabled, archived";

pub struct SourceInput {
    pub name: String,
    /// link = 链接收藏；excerpt = 用户摘录（excerpt_text 必填）。
    pub kind: String,
    pub url: String,
    pub related_symbol: String,
    pub thesis_id: Option<i64>,
    pub publisher: String,
    pub published_at: Option<String>,
    pub excerpt_text: String,
}

fn validate_input(conn: &Connection, input: &SourceInput) -> Result<(), SourceError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 128 {
        return Err(SourceError::Invalid("名称需 1~128 字符".into()));
    }
    if input.kind != "link" && input.kind != "excerpt" {
        return Err(SourceError::Invalid(format!("未知类型 {}", input.kind)));
    }
    if input.kind == "link" && input.url.trim().is_empty() {
        return Err(SourceError::Invalid("链接收藏必须提供 URL".into()));
    }
    if input.url.len() > 2048
        || input.excerpt_text.chars().count() > 12000
        || input.related_symbol.len() > 128
        || input.publisher.chars().count() > 128
    {
        return Err(SourceError::Invalid("信息源内容超过长度限制".into()));
    }
    if !input.url.trim().is_empty()
        && !input.url.trim().starts_with("http://")
        && !input.url.trim().starts_with("https://")
    {
        return Err(SourceError::Invalid("URL 必须以 http(s):// 开头".into()));
    }
    if input.kind == "excerpt" && input.excerpt_text.trim().is_empty() {
        return Err(SourceError::Invalid(
            "摘录必须包含正文（链接收藏与已取得正文状态区分）".into(),
        ));
    }
    if let Some(date) = &input.published_at {
        if !is_valid_calendar_date(date) {
            return Err(SourceError::Invalid("发布日期不是有效日历日期".into()));
        }
    }
    if let Some(thesis_id) = input.thesis_id {
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM investment_theses WHERE id=?1 AND archived=0",
                params![thesis_id],
                |r| r.get::<_, i64>(0).map(|_| true),
            )
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(SourceError::NotFound(format!("投资论点 {thesis_id}")));
        }
    }
    Ok(())
}

pub fn create_source(conn: &Connection, input: &SourceInput) -> Result<i64, SourceError> {
    validate_input(conn, input)?;
    let now = now_secs();
    let fetched = if input.kind == "excerpt" {
        Some(now.clone())
    } else {
        None
    };
    conn.execute(
        "INSERT INTO info_sources(name, url, kind, related_symbol, thesis_id, publisher,
                                   published_at, excerpt_text, excerpt_fetched_at,
                                   created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
        params![
            input.name.trim(),
            input.url.trim(),
            input.kind,
            input.related_symbol.trim(),
            input.thesis_id,
            input.publisher.trim(),
            input.published_at,
            input.excerpt_text.trim(),
            fetched,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_source(conn: &Connection, id: i64, input: &SourceInput) -> Result<(), SourceError> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM info_sources WHERE id=?1 AND archived=0",
            params![id],
            |r| r.get::<_, i64>(0).map(|_| true),
        )
        .optional()?
        .unwrap_or(false);
    if !exists {
        return Err(SourceError::NotFound(format!("信息源 {id}")));
    }
    validate_input(conn, input)?;
    let now = now_secs();
    // 摘录正文变化时刷新取得时间；仅改元数据不伪造成"新取得"。
    let excerpt_changed: i64 = conn.query_row(
        "SELECT excerpt_text != ?2 FROM info_sources WHERE id=?1",
        params![id, input.excerpt_text.trim()],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE info_sources SET name=?2, url=?3, kind=?4, related_symbol=?5, thesis_id=?6,
             publisher=?7, published_at=?8, excerpt_text=?9,
             excerpt_fetched_at=CASE WHEN ?10=1 THEN ?11 ELSE excerpt_fetched_at END,
             updated_at=?11
         WHERE id=?1",
        params![
            id,
            input.name.trim(),
            input.url.trim(),
            input.kind,
            input.related_symbol.trim(),
            input.thesis_id,
            input.publisher.trim(),
            input.published_at,
            input.excerpt_text.trim(),
            excerpt_changed,
            now
        ],
    )?;
    Ok(())
}

pub fn list_sources(
    conn: &Connection,
    include_archived: bool,
) -> Result<Vec<InfoSourceRow>, SourceError> {
    let sql = if include_archived {
        format!("SELECT {SOURCE_COLS} FROM info_sources ORDER BY favorite DESC, id DESC LIMIT 200")
    } else {
        format!(
            "SELECT {SOURCE_COLS} FROM info_sources WHERE archived=0
             ORDER BY favorite DESC, id DESC LIMIT 200"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_source)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn archive_source(conn: &Connection, id: i64) -> Result<(), SourceError> {
    let affected = conn.execute(
        "UPDATE info_sources SET archived=1, enabled=0, updated_at=?2 WHERE id=?1 AND archived=0",
        params![id, now_secs()],
    )?;
    if affected == 0 {
        return Err(SourceError::NotFound(format!("活跃信息源 {id}")));
    }
    Ok(())
}

pub fn set_source_flags(
    conn: &Connection,
    id: i64,
    favorite: Option<bool>,
    read_status: Option<bool>,
) -> Result<(), SourceError> {
    let now = now_secs();
    let mut changed = 0;
    if let Some(v) = favorite {
        changed += conn.execute(
            "UPDATE info_sources SET favorite=?2, updated_at=?3 WHERE id=?1 AND archived=0",
            params![id, v as i64, now],
        )?;
    }
    if let Some(v) = read_status {
        changed += conn.execute(
            "UPDATE info_sources SET read_status=?2, updated_at=?3 WHERE id=?1 AND archived=0",
            params![id, v as i64, now],
        )?;
    }
    if changed == 0 {
        return Err(SourceError::NotFound(format!("活跃信息源 {id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 投资论点（信息源可挂靠；持有理由/期限/失效条件，03 §6）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThesisRow {
    pub id: i64,
    pub name: String,
    pub holding_reason: String,
    pub horizon: String,
    pub verify_metrics: String,
    pub invalid_condition: String,
    pub related_symbols: String,
    pub version: i64,
    pub archived: bool,
}

fn row_to_thesis(r: &rusqlite::Row) -> rusqlite::Result<ThesisRow> {
    Ok(ThesisRow {
        id: r.get(0)?,
        name: r.get(1)?,
        holding_reason: r.get(2)?,
        horizon: r.get(3)?,
        verify_metrics: r.get(4)?,
        invalid_condition: r.get(5)?,
        related_symbols: r.get(6)?,
        version: r.get(7)?,
        archived: r.get(8)?,
    })
}

pub fn list_theses(conn: &Connection) -> Result<Vec<ThesisRow>, SourceError> {
    list_theses_with_history(conn, false)
}
pub fn list_theses_with_history(
    conn: &Connection,
    include_archived: bool,
) -> Result<Vec<ThesisRow>, SourceError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, holding_reason, horizon, verify_metrics, invalid_condition,
                related_symbols, version, archived
         FROM investment_theses WHERE archived=0 OR ?1 ORDER BY archived,id DESC LIMIT 200",
    )?;
    let rows = stmt
        .query_map([include_archived], row_to_thesis)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn require_thesis_version(
    conn: &Connection,
    id: i64,
    expected: i64,
) -> Result<(), SourceError> {
    let actual: Option<i64> = conn
        .query_row(
            "SELECT version FROM investment_theses WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    match actual {
        Some(v) if v == expected => Ok(()),
        Some(_) => Err(SourceError::Conflict),
        None => Err(SourceError::NotFound("策略".into())),
    }
}

pub struct ThesisInput {
    pub name: String,
    pub holding_reason: String,
    pub horizon: String,
    pub verify_metrics: String,
    pub invalid_condition: String,
    pub related_symbols: Vec<String>,
}

fn validate_thesis(input: &ThesisInput) -> Result<(), SourceError> {
    if input.name.trim().is_empty()
        || input.name.chars().count() > 128
        || [
            &input.holding_reason,
            &input.horizon,
            &input.verify_metrics,
            &input.invalid_condition,
        ]
        .iter()
        .any(|s| s.chars().count() > 4096)
        || input.related_symbols.len() > 30
        || input.related_symbols.iter().any(|s| s.len() > 128)
    {
        return Err(SourceError::Invalid("持有理由内容无效或过长".into()));
    }
    Ok(())
}

pub fn create_thesis(conn: &Connection, input: &ThesisInput) -> Result<i64, SourceError> {
    validate_thesis(input)?;
    let name = input.name.trim();
    let symbols = serde_json::to_string(&input.related_symbols)
        .map_err(|e| SourceError::Db(e.to_string()))?;
    let now = now_secs();
    conn.execute(
        "INSERT INTO investment_theses(name, holding_reason, horizon, verify_metrics,
                                       invalid_condition, related_symbols, created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?7)",
        params![
            name,
            input.holding_reason.trim(),
            input.horizon.trim(),
            input.verify_metrics.trim(),
            input.invalid_condition.trim(),
            symbols,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_thesis(conn: &Connection, id: i64, input: &ThesisInput) -> Result<(), SourceError> {
    validate_thesis(input)?;
    // 编辑递增 version（方案 05 §2）；已有报告快照保留其引用时的策略内容。
    let affected=conn.execute("UPDATE investment_theses SET name=?2,holding_reason=?3,horizon=?4,verify_metrics=?5,invalid_condition=?6,related_symbols=?7,version=version+1,updated_at=?8 WHERE id=?1 AND archived=0",params![id,input.name.trim(),input.holding_reason,input.horizon,input.verify_metrics,input.invalid_condition,serde_json::to_string(&input.related_symbols).map_err(|e|SourceError::Db(e.to_string()))?,now_secs()])?;
    if affected == 0 {
        return Err(SourceError::NotFound("持有理由".into()));
    }
    Ok(())
}
pub fn archive_thesis(conn: &Connection, id: i64) -> Result<(), SourceError> {
    let affected = conn.execute(
        "UPDATE investment_theses SET archived=1,version=version+1,updated_at=?2 WHERE id=?1 AND archived=0",
        params![id, now_secs()],
    )?;
    if affected == 0 {
        return Err(SourceError::NotFound("持有理由".into()));
    }
    Ok(())
}

/// 恢复归档策略（方案 05 §2：归档/删除及恢复）。恢复递增 version，防止旧窗口覆盖恢复后的内容。
pub fn restore_thesis(conn: &Connection, id: i64) -> Result<(), SourceError> {
    let affected = conn.execute(
        "UPDATE investment_theses SET archived=0,version=version+1,updated_at=?2 WHERE id=?1 AND archived=1",
        params![id, now_secs()],
    )?;
    if affected == 0 {
        return Err(SourceError::NotFound("归档持有理由".into()));
    }
    Ok(())
}

#[cfg(test)]
#[path = "sources_tests.rs"]
mod tests;
