//! Immutable opportunity captures and versioned feedback; no network or portfolio mutation.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, FinanceError> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= max)
        .ok_or_else(|| FinanceError::Invalid(format!("{key} 无效")))
}
pub(crate) fn validate(v: &Value, fields: &[&str]) -> Result<(), FinanceError> {
    if !v
        .as_object()
        .is_some_and(|o| o.len() == fields.len() && o.keys().all(|k| fields.contains(&k.as_str())))
    {
        return Err(FinanceError::Invalid("字段无效".into()));
    }
    if !text(v, "requestId", 128)?
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(FinanceError::Invalid("requestId 无效".into()));
    }
    Ok(())
}
pub(crate) fn decode(s: &str) -> Result<Value, FinanceError> {
    serde_json::from_str(s).map_err(|_| FinanceError::Db("机会记录无法读取".into()))
}
pub(crate) fn replay(conn: &Connection, v: &Value) -> Result<Option<Value>, FinanceError> {
    let old: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_json,result_json FROM opportunity_ops WHERE request_id=?1",
            [text(v, "requestId", 128)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match old {
        Some((payload, result)) if decode(&payload)? == *v => Ok(Some(decode(&result)?)),
        Some(_) => Err(FinanceError::Conflict("请求 ID 已用于其他操作".into())),
        None => Ok(None),
    }
}
pub(crate) fn remember(conn: &Connection, v: &Value, result: &Value) -> Result<(), FinanceError> {
    conn.execute(
        "INSERT INTO opportunity_ops(request_id,payload_json,result_json) VALUES(?1,?2,?3)",
        params![
            text(v, "requestId", 128)?,
            v.to_string(),
            result.to_string()
        ],
    )?;
    Ok(())
}

pub(crate) fn capture(store: &Store, v: &Value) -> Result<Value, FinanceError> {
    validate(v, &["requestId", "snapshotId", "dataRevision"])?;
    text(v, "snapshotId", 256)?;
    text(v, "dataRevision", 32)?;
    if let Some(result) = store.with_read(|c| replay(c, v))? {
        return Ok(result);
    }
    let projection = crate::home::build_snapshot(store, "all")?["opportunities"].clone();
    if projection["snapshotId"] != v["snapshotId"]
        || projection["dataRevision"] != v["dataRevision"]
    {
        return Err(FinanceError::Conflict(
            "候选已变化，请重新读取后保存".into(),
        ));
    }
    store.with_write(|conn| {
        if let Some(result) = replay(conn, v)? {
            return Ok(result);
        }
        if Store::data_revision(conn)? != text(v, "dataRevision", 32)? {
            return Err(FinanceError::Conflict("账本已变化".into()));
        }
        let (saved, _) = persist(conn, projection)?;
        remember(conn, v, &saved)?;
        Ok(saved)
    })
}

fn facts_key(projection: &Value) -> Result<String, FinanceError> {
    let today = crate::finance::today_cn();
    let mut facts = projection["items"].clone();
    for row in facts
        .as_array_mut()
        .ok_or_else(|| FinanceError::Db("候选结构无效".into()))?
    {
        for key in ["candidateId", "snapshotId", "dataRevision"] {
            row.as_object_mut().unwrap().remove(key);
        }
    }
    let key = format!(
        "{}:{}",
        today,
        app_runtime_core::sha256_hex(facts.to_string().as_bytes())
    );
    Ok(key)
}

pub(crate) fn attach_recorded(
    conn: &Connection,
    projection: &mut Value,
) -> Result<(), FinanceError> {
    let key = facts_key(projection)?;
    let saved: Option<String> = conn
        .query_row(
            "SELECT projection_json FROM opportunity_snapshots WHERE facts_key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(saved) = saved {
        let saved = decode(&saved)?;
        projection["recordedSnapshot"] = json!({"snapshotId":saved["snapshotId"],"dataRevision":saved["dataRevision"],"generatedAt":saved["generatedAt"],"validUntil":saved["validUntil"]});
    }
    Ok(())
}

fn persist(conn: &Connection, mut projection: Value) -> Result<(Value, bool), FinanceError> {
    let today = crate::finance::today_cn();
    let facts_key = facts_key(&projection)?;
    let now = crate::finance::now_secs();
    let until = now
        .parse::<u64>()
        .map_err(|_| FinanceError::Db("时间无效".into()))?
        .checked_add(86400)
        .ok_or_else(|| FinanceError::Overflow("时间".into()))?;
    projection["generatedAt"] = now.clone().into();
    projection["generatedDay"] = today.into();
    projection["validUntil"] = until.to_string().into();
    let existing: Option<String> = conn
        .query_row(
            "SELECT projection_json FROM opportunity_snapshots WHERE facts_key=?1",
            [&facts_key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(saved) = existing {
        return Ok((decode(&saved)?, false));
    }
    conn.execute("INSERT INTO opportunity_snapshots(snapshot_id,facts_key,data_revision,generated_at,projection_json) VALUES(?1,?2,?3,?4,?5)", params![text(&projection,"snapshotId",256)?,facts_key,text(&projection,"dataRevision",32)?,now,projection.to_string()])?;
    Ok((projection, true))
}

/// Explicit generation command: never used by a GET; all cohorts, including weak/missing data, are retained.
pub(crate) fn generate_home(
    store: &Store,
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Value, FinanceError> {
    for _ in 0..3 {
        let mut home = crate::home::cached_snapshot_range(store, range, from, to)?;
        let recorded = store.with_write(|conn| {
            if Store::data_revision(conn)? != text(&home, "dataRevision", 32)? {
                return Err(FinanceError::Conflict("账本已变化".into()));
            }
            persist(conn, home["opportunities"].clone())
        });
        let (saved, inserted) = match recorded {
            Err(FinanceError::Conflict(_)) => continue,
            result => result?,
        };
        if inserted {
            continue;
        }
        home["opportunities"]["recordedSnapshot"] = json!({"snapshotId":saved["snapshotId"],"dataRevision":saved["dataRevision"],"generatedAt":saved["generatedAt"],"validUntil":saved["validUntil"]});
        return Ok(home);
    }
    Err(FinanceError::Conflict(
        "生成期间数据持续变化，请重试".into(),
    ))
}

pub(crate) fn feedback(store: &Store, v: &Value) -> Result<Value, FinanceError> {
    validate(
        v,
        &[
            "requestId",
            "snapshotId",
            "candidateId",
            "dataRevision",
            "useful",
            "reason",
            "expectedVersion",
        ],
    )?;
    let sid = text(v, "snapshotId", 256)?;
    let cid = text(v, "candidateId", 320)?;
    let revision = text(v, "dataRevision", 32)?;
    let useful = v["useful"]
        .as_bool()
        .ok_or_else(|| FinanceError::Invalid("useful 必须为布尔值".into()))?;
    let reason = v["reason"]
        .as_str()
        .filter(|s| s.chars().count() <= 200)
        .ok_or_else(|| FinanceError::Invalid("理由须不超过 200 字".into()))?;
    let expected = v["expectedVersion"]
        .as_i64()
        .filter(|n| *n >= 0)
        .ok_or_else(|| FinanceError::Invalid("版本无效".into()))?;
    store.with_write(|conn| {
        if let Some(result)=replay(conn,v)? {return Ok(result)}
        let saved:Option<String>=conn.query_row("SELECT projection_json FROM opportunity_snapshots WHERE snapshot_id=?1",[sid],|r|r.get(0)).optional()?;
        let p=decode(&saved.ok_or_else(||FinanceError::NotFound("候选快照".into()))?)?;
        if p["dataRevision"]!=revision || !p["items"].as_array().is_some_and(|items|items.iter().any(|r|r["candidateId"]==cid)) {
            return Err(FinanceError::Invalid("候选不属于此快照".into()));
        }
        let version:i64=conn.query_row("SELECT COALESCE(MAX(version),0) FROM opportunity_feedback WHERE snapshot_id=?1 AND candidate_id=?2",params![sid,cid],|r|r.get(0))?;
        if version!=expected {return Err(FinanceError::Conflict("反馈已被修改，请重新读取".into()))}
        let next=version.checked_add(1).ok_or_else(||FinanceError::Overflow("版本".into()))?;
        conn.execute("INSERT INTO opportunity_feedback(snapshot_id,candidate_id,version,useful,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![sid,cid,next,useful,reason,crate::finance::now_secs()])?;
        let result=json!({"snapshotId":sid,"candidateId":cid,"version":next,"useful":useful,"reason":reason});
        remember(conn,v,&result)?;
        Ok(result)
    })
}

pub(crate) fn snapshots(store: &Store, page: i64) -> Result<Value, FinanceError> {
    if !(1..=1000000).contains(&page) {
        return Err(FinanceError::Invalid("分页无效".into()));
    }
    store.with_snapshot(|conn| {
        let total:i64=conn.query_row("SELECT COUNT(*) FROM opportunity_snapshots",[],|r|r.get(0))?;
        let mut stmt=conn.prepare("SELECT projection_json FROM opportunity_snapshots ORDER BY rowid DESC LIMIT 10 OFFSET ?1")?;
        let values=stmt.query_map([(page-1)*10],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        let mut items=Vec::new();
        for saved in values {
            let mut p=decode(&saved)?;
            let sid=text(&p,"snapshotId",256)?.to_string();
            for row in p["items"].as_array_mut().ok_or_else(||FinanceError::Db("候选结构无效".into()))? {
                row["feedback"]=conn.query_row("SELECT version,useful,reason FROM opportunity_feedback WHERE snapshot_id=?1 AND candidate_id=?2 ORDER BY version DESC LIMIT 1",params![sid,text(row,"candidateId",320)?],|r|Ok(json!({"version":r.get::<_,i64>(0)?,"useful":r.get::<_,bool>(1)?,"reason":r.get::<_,String>(2)?}))).optional()?.unwrap_or(json!({"version":0,"useful":null,"reason":""}));
            }
            crate::opportunity_observations::attach(conn, &sid, p["items"].as_array_mut().ok_or_else(||FinanceError::Db("候选结构无效".into()))?)?;
            items.push(p);
        }
        Ok(json!({"page":page,"pageSize":10,"total":total,"items":items}))
    })
}

pub(crate) fn feedback_history(
    store: &Store,
    sid: &str,
    cid: &str,
    page: i64,
) -> Result<Value, FinanceError> {
    text(&json!({"snapshotId":sid}), "snapshotId", 256)?;
    text(&json!({"candidateId":cid}), "candidateId", 320)?;
    if !(1..=1000000).contains(&page) {
        return Err(FinanceError::Invalid("分页无效".into()));
    }
    store.with_snapshot(|conn| {
        let saved:Option<String>=conn.query_row("SELECT projection_json FROM opportunity_snapshots WHERE snapshot_id=?1",[sid],|r|r.get(0)).optional()?;
        let p=decode(&saved.ok_or_else(||FinanceError::NotFound("候选快照".into()))?)?;
        if !p["items"].as_array().is_some_and(|items|items.iter().any(|r|r["candidateId"]==cid)) {
            return Err(FinanceError::NotFound("候选".into()));
        }
        let total:i64=conn.query_row("SELECT COUNT(*) FROM opportunity_feedback WHERE snapshot_id=?1 AND candidate_id=?2",params![sid,cid],|r|r.get(0))?;
        let mut stmt=conn.prepare("SELECT version,useful,reason,created_at FROM opportunity_feedback WHERE snapshot_id=?1 AND candidate_id=?2 ORDER BY version DESC LIMIT 20 OFFSET ?3")?;
        let items=stmt.query_map(params![sid,cid,(page-1)*20],|r|Ok(json!({"version":r.get::<_,i64>(0)?,"useful":r.get::<_,bool>(1)?,"reason":r.get::<_,String>(2)?,"createdAt":r.get::<_,String>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(json!({"snapshotId":sid,"candidateId":cid,"page":page,"pageSize":20,"total":total,"items":items}))
    })
}
pub(crate) fn stats(store: &Store) -> Result<Value, FinanceError> {
    store.with_snapshot(|conn| {
        let mut stmt=conn.prepare("SELECT f.useful,s.projection_json,f.candidate_id FROM opportunity_feedback f JOIN opportunity_snapshots s ON s.snapshot_id=f.snapshot_id WHERE NOT EXISTS(SELECT 1 FROM opportunity_feedback n WHERE n.snapshot_id=f.snapshot_id AND n.candidate_id=f.candidate_id AND n.version>f.version)")?;
        let mut total=0i64;let mut useful=0i64;
        let mut groups=std::collections::BTreeMap::<(String,String),(i64,i64)>::new();
        let mut rows=stmt.query([])?;
        while let Some(r)=rows.next()? {
            let yes=r.get::<_,bool>(0)?;let p=decode(&r.get::<_,String>(1)?)?;let cid=r.get::<_,String>(2)?;
            let row=p["items"].as_array().and_then(|items|items.iter().find(|i|i["candidateId"]==cid)).ok_or_else(||FinanceError::Db("反馈关联无效".into()))?;
            total+=1;useful+=i64::from(yes);
            for (kind,key) in [("source",row["pool"].as_str().unwrap_or("unknown")),("strategy",row["strategyVersion"].as_str().unwrap_or("unknown"))] {
                let g=groups.entry((kind.into(),key.into())).or_default();g.0+=1;g.1+=i64::from(yes);
            }
        }
        let summary=|n:i64,u:i64|json!({"total":n,"useful":u,"usefulRatePpm":if n>0 {Some(u*1000000/n)}else{None}});
        Ok(json!({"feedback":summary(total,useful),"groups":groups.into_iter().map(|((kind,key),(n,u))|json!({"kind":kind,"key":key,"feedback":summary(n,u)})).collect::<Vec<_>>(),"observations":crate::opportunity_observations::coverage(conn)?,"metric":"user-feedback-not-investment-return"}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_generation_retains_every_candidate_and_settles_metadata_revision() {
        let store = Store::open_in_memory().unwrap();
        store.with_write(|c|c.execute("INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES(1,'110022','Fund','fund',CURRENT_TIMESTAMP)",[])).unwrap();
        let before = store.with_read(Store::data_revision).unwrap();
        crate::home::build_snapshot(&store, "all").unwrap();
        assert_eq!(
            store.with_read(Store::data_revision).unwrap(),
            before,
            "GET projection stays read only"
        );
        let first = generate_home(&store, "all", None, None).unwrap();
        let after = store.with_read(Store::data_revision).unwrap();
        assert_eq!(first["dataRevision"], after);
        let cached = crate::home::cached_snapshot_range(&store, "all", None, None).unwrap();
        assert_eq!(
            cached["opportunities"]["recordedSnapshot"],
            first["opportunities"]["recordedSnapshot"]
        );
        assert_eq!(store.with_read(Store::data_revision).unwrap(), after);
        assert_ne!(before, after);
        assert_eq!(
            first["opportunities"]["items"][0]["quality"],
            "insufficient_data"
        );
        assert!(first["opportunities"]["recordedSnapshot"]["snapshotId"]
            .as_str()
            .is_some());
        assert_eq!(snapshots(&store, 1).unwrap()["total"], 1);
        for _ in 0..5 {
            assert_eq!(
                generate_home(&store, "month", None, None).unwrap()["dataRevision"],
                after
            );
        }
        assert_eq!(snapshots(&store, 1).unwrap()["total"], 1);
        assert_eq!(
            store
                .with_read(
                    |c| c.query_row("SELECT COUNT(*) FROM opportunity_ops", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            0,
            "automatic generation does not duplicate request result payloads"
        );
        store.with_write(|c|c.execute("INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES(1,'sh600519','Stock','stock',CURRENT_TIMESTAMP)",[])).unwrap();
        let changed = generate_home(&store, "all", None, None).unwrap();
        assert_eq!(
            changed["opportunities"]["items"].as_array().unwrap().len(),
            2
        );
        let history = snapshots(&store, 1).unwrap();
        assert_eq!(history["total"], 2);
        assert_eq!(history["items"][0]["items"].as_array().unwrap().len(), 2);
        assert_eq!(history["items"][1]["items"].as_array().unwrap().len(), 1);
        assert!(generate_home(&store, "custom", None, None).is_err());
    }
    #[test]
    fn v19_upgrade_is_idempotent_and_matches_new_database() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("fund.db")).unwrap();
        for step in crate::migration::all_migrations()
            .iter()
            .filter(|m| m.version <= 19)
        {
            conn.execute_batch(step.sql).unwrap();
            conn.pragma_update(None, "user_version", step.version)
                .unwrap();
        }
        conn.execute(
            "INSERT INTO accounts(name,created_at,updated_at) VALUES('preserved','1','1')",
            [],
        )
        .unwrap();
        assert_eq!(
            crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
            crate::storage::SCHEMA_VERSION
        );
        assert_eq!(
            crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
            crate::storage::SCHEMA_VERSION
        );
        let schema = |c: &Connection| {
            c.prepare(
                "SELECT name,sql FROM sqlite_master WHERE name LIKE 'opportunity_%' ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        let new = Store::open_in_memory().unwrap();
        assert_eq!(
            schema(&conn),
            new.with_read(|c| Ok::<_, rusqlite::Error>(schema(c)))
                .unwrap()
        );
        assert_eq!(
            conn.query_row("SELECT name FROM accounts", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
    }
    #[test]
    fn immutable_capture_feedback_replay_conflict_and_latest_stats() {
        let store = Store::open_in_memory().unwrap();
        store.with_write(|c|c.execute("INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES(1,'sh600519','<img src=x>','stock',CURRENT_TIMESTAMP)",[])).unwrap();
        let p = crate::home::build_snapshot(&store, "all").unwrap()["opportunities"].clone();
        let input = json!({"requestId":"capture-1","snapshotId":p["snapshotId"],"dataRevision":p["dataRevision"]});
        let saved = capture(&store, &input).unwrap();
        assert_eq!(capture(&store, &input).unwrap(), saved);
        assert!(matches!(
            capture(
                &store,
                &json!({"requestId":"capture-1","snapshotId":"changed","dataRevision":"0"})
            ),
            Err(FinanceError::Conflict(_))
        ));
        let next = crate::home::build_snapshot(&store, "all").unwrap()["opportunities"].clone();
        assert_eq!(capture(&store,&json!({"requestId":"capture-2","snapshotId":next["snapshotId"],"dataRevision":next["dataRevision"]})).unwrap(),saved,"metadata revisions do not duplicate samples");
        assert_eq!(snapshots(&store, 1).unwrap()["total"], 1);
        assert!(stats(&store).unwrap()["feedback"]["usefulRatePpm"].is_null());
        let mut v = json!({"requestId":"feedback-1","snapshotId":saved["snapshotId"],"candidateId":saved["items"][0]["candidateId"],"dataRevision":saved["dataRevision"],"useful":true,"reason":"first","expectedVersion":0});
        let first = feedback(&store, &v).unwrap();
        assert_eq!(first["version"], 1);
        assert_eq!(feedback(&store, &v).unwrap(), first);
        v["requestId"] = "feedback-2".into();
        assert!(matches!(
            feedback(&store, &v),
            Err(FinanceError::Conflict(_))
        ));
        v["expectedVersion"] = 1.into();
        v["useful"] = false.into();
        v["reason"] = "edited".into();
        assert_eq!(feedback(&store, &v).unwrap()["version"], 2);
        assert_eq!(
            stats(&store).unwrap()["feedback"],
            json!({"total":1,"useful":0,"usefulRatePpm":0})
        );
        assert_eq!(
            snapshots(&store, 1).unwrap()["items"][0]["items"][0]["feedback"]["reason"],
            "edited"
        );
        let history = feedback_history(
            &store,
            saved["snapshotId"].as_str().unwrap(),
            saved["items"][0]["candidateId"].as_str().unwrap(),
            1,
        )
        .unwrap();
        assert_eq!(history["total"], 2);
        assert_eq!(history["items"][0]["reason"], "edited");
        assert_eq!(history["items"][1]["reason"], "first");
        assert!(
            feedback_history(&store, saved["snapshotId"].as_str().unwrap(), "forged", 1).is_err()
        );
        assert_eq!(
            store
                .with_read(|c| c
                    .query_row("SELECT COUNT(*) FROM opportunity_feedback", [], |r| r
                        .get::<_, i64>(0)))
                .unwrap(),
            2
        );
        v["useful"] = 1.into();
        assert!(matches!(
            feedback(&store, &v),
            Err(FinanceError::Invalid(_))
        ));
        v["useful"] = true.into();
        v["reason"] = "长".repeat(201).into();
        assert!(matches!(
            feedback(&store, &v),
            Err(FinanceError::Invalid(_))
        ));
        assert!(snapshots(&store, 0).is_err());
        for version in 2..23 {
            v["requestId"] = format!("history-{version}").into();
            v["expectedVersion"] = version.into();
            v["reason"] = format!("version-{version}").into();
            feedback(&store, &v).unwrap();
        }
        let sid = saved["snapshotId"].as_str().unwrap();
        let cid = saved["items"][0]["candidateId"].as_str().unwrap();
        let first_page = feedback_history(&store, sid, cid, 1).unwrap();
        let second_page = feedback_history(&store, sid, cid, 2).unwrap();
        assert_eq!(first_page["items"].as_array().unwrap().len(), 20);
        assert_eq!(second_page["items"].as_array().unwrap().len(), 3);
        assert_eq!(second_page["items"][2]["reason"], "first");
        assert_eq!(stats(&store).unwrap()["feedback"]["total"], 1);
    }
}
