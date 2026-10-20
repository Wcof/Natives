//! User usefulness feedback is separate from AI evidence and market outcomes.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub(crate) fn save(store: &Store, input: &Value) -> Result<Value, FinanceError> {
    crate::opportunity_records::validate(
        input,
        &["id", "expectedVersion", "useful", "reason", "requestId"],
    )?;
    let id = input["id"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| FinanceError::Invalid("报告身份无效".into()))?;
    let expected = input["expectedVersion"]
        .as_i64()
        .filter(|v| *v >= 0)
        .ok_or_else(|| FinanceError::Invalid("反馈版本无效".into()))?;
    let useful = input["useful"]
        .as_bool()
        .ok_or_else(|| FinanceError::Invalid("反馈必须为布尔值".into()))?;
    let reason = input["reason"]
        .as_str()
        .filter(|s| s.chars().count() <= 200)
        .ok_or_else(|| FinanceError::Invalid("反馈原因最多 200 字符".into()))?;
    crate::config_receipts::write(store, "report_feedback", input, |conn| {
        let active: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM analysis_reports WHERE id=?1 AND deleted_at IS NULL)",
            [id],
            |r| r.get(0),
        )?;
        if !active {
            return Err(FinanceError::Invalid("报告已归档或不存在".into()));
        }
        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM analysis_report_feedback WHERE report_id=?1",
            [id],
            |r| r.get(0),
        )?;
        if version != expected {
            return Err(FinanceError::Conflict("反馈已变化，请刷新".into()));
        }
        let next = version
            .checked_add(1)
            .ok_or_else(|| FinanceError::Overflow("反馈版本".into()))?;
        conn.execute("INSERT INTO analysis_report_feedback(report_id,version,useful,reason,created_at) VALUES(?1,?2,?3,?4,?5)", params![id,next,useful,reason,crate::finance::now_secs()])?;
        Ok(json!({"ok":true,"id":id,"version":next,"useful":useful,"reason":reason}))
    })
}

