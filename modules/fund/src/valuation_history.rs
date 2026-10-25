//! Derived valuation observations; never a checked return baseline.
use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn groups(snapshot: &Value) -> Vec<Value> {
    snapshot["groupAllocation"]["groups"]
        .as_array()
        .map(|rows| {
            rows.iter().map(|g|
        json!({"groupId":g["groupId"],"name":g["name"],"valueMinor":g["currentMinor"]})
    ).collect()
        })
        .unwrap_or_default()
}

fn current(snapshot: &Value) -> Result<Value, FinanceError> {
    let at = snapshot["generatedAt"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|at| *at > 0)
        .ok_or_else(|| FinanceError::Invalid("估值观察时间无效".into()))?;
    Ok(
        json!({"date":snapshot["requestedRange"]["today"],"observedAt":at,
        "valueMinor":snapshot["wealth"]["totalA"],"quality":if snapshot["wealth"]["quality"]=="ready" {"reference"} else {"incomplete"},
        "groups":groups(snapshot),"source":"current-valuation"}),
    )
}

pub(crate) fn observe(store: &Store, snapshot: &mut Value) -> Result<(), FinanceError> {
    if snapshot["requestedRange"]["to"] != snapshot["requestedRange"]["today"] {
        return Ok(());
    }
    store.with_observation_write(|conn| {
        if Store::data_revision(conn)? != snapshot["dataRevision"].as_str().unwrap_or("") {
            return Err(FinanceError::Conflict(
                "估值期间数据已变化，请刷新重试".into(),
            ));
        }
        record(conn, snapshot)?;
        let from = snapshot["requestedRange"]["from"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let to = snapshot["requestedRange"]["to"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        project(conn, snapshot, &from, &to)
    })
}

fn record(conn: &Connection, snapshot: &Value) -> Result<(), FinanceError> {
    if snapshot["wealth"]["quality"] == "empty" {
        return Ok(());
    }
    let point = current(snapshot)?;
    let at = point["observedAt"]
        .as_i64()
        .ok_or_else(|| FinanceError::Invalid("估值观察时间无效".into()))?;
    let date = point["date"]
        .as_str()
        .ok_or_else(|| FinanceError::Invalid("估值观察日期无效".into()))?;
    if crate::assets::civil_from_days((at + 8 * 3600) / 86400) != date {
        return Err(FinanceError::Conflict("估值跨日，请刷新重试".into()));
    }
    let value = point["valueMinor"].as_i64().or_else(|| {
        point["valueMinor"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
    });
    conn.execute("INSERT INTO valuation_observations(minute,date,observed_at,value_minor,quality,groups_json,revision,valuation_json)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(minute) DO UPDATE SET
        observed_at=excluded.observed_at,value_minor=excluded.value_minor,quality=excluded.quality,
        groups_json=excluded.groups_json,revision=excluded.revision,valuation_json=excluded.valuation_json
        WHERE excluded.observed_at>=valuation_observations.observed_at",
        params![at/60,date,at,value,point["quality"].as_str(),point["groups"].to_string(),snapshot["dataRevision"].as_str(),snapshot["wealth"]["valuation"].to_string()])?;
    // Intraday precision is retained for today; older days keep their last real observation.
    conn.execute(
        "DELETE FROM valuation_observations WHERE date<?1 OR
        (date<?2 AND minute NOT IN (SELECT MAX(minute) FROM valuation_observations GROUP BY date))",
        params![crate::assets::minus_days(date, 399), date],
    )?;
    Ok(())
}

fn observation_rows(
    conn: &Connection,
    from: &str,
    to: &str,
    intraday: bool,
) -> Result<Vec<Value>, FinanceError> {
    let sql = if intraday {
        "SELECT date,observed_at,value_minor,quality,groups_json FROM valuation_observations WHERE date=?2 ORDER BY minute LIMIT 1440"
    } else {
        "SELECT date,observed_at,value_minor,quality,groups_json FROM valuation_observations WHERE minute IN
        (SELECT MAX(minute) FROM valuation_observations WHERE date>=?1 AND date<=?2 GROUP BY date ORDER BY date DESC LIMIT 400) ORDER BY date"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![from, to], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (date,at,value,quality,raw)=row?;
        let groups:Value=serde_json::from_str(&raw).map_err(|_|FinanceError::Invalid("估值观察分类损坏".into()))?;
        Ok(json!({"date":date,"observedAt":at,"valueMinor":value.map(|v|v.to_string()),"quality":quality,"groups":groups,"source":"valuation-observation"}))
    }).collect()
}

pub(crate) fn project(
    conn: &Connection,
    snapshot: &mut Value,
    from: &str,
    to: &str,
) -> Result<(), FinanceError> {
    let mut intraday = snapshot["requestedRange"]["key"] == "today";
    let mut observations = observation_rows(conn, from, to, intraday)?;
    if !intraday
        && snapshot["requestedRange"]["today"] == to
        && observations
            .iter()
            .chain(
                snapshot["portfolio"]["history"]["points"]
                    .as_array()
                    .into_iter()
                    .flatten(),
            )
            .all(|p| p["date"] == to)
    {
        intraday = true;
        observations = observation_rows(conn, from, to, true)?;
    }
    if !intraday && observations.is_empty() {
        if snapshot["requestedRange"]["today"] == to {
            snapshot["portfolio"]["history"]["todayPoints"] =
                today_points(conn, snapshot, to)?.into();
        }
        return Ok(());
    }
    let mut points = BTreeMap::<i64, Value>::new();
    if intraday {
        let mut stmt=conn.prepare("SELECT created_at,total_minor,quality,breakdown_json FROM value_snapshots WHERE scope='A' AND taken_at=?1 ORDER BY id DESC LIMIT 200")?;
        let rows = stmt.query_map([to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (created, value, quality, raw) = row?;
            let Some(at) = created.parse::<i64>().ok().filter(|at| {
                *at > 0 && crate::assets::civil_from_days((*at + 8 * 3600) / 86400) == to
            }) else {
                continue;
            };
            let breakdown: Value = serde_json::from_str(&raw)
                .map_err(|_| FinanceError::Invalid("估值快照分类损坏".into()))?;
            let point = json!({"date":to,"observedAt":at,"valueMinor":value.to_string(),"quality":quality,"groups":breakdown.get("groupValues").cloned().unwrap_or(json!([])),"source":"value-snapshot"});
            if points
                .get(&(at / 60))
                .is_none_or(|p| p["observedAt"].as_i64().unwrap_or(0) < at)
            {
                points.insert(at / 60, point);
            }
        }
    }
    for point in observations {
        let at = point["observedAt"].as_i64().unwrap_or(0);
        let key = if intraday {
            at / 60
        } else {
            crate::assets::days_from_civil(point["date"].as_str().unwrap_or("")).unwrap_or(0)
        };
        if points
            .get(&key)
            .is_none_or(|p| p["observedAt"].as_i64().unwrap_or(0) <= at)
        {
            points.insert(key, point);
        }
    }
    if intraday {
        if snapshot["wealth"]["quality"] != "empty" {
            let point = current(snapshot)?;
            let at = point["observedAt"].as_i64().unwrap_or(0);
            if points
                .get(&(at / 60))
                .is_none_or(|p| p["observedAt"].as_i64().unwrap_or(0) <= at)
            {
                points.insert(at / 60, point);
            }
        }
    } else {
        for point in snapshot["portfolio"]["history"]["points"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(day) = point["date"]
                .as_str()
                .and_then(crate::assets::days_from_civil)
            {
                if point["source"] == "holding-nav-replay" {
                    points.entry(day).or_insert_with(|| point.clone());
                } else {
                    points.insert(day, point.clone());
                }
            }
        }
    }
    let mut values: Vec<Value> = points.into_values().collect();
    if intraday {
        values = sample_intraday(values);
    } else if values.len() > 400 {
        values.drain(..values.len() - 400);
        snapshot["portfolio"]["history"]["truncated"] = true.into();
    }
    let history = &mut snapshot["portfolio"]["history"];
    if from.is_empty() {
        if let Some(first) = values.first() {
            history["from"] = first["date"].clone();
        }
    }
    let uses_observations = intraday
        || values
            .iter()
            .any(|p| p["source"] == "valuation-observation");
    history["points"] = values.into();
    history["timeResolution"] = if intraday { "minute" } else { "day" }.into();
    if uses_observations {
        history["source"] = "value-snapshots-and-valuation-observations".into();
        history["status"] = "reference".into();
        // An observed value cannot turn missing checked endpoints into a return rate.
        history["twrPpm"] = Value::Null;
    }
    if !intraday && snapshot["requestedRange"]["today"] == to {
        snapshot["portfolio"]["history"]["todayPoints"] = today_points(conn, snapshot, to)?.into();
    }
    Ok(())
}

// The narrow projection lets a newly configured category show today's real points
// while the total asset chart and return controls keep their existing daily range.
fn today_points(
    conn: &Connection,
    snapshot: &Value,
    today: &str,
) -> Result<Vec<Value>, FinanceError> {
    let current_groups: Vec<Value> = groups(snapshot)
        .into_iter()
        .map(|g| json!({"groupId":g["groupId"],"name":g["name"],"currentMinor":g["valueMinor"]}))
        .collect();
    let mut narrow = json!({"generatedAt":snapshot["generatedAt"],
        "wealth":{"quality":snapshot["wealth"]["quality"],"totalA":snapshot["wealth"]["totalA"]},
        "groupAllocation":{"groups":current_groups},
        "requestedRange":{"key":"today","today":today,"to":today},"portfolio":{"history":{"points":[]}}});
    project(conn, &mut narrow, today, today)?;
    Ok(narrow["portfolio"]["history"]["points"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn sample_intraday(mut points: Vec<Value>) -> Vec<Value> {
    for i in 1..points.len() {
        let gap = points[i]["observedAt"].as_i64().unwrap_or(0)
            - points[i - 1]["observedAt"].as_i64().unwrap_or(0)
            > 300;
        points[i]["breakBefore"] = gap.into();
    }
    let bytes: usize = points.iter().map(|p| p.to_string().len()).sum();
    let limit = if bytes > 0 {
        (95 * 1024 * points.len() / bytes).clamp(2, 200)
    } else {
        200
    };
    if points.len() <= limit {
        return points;
    }
    let mut sampled = Vec::new();
    let mut previous = 0;
    for n in 0..limit {
        let index = n * (points.len() - 1) / (limit - 1);
        let mut point = points[index].clone();
        let group_ids: Vec<&Value> = points[index]["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|g| &g["groupId"])
            .collect();
        // Never conceal a missing value/category or a disconnected period when reducing points.
        point["breakBefore"] = points[previous + usize::from(n > 0)..=index]
            .iter()
            .any(|p| {
                p["breakBefore"] == true
                    || p["valueMinor"].is_null()
                    || p["quality"] == "incomplete"
                    || group_ids.iter().any(|id| {
                        !p["groups"].as_array().is_some_and(|gs| {
                            gs.iter()
                                .any(|g| &g["groupId"] == *id && !g["valueMinor"].is_null())
                        })
                    })
            })
            .into();
        sampled.push(point);
        previous = index;
    }
    sampled
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v29_upgrade_preserves_checked_snapshots_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("fund.db")).unwrap();
        for step in crate::migration::all_migrations()
            .iter()
            .filter(|step| step.version <= 28)
        {
            conn.execute_batch(step.sql).unwrap();
            conn.pragma_update(None, "user_version", step.version)
                .unwrap();
        }
        conn.execute("INSERT INTO value_snapshots(taken_at,total_minor,quality,created_at) VALUES('2000-01-01',12345,'ready','0')",[]).unwrap();
        for _ in 0..2 {
            assert_eq!(
                crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
                29
            );
        }
        assert_eq!(
            conn.query_row("SELECT total_minor FROM value_snapshots", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            12345
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM valuation_observations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    fn snapshot(date: &str, minute: i64, value: Option<i64>) -> Value {
        let at = crate::assets::days_from_civil(date).unwrap() * 86400 - 8 * 3600 + minute * 60;
        json!({"generatedAt":at.to_string(),"dataRevision":"0",
            "requestedRange":{"key":"today","from":date,"to":date,"today":date},
            "wealth":{"totalA":value,"quality":if value.is_some(){"ready"}else{"incomplete"},"valuation":{"batch":"test"}},
            "groupAllocation":{"groups":[{"groupId":"g","currentMinor":value}]},
            "portfolio":{"history":{"points":[],"twrPpm":null}}})
    }
    fn observe_fixture(store: &Store, snapshot: &mut Value) {
        snapshot["dataRevision"] = store.with_read(Store::data_revision).unwrap().into();
        observe(store, snapshot).unwrap();
    }
    fn count(store: &Store, table: &str) -> i64 {
        store
            .with_read(|c| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)))
            .unwrap()
    }
    #[test]
    fn intraday_preserves_real_times_amounts_and_revision_without_checked_baselines() {
        let store = Store::open_in_memory().unwrap();
        let date = crate::finance::today_cn();
        let revision = store.with_read(Store::data_revision).unwrap();
        let mut first = snapshot(&date, 570, Some(10000));
        observe_fixture(&store, &mut first);
        assert_eq!(
            first["portfolio"]["history"]["points"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut next = snapshot(&date, 571, Some(12000));
        observe_fixture(&store, &mut next);
        let points = next["portfolio"]["history"]["points"].as_array().unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0]["groups"][0]["valueMinor"], 10000);
        assert_eq!(points[1]["groups"][0]["valueMinor"], 12000);
        assert_ne!(points[0]["observedAt"], points[1]["observedAt"]);
        assert_eq!(count(&store, "value_snapshots"), 0);
        assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
        observe_fixture(&store, &mut next);
        assert_eq!(
            count(&store, "valuation_observations"),
            2,
            "multiple surfaces in the same minute are idempotent"
        );
        next["dataRevision"] = "stale".into();
        assert!(matches!(
            observe(&store, &mut next),
            Err(FinanceError::Conflict(_))
        ));
        assert_eq!(count(&store, "valuation_observations"), 2);
        next["requestedRange"]["key"] = "month".into();
        store
            .with_read(|c| project(c, &mut next, "", &date))
            .unwrap();
        assert_eq!(
            next["portfolio"]["history"]["timeResolution"], "minute",
            "a new portfolio needs no extra range selection to see today's trend"
        );
    }
    #[test]
    fn same_day_checked_snapshots_keep_times_but_import_dates_are_not_fabricated() {
        let store = Store::open_in_memory().unwrap();
        let date = crate::finance::today_cn();
        let mut s = snapshot(&date, 572, Some(30000));
        let at = s["generatedAt"].as_str().unwrap().parse::<i64>().unwrap();
        store.with_write(|c| {
            for (offset,value) in [(120,10000),(60,20000)] {
                c.execute("INSERT INTO value_snapshots(taken_at,total_minor,quality,created_at) VALUES(?1,?2,'ready',?3)",params![date,value,(at-offset).to_string()])?;
            }
            c.execute("INSERT INTO value_snapshots(taken_at,total_minor,quality,created_at) VALUES(?1,999999,'ready','0')",[&date])?;
            Ok::<_,rusqlite::Error>(())
        }).unwrap();
        store
            .with_read(|c| project(c, &mut s, &date, &date))
            .unwrap();
        let points = s["portfolio"]["history"]["points"].as_array().unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0]["valueMinor"], "10000");
        assert_eq!(points[1]["valueMinor"], "20000");
        assert!(!points.iter().any(|p| p["valueMinor"] == "999999"));
    }
    #[test]
    fn observations_survive_reopen_and_queries_are_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let date = crate::finance::today_cn();
        {
            let store = Store::open(dir.path()).unwrap();
            let mut s = snapshot(&date, 570, Some(0));
            s["dataRevision"] = store.with_read(Store::data_revision).unwrap().into();
            observe_fixture(&store, &mut s);
        }
        let store = Store::open(dir.path()).unwrap();
        let mut s = snapshot(&date, 571, Some(0));
        let revision = store.with_read(Store::data_revision).unwrap();
        store
            .with_read(|c| project(c, &mut s, &date, &date))
            .unwrap();
        assert_eq!(
            s["portfolio"]["history"]["points"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(count(&store, "valuation_observations"), 1);
        assert_eq!(store.with_read(Store::data_revision).unwrap(), revision);
        assert_eq!(s["portfolio"]["history"]["points"][0]["valueMinor"], "0");
    }
    #[test]
    fn minute_updates_keep_newest_and_unknowns_are_not_zero_or_returns() {
        let store = Store::open_in_memory().unwrap();
        let date = crate::finance::today_cn();
        let mut s = snapshot(&date, 570, Some(10000));
        observe_fixture(&store, &mut s);
        let stamp = s["generatedAt"].as_str().unwrap().parse::<i64>().unwrap();
        s["generatedAt"] = (stamp + 30).to_string().into();
        s["wealth"]["totalA"] = 11000.into();
        observe_fixture(&store, &mut s);
        let mut older = snapshot(&date, 570, Some(9000));
        observe_fixture(&store, &mut older);
        assert_eq!(
            older["portfolio"]["history"]["points"][0]["valueMinor"],
            "11000"
        );
        let mut unknown = snapshot(&date, 571, None);
        observe_fixture(&store, &mut unknown);
        let points = &unknown["portfolio"]["history"]["points"];
        assert!(points[1]["valueMinor"].is_null());
        assert_eq!(points[1]["quality"], "incomplete");
        assert!(unknown["portfolio"]["history"]["twrPpm"].is_null());
    }
    #[test]
    fn daily_retention_is_bounded_without_erasing_checked_snapshots() {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        for ago in [401, 1, 0] {
            let day = crate::assets::minus_days(&today, ago);
            for minute in [570, 571] {
                let mut s = snapshot(&day, minute, Some(10000));
                observe_fixture(&store, &mut s);
            }
        }
        assert_eq!(
            count(&store, "valuation_observations"),
            3,
            "two today plus yesterday's last point"
        );
        let mut s = snapshot(&today, 572, Some(10000));
        s["requestedRange"]["key"] = "all".into();
        store.with_read(|c| project(c, &mut s, "", &today)).unwrap();
        assert_eq!(
            s["portfolio"]["history"]["points"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let yesterday = crate::assets::minus_days(&today, 1);
        assert_eq!(s["portfolio"]["history"]["points"][0]["date"], yesterday);
        assert_eq!(
            s["portfolio"]["history"]["todayPoints"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(s["portfolio"]["history"]["from"], yesterday);
        let before = count(&store, "valuation_observations");
        s["requestedRange"]["to"] = yesterday.into();
        observe_fixture(&store, &mut s);
        assert_eq!(
            count(&store, "valuation_observations"),
            before,
            "historical queries cannot record today's money"
        );
    }
    #[test]
    fn bounded_projection_keeps_endpoints_and_missing_periods() {
        let date = crate::finance::today_cn();
        let mut points = Vec::new();
        for n in 0..600 {
            let mut p = current(&snapshot(&date, 1 + n, Some(10000))).unwrap();
            if n == 300 {
                p["valueMinor"] = Value::Null;
                p["quality"] = "incomplete".into();
            }
            points.push(p);
        }
        let first = points[0].clone();
        let last = points[599].clone();
        let result = sample_intraday(points);
        assert_eq!(result.len(), 200);
        assert_eq!(result[0]["observedAt"], first["observedAt"]);
        assert_eq!(result[199]["observedAt"], last["observedAt"]);
        assert!(result.iter().any(|p| p["breakBefore"] == true));
    }
}
