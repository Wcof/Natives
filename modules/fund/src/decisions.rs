//! 决策记录（F09）：rules 拆分（架构规模门禁）。
//! 观察/暂缓/已处理记录；记录不改变事件状态（记录 ≠ 成交）。

use crate::rules::{now_secs, RuleError};
use rusqlite::{params, Connection, OptionalExtension};

// ---------------------------------------------------------------------------
// 决策记录（F09）
// ---------------------------------------------------------------------------

pub struct DecisionInput {
    pub event_id: i64,
    pub action: String,
    pub note: String,
    pub snapshot_id: String,
}

pub fn record_decision(conn: &Connection, input: &DecisionInput) -> Result<i64, RuleError> {
    if input.note.chars().count() > 4096 || input.snapshot_id.len() > 256 {
        return Err(RuleError::Invalid("记录内容过长".into()));
    }
    if !matches!(input.action.as_str(), "observe" | "defer" | "done") {
        return Err(RuleError::Invalid(format!("未知决策类型 {}", input.action)));
    }
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM watch_events WHERE id=?1",
            params![input.event_id],
            |r| r.get::<_, i64>(0).map(|_| true),
        )
        .optional()?
        .unwrap_or(false);
    if !exists {
        return Err(RuleError::NotFound(format!("事件 {}", input.event_id)));
    }
    let now = now_secs();
    conn.execute(
        "INSERT INTO decision_records(event_id, action, note, evidence_json, snapshot_id, created_at)
         VALUES(?1,?2,?3,'[]',?4,?5)",
        params![input.event_id, input.action, input.note, input.snapshot_id, now],
    )?;
    conn.execute("UPDATE decision_records SET evidence_json=(SELECT json_object('ruleId',rule_id,'ruleVersion',rule_version,'ruleType',rule_type,'objectKey',object_key,'triggerValue',trigger_value,'thresholdValue',threshold_value,'readyState',ready_state,'snapshotId',snapshot_id) FROM watch_events WHERE id=?2) WHERE id=?1",params![conn.last_insert_rowid(),input.event_id])?;
    // 记录"已处理"不等于真实成交：事件状态由规则解除逻辑管理（03/F09）。
    Ok(conn.last_insert_rowid())
}