pub(crate) fn latest(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Value> {
    Ok(conn.query_row("SELECT version,useful,reason,created_at FROM analysis_report_feedback WHERE report_id=?1 ORDER BY version DESC LIMIT 1",[id], |r| Ok(json!({"version":r.get::<_,i64>(0)?,"useful":r.get::<_,bool>(1)?,"reason":r.get::<_,String>(2)?,"createdAt":r.get::<_,String>(3)?}))).optional()?.unwrap_or(Value::Null))
}

pub(crate) fn history(store: &Store, id: i64, page: i64) -> Result<Value, FinanceError> {
    if id <= 0 || !(1..=1_000_000).contains(&page) {
        return Err(FinanceError::Invalid("报告身份或页码无效".into()));
    }
    store.with_read(|conn| {
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM analysis_reports WHERE id=?1)",[id],|r|r.get(0))?;
        if !exists { return Err(FinanceError::Invalid("报告不存在".into())); }
        let total:i64=conn.query_row("SELECT COUNT(*) FROM analysis_report_feedback WHERE report_id=?1",[id],|r|r.get(0))?;
        let mut stmt=conn.prepare("SELECT version,useful,reason,created_at FROM analysis_report_feedback WHERE report_id=?1 ORDER BY version DESC LIMIT 20 OFFSET ?2")?;
        let rows=stmt.query_map(params![id,(page-1)*20],|r|Ok(json!({"version":r.get::<_,i64>(0)?,"useful":r.get::<_,bool>(1)?,"reason":r.get::<_,String>(2)?,"createdAt":r.get::<_,String>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(json!({"id":id,"page":page,"pageSize":20,"total":total,"items":rows}))
    })
}

pub(crate) fn stats(store: &Store) -> Result<Value, FinanceError> {
    store.with_read(|conn| {
        let mut rows=Vec::new();
        let mut totals=(0_i64,0_i64,0_i64);
        for task in ["diagnosis","conditions","news_impact","briefing"] {
            let (reports,feedback,useful):(i64,i64,i64)=conn.query_row("SELECT COUNT(*),COUNT(f.version),COALESCE(SUM(f.useful),0) FROM analysis_reports r LEFT JOIN analysis_report_feedback f ON f.report_id=r.id AND f.version=(SELECT MAX(version) FROM analysis_report_feedback WHERE report_id=r.id) WHERE r.deleted_at IS NULL AND r.task=?1",[task],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            totals.0+=reports;totals.1+=feedback;totals.2+=useful;
            rows.push(summary(task,reports,feedback,useful));
        }
        Ok(json!({"total":summary("all",totals.0,totals.1,totals.2),"byTask":rows,"metric":"user_usefulness_not_return"}))
    })
}
fn summary(task: &str, reports: i64, feedback: i64, useful: i64) -> Value {
    json!({"task":task,"reports":reports,"feedback":feedback,"unrated":reports-feedback,"useful":useful,"usefulRatePpm":if feedback==0{None}else{Some(useful*1_000_000/feedback)}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v24_upgrade_preserves_reports_and_matches_new_schema() {
        let dir = tempfile::tempdir().unwrap();
        let conn = rusqlite::Connection::open(dir.path().join("fund.db")).unwrap();
        for step in crate::migration::all_migrations()
            .iter()
            .filter(|m| m.version <= 24)
        {
            conn.execute_batch(step.sql).unwrap();
            conn.pragma_update(None, "user_version", step.version)
                .unwrap();
        }
        conn.execute("INSERT INTO analysis_reports(task,scope_json,evidence_json,snapshot_id,data_revision,result_text,generated_at,saved_at) VALUES('diagnosis','{}','{}','s','1','preserved','1','1')",[]).unwrap();
        for _ in 0..2 {
            assert_eq!(
                crate::migration::run_migrations(dir.path(), &conn, "feedback-test").unwrap(),
                crate::storage::SCHEMA_VERSION
            );
        }
        assert_eq!(
            conn.query_row("SELECT result_text FROM analysis_reports", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
        let ddl = |c: &rusqlite::Connection| {
            c.query_row(
                "SELECT sql FROM sqlite_master WHERE name='analysis_report_feedback'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        let fresh = Store::open_in_memory().unwrap();
        assert_eq!(
            ddl(&conn),
            fresh
                .with_read(|c| Ok::<_, rusqlite::Error>(ddl(c)))
                .unwrap()
        );
    }
    #[test]
    fn latest_feedback_only_archive_replay_conflict_and_empty_rate() {
        let store = Store::open_in_memory().unwrap();
        store.with_write(|c|c.execute("INSERT INTO analysis_reports(task,scope_json,evidence_json,snapshot_id,data_revision,result_text,generated_at,saved_at) VALUES('diagnosis','{}','{}','s','1','text','1','1')",[])).unwrap();
        assert!(stats(&store).unwrap()["total"]["usefulRatePpm"].is_null());
        let mut input = json!({"id":1,"expectedVersion":0,"useful":true,"reason":"first","requestId":"feedback-first"});
        let saved = save(&store, &input).unwrap();
        assert_eq!(saved, save(&store, &input).unwrap());
        input["requestId"] = json!("feedback-second");
        assert!(matches!(
            save(&store, &input),
            Err(FinanceError::Conflict(_))
        ));
        input["expectedVersion"] = json!(1);
        input["useful"] = json!(false);
        input["reason"] = json!("corrected");
        save(&store, &input).unwrap();
        let s = stats(&store).unwrap();
        assert_eq!(s["total"]["feedback"], 1);
        assert_eq!(s["total"]["useful"], 0);
        assert_eq!(history(&store, 1, 1).unwrap()["total"], 2);
        input["requestId"] = json!("feedback-invalid");
        input["expectedVersion"] = json!(2);
        input["useful"] = json!(1);
        assert!(save(&store, &input).is_err());
        input["useful"] = json!(true);
        input["reason"] = json!("x".repeat(201));
        assert!(save(&store, &input).is_err());
        store
            .with_write(|c| c.execute("UPDATE analysis_reports SET deleted_at='2' WHERE id=1", []))
            .unwrap();
        input["reason"] = json!("");
        assert!(save(&store, &input).is_err());
        assert_eq!(stats(&store).unwrap()["total"]["reports"], 0);
        assert_eq!(history(&store, 1, 1).unwrap()["total"], 2);
        assert!(history(&store, 1, 0).is_err());
    }
}
