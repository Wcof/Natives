//! Minimal, revision-bound disclosure for a user-approved business notification.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) fn preview(store: &Store, input: &Value) -> Result<Value, FinanceError> {
    let channel_id = input["id"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| FinanceError::Invalid("渠道身份无效".into()))?;
    let subject = input["subject"].as_str().unwrap_or("");
    let subject_id = input["subjectId"].as_i64().filter(|v| *v > 0);
    if (subject != "candidate" && subject_id.is_none())
        || (subject != "candidate" && input.get("snapshotId").is_some())
    {
        return Err(FinanceError::Invalid("通知对象无效".into()));
    }
    let language = input["language"].as_str().unwrap_or("zh_CN");
    if !["zh_CN", "en"].contains(&language) {
        return Err(FinanceError::Invalid("通知语言无效".into()));
    }
    let english = language == "en";
    store.with_read(|conn| {
        let channel: Option<(String, String, i64, String)> = conn.query_row(
            "SELECT name,kind,version,config_json FROM notify_channels WHERE id=?1 AND enabled=1 AND archived=0",
            [channel_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (name, kind, version, config) = channel.ok_or_else(|| FinanceError::Invalid("渠道不可用".into()))?;
        let config: Value = serde_json::from_str(&config).map_err(|_|FinanceError::Db("渠道配置损坏".into()))?;
        // Free text and monetary thresholds never enter the message.
        let (body, snapshot, reference) = match subject {
            "event" => {
                let subject_id=subject_id.unwrap();
                let row: Option<(i64,String,String,String)> = conn.query_row(
                    "SELECT rule_version,snapshot_id,status,updated_at FROM watch_events WHERE id=?1 AND status!='resolved'",
                    [subject_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
                let (v,snapshot,status,updated) = row.ok_or_else(|| FinanceError::Invalid("事件已解除或不存在".into()))?;
                (if english {format!("Investment rule event #{subject_id}, version {v}. Open Natives to review the evidence. No trade has been placed.")}else{format!("投资规则事件 #{subject_id}，版本 {v}。请返回 Natives 查看依据，本消息不代表交易指令。")},snapshot,format!("event:{subject_id}:{v}:{status}:{updated}"))
            }
            "report" => {
                let subject_id=subject_id.unwrap();
                let row: Option<(String,i64,String,String)> = conn.query_row(
                    "SELECT task,version,snapshot_id,saved_at FROM analysis_reports WHERE id=?1 AND deleted_at IS NULL",
                    [subject_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
                let (task,v,snapshot,saved) = row.ok_or_else(|| FinanceError::Invalid("报告已归档或不存在".into()))?;
                if !["diagnosis","conditions","news_impact","briefing"].contains(&task.as_str()) {
                    return Err(FinanceError::Invalid("报告类别无效".into()));
                }
                (if english{format!("Investment analysis report #{subject_id}, {task}, version {v}, saved {saved}. Open Natives to review the evidence. AI analysis is not an order.")}else{format!("投资分析报告 #{subject_id}，版本 {v}。请返回 Natives 查看依据，AI 分析不代表交易指令。")},snapshot,format!("report:{subject_id}:{v}"))
            }
            "candidate" => {
                let sid=input["snapshotId"].as_str().filter(|s|!s.is_empty()&&s.len()<=256).ok_or_else(||FinanceError::Invalid("缺少候选快照".into()))?;
                let cid=input["subjectId"].as_str().filter(|s|!s.is_empty()&&s.len()<=256).ok_or_else(||FinanceError::Invalid("候选身份无效".into()))?;
                let text:Option<String>=conn.query_row("SELECT projection_json FROM opportunity_snapshots WHERE snapshot_id=?1",[sid],|r|r.get(0)).optional()?;
                let saved:Value=serde_json::from_str(&text.ok_or_else(||FinanceError::Invalid("候选快照不存在".into()))?).map_err(|_|FinanceError::Db("候选快照损坏".into()))?;
                let until=saved["validUntil"].as_str().and_then(|s|s.parse::<u64>().ok()).ok_or_else(||FinanceError::Invalid("候选有效期缺失".into()))?;
                let now=crate::finance::now_secs().parse::<u64>().map_err(|_|FinanceError::Db("时间不可用".into()))?;
                if now>=until {return Err(FinanceError::Conflict("候选快照已过期，请重新生成".into()));}
                if !saved["items"].as_array().is_some_and(|rows|rows.iter().any(|r|r["candidateId"]==cid)){return Err(FinanceError::Invalid("候选不属于该快照".into()));}
                (if english {"An investment candidate is available for review in Natives. This is an observation, not an order.".to_string()}else{"Natives 有一条投资候选可供复核。请返回应用查看条件和失效依据，本消息不代表交易指令。".to_string()},sid.to_string(),format!("candidate:{sid}:{cid}"))
            }
            _ => return Err(FinanceError::Invalid("通知对象类型无效".into())),
        };
        // 用户配置的钉钉关键词与 @ 手机号属于出站内容，必须进入预览及授权 token。
        let (body, recipients) = if kind == "dingtalk" {
            let keyword = config["keyword"].as_str().unwrap_or("");
            let body = if keyword.is_empty() || body.contains(keyword) { body } else { format!("{body} {keyword}") };
            let recipients: Vec<&str> = config["phones"].as_str().unwrap_or("").split(',').map(str::trim).filter(|s|!s.is_empty()).collect();
            (body, recipients)
        } else { (body, Vec::new()) };
        let mut result = json!({"id":channel_id,"subject":subject,"subjectId":input["subjectId"],"channelName":name,"channelKind":kind,"channelVersion":version,"title":if english {"Natives investment review"}else{"Natives 投资复核"},"body":body,"recipients":recipients,"snapshotId":snapshot,"dataRevision":Store::data_revision(conn)?,"subjectRef":reference,"discloseFinance":false});
        let token = format!("{:x}",Sha256::digest(result.to_string().as_bytes()));
        result["previewToken"] = json!(token);
        Ok(result)
    })
}

pub(crate) fn validate_send(store: &Store, input: &Value) -> Result<Value, FinanceError> {
    if !input.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            [
                "id",
                "subject",
                "subjectId",
                "snapshotId",
                "language",
                "previewToken",
                "requestId",
            ]
            .contains(&k.as_str())
        })
    }) {
        return Err(FinanceError::Invalid("业务通知不接受自定义正文".into()));
    }
    let result = preview(store, input)?;
    if input["previewToken"].as_str().is_none() || input["previewToken"] != result["previewToken"] {
        return Err(FinanceError::Conflict(
            "通知依据或渠道已变化，请重新预览".into(),
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dingtalk_preview_discloses_keyword_mentions_and_matches_wire_text() {
        let store = Store::open_in_memory().unwrap();
        let channel = crate::channels::save(&store, &json!({"kind":"dingtalk","name":"Test","config":{"keyword":"review-tag","phones":"13800138000, 13900139000"}})).unwrap();
        store.with_write(|c|c.execute("INSERT INTO analysis_reports(task,scope_json,evidence_json,snapshot_id,data_revision,result_text,generated_at,saved_at) VALUES('diagnosis','{}','{}','s','1','PRIVATE','1','1')",[])).unwrap();
        let p = preview(
            &store,
            &json!({"id":channel["id"],"subject":"report","subjectId":1}),
        )
        .unwrap();
        assert!(p["body"].as_str().unwrap().contains("review-tag"));
        assert_eq!(p["recipients"], json!(["13800138000", "13900139000"]));
        let request = crate::channel_protocols::build_request(
            "dingtalk",
            &json!({"keyword":"review-tag","phones":"13800138000, 13900139000"}),
            &json!({"token":"fixture"}),
            p["title"].as_str().unwrap(),
            p["body"].as_str().unwrap(),
        )
        .unwrap();
        let wire: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(
            wire["text"]["content"],
            format!(
                "{}\n{}",
                p["title"].as_str().unwrap(),
                p["body"].as_str().unwrap()
            )
        );
        assert_eq!(wire["at"]["atMobiles"], p["recipients"]);
        assert!(!p.to_string().contains("PRIVATE"));
    }
    #[test]
    fn disclosure_excludes_personal_data_and_rejects_changed_evidence() {
        let store = Store::open_in_memory().unwrap();
        let channel = crate::channels::save(
            &store,
            &json!({"kind":"wecom","name":"Test destination","config":{}}),
        )
        .unwrap();
        store.with_write(|c| {
            c.execute("INSERT INTO analysis_reports(task,scope_json,evidence_json,snapshot_id,data_revision,result_text,generated_at,saved_at) VALUES('diagnosis','{}','{}','snapshot','1','PRIVATE 99999 account','1','1')",[])?;
            Ok::<_,FinanceError>(())
        }).unwrap();
        let input = json!({"id":channel["id"],"subject":"report","subjectId":1});
        let result = preview(&store, &input).unwrap();
        assert!(!result.to_string().contains("PRIVATE"));
        assert!(!result.to_string().contains("99999"));
        let mut send = input.clone();
        send["previewToken"] = result["previewToken"].clone();
        send["requestId"] = json!("send-once");
        validate_send(&store, &send).unwrap();
        let mut injected = send.clone();
        injected["body"] = json!("unapproved");
        assert!(validate_send(&store, &injected).is_err());
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE analysis_reports SET version=version+1 WHERE id=1",
                    [],
                )?;
                Ok::<_, FinanceError>(())
            })
            .unwrap();
        assert!(matches!(
            validate_send(&store, &send),
            Err(FinanceError::Conflict(_))
        ));
    }
    #[test]
    fn candidate_preview_requires_saved_membership_and_unexpired_snapshot() {
        let store = Store::open_in_memory().unwrap();
        let channel =
            crate::channels::save(&store, &json!({"kind":"wecom","name":"C","config":{}})).unwrap();
        store.with_write(|c|c.execute("INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES(1,'sh600519','PRIVATE_NAME','stock','1')",[])).unwrap();
        crate::opportunity_records::generate_home(&store, "all", None, None).unwrap();
        let history = crate::opportunity_records::snapshots(&store, 1).unwrap();
        let saved = &history["items"][0];
        let input = json!({"id":channel["id"],"subject":"candidate","subjectId":saved["items"][0]["candidateId"],"snapshotId":saved["snapshotId"]});
        let p = preview(&store, &input).unwrap();
        assert!(!p.to_string().contains("PRIVATE_NAME"));
        let mut forged = input.clone();
        forged["subjectId"] = json!("foreign-candidate");
        assert!(preview(&store, &forged).is_err());
        store.with_write(|c|c.execute("UPDATE opportunity_snapshots SET projection_json=json_set(projection_json,'$.validUntil','0')",[])).unwrap();
        assert!(matches!(
            preview(&store, &input),
            Err(FinanceError::Conflict(_))
        ));
    }
}
