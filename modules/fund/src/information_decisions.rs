//! Decisions on information preserve the saved report as their evidence authority.
use crate::{rules::RuleError, storage::Store};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Input {
    pub information_id: i64,
    pub report_id: i64,
    pub action: String,
    pub note: String,
    pub request_id: String,
}

pub(crate) fn save(store: &Store, input: &Input) -> Result<i64, RuleError> {
    if input.information_id <= 0
        || input.report_id <= 0
        || !matches!(input.action.as_str(), "observe" | "defer" | "done")
        || input.note.trim().is_empty()
        || input.note.chars().count() > 4096
        || !(8..=128).contains(&input.request_id.len())
        || !input
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(RuleError::Invalid("资讯决定内容或身份无效".into()));
    }
    store.with_write(|c| {
        let previous:Option<(i64,i64,i64,String,String)>=c.query_row("SELECT id,information_id,report_id,action,note FROM information_decisions WHERE request_id=?1",[&input.request_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        if let Some((id,item,report,action,note))=previous {
            return if item==input.information_id && report==input.report_id && action==input.action && note==input.note.trim() {Ok(id)} else {Err(RuleError::Conflict("决定请求已用于其他内容".into()))};
        }
        let version:Option<i64>=c.query_row("SELECT content_version FROM information_items WHERE id=?1 AND archived=0",[input.information_id],|r|r.get(0)).optional()?;
        let Some(version)=version else {return Err(RuleError::NotFound("资讯已归档或不存在".into()));};
        let report:Option<(String,String)>=c.query_row("SELECT task,evidence_json FROM analysis_reports WHERE id=?1 AND deleted_at IS NULL",[input.report_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((task,body))=report else {return Err(RuleError::NotFound("分析报告不存在".into()));};
        let packet:Value=serde_json::from_str(&body).map_err(|_|RuleError::Invalid("报告证据损坏".into()))?;
        let expected=format!("I{}",input.information_id);
        let piece=packet["evidence"].as_array().and_then(|rows|rows.iter().find(|r|r["id"]==expected && r["kind"]=="information"));
        if task!="news_impact" || piece.is_none() {return Err(RuleError::Invalid("报告没有分析该资讯".into()));}
        let captured=piece.and_then(|p|p["contentVersion"].as_i64().or_else(||p["contentVersion"].as_str().and_then(|v|v.parse().ok())));
        if captured!=Some(version) {return Err(RuleError::Conflict("资讯已更新，请重新分析".into()));}
        c.execute("INSERT INTO information_decisions(information_id,report_id,action,note,request_id,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![input.information_id,input.report_id,input.action,input.note.trim(),input.request_id,crate::finance::now_secs()])?;
        Ok(c.last_insert_rowid())
    })
}

pub(crate) fn list(store: &Store) -> Result<Value, RuleError> {
    store.with_read(|c| {
        let mut stmt=c.prepare("SELECT d.id,d.information_id,d.report_id,d.action,d.note,d.created_at,i.title,r.snapshot_id,r.data_revision,r.model_name FROM information_decisions d JOIN information_items i ON i.id=d.information_id JOIN analysis_reports r ON r.id=d.report_id ORDER BY d.id DESC LIMIT 100")?;
        let rows=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"informationId":r.get::<_,i64>(1)?,"reportId":r.get::<_,i64>(2)?,"action":r.get::<_,String>(3)?,"note":r.get::<_,String>(4)?,"createdAt":r.get::<_,String>(5)?,"title":r.get::<_,String>(6)?,"snapshotId":r.get::<_,String>(7)?,"dataRevision":r.get::<_,String>(8)?,"modelName":r.get::<_,String>(9)?,"sourceKind":"information"})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(json!({"records":rows}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, i64) {
        let store = Store::open_in_memory().unwrap();
        store.with_write(|c|c.execute("INSERT INTO information_items(source_id,external_id,title,retrieved_at,content_hash,created_at,updated_at) VALUES('fixture','a','Synthetic','0','hash','0','0')",[])).unwrap();
        let revision = store.with_read(Store::data_revision).unwrap();
        let evidence = json!({"task":"news_impact","scope":"portfolio","snapshotId":"fixture","dataRevision":revision,"evidence":[{"id":"I1","kind":"information","contentVersion":1}]});
        let payload = json!({"task":"news_impact","snapshotId":"fixture","dataRevision":revision,"evidence":evidence,"resultText":"Synthetic analysis"});
        let saved = crate::api::reports_api::api_analysis_report_save(&store, &payload.to_string())
            .unwrap();
        (
            store,
            serde_json::from_str::<Value>(&saved).unwrap()["id"]
                .as_i64()
                .unwrap(),
        )
    }
    #[test]
    fn decisions_bind_real_evidence_and_retry_without_creating_trades() {
        let (store, report) = fixture();
        let mut input = Input {
            information_id: 1,
            report_id: report,
            action: "observe".into(),
            note: "Wait for evidence".into(),
            request_id: "fixture-request-1".into(),
        };
        let id = save(&store, &input).unwrap();
        assert_eq!(save(&store, &input).unwrap(), id);
        assert_eq!(
            list(&store).unwrap()["records"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            store
                .with_read(
                    |c| c.query_row("SELECT COUNT(*) FROM transactions", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            0
        );
        input.note = "Different".into();
        assert!(matches!(save(&store, &input), Err(RuleError::Conflict(_))));
        input.request_id = "fixture-request-2".into();
        input.action = "中文动作".into();
        assert!(save(&store, &input).is_err());
        input.action = "defer".into();
        input.information_id = 2;
        assert!(save(&store, &input).is_err());
        input.information_id = 1;
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE information_items SET content_version=2 WHERE id=1",
                    [],
                )
            })
            .unwrap();
        assert!(matches!(save(&store, &input), Err(RuleError::Conflict(_))));
    }

    #[test]
    fn report_lookup_keeps_captured_evidence_and_handles_missing_archived_and_bad_ids() {
        let (store, id) = fixture();
        let get = |query: &str| -> Value {
            serde_json::from_str(
                &crate::api::reports_api::api_analysis_reports(&store, query).unwrap(),
            )
            .unwrap()
        };
        let selected = get(&format!("/api/analysis/reports?id={id}&page=999"));
        assert_eq!(selected["reports"].as_array().unwrap().len(), 1);
        assert_eq!(
            selected["reports"][0]["evidence"]["evidence"][0]["id"],
            "I1"
        );
        assert!(get("/api/analysis/reports?page=1")["reports"][0]
            .get("evidence")
            .is_none());
        assert!(get("/api/analysis/reports?id=999")["reports"]
            .as_array()
            .unwrap()
            .is_empty());
        for query in ["id=0", "id=-1", "id=bad"] {
            assert_eq!(
                crate::api::reports_api::api_analysis_reports(
                    &store,
                    &format!("/api/analysis/reports?{query}")
                )
                .unwrap_err()
                .0,
                400
            );
        }
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE analysis_reports SET deleted_at='1' WHERE id=?1",
                    [id],
                )
            })
            .unwrap();
        assert!(get(&format!("/api/analysis/reports?id={id}"))["reports"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            get(&format!("/api/analysis/reports?id={id}&includeDeleted=1"))["reports"][0]
                ["deleted"],
            true
        );
    }
}
