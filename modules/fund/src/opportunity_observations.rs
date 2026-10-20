//! 机会批次成熟结果观察（v21）。
//!
//! 契约：plan/03 §T06 报价观察契约、plan/05 §T06 报价观察适配。方向政策迁移自供体
//! `src/modules/automation/agent_prediction_evaluation.py`（MIT，commit
//! 00b345b05145705afea9ea9d5cecc090c009d83c）：buy/add 参考涨幅大于 0 记命中，
//! avoid 小于 0 记命中，hold/watch 绝对涨幅严格小于 2% 记命中；零涨幅不算上涨或
//! 下跌命中。原始 signal 来自不可变快照投影，缺 signal 的旧记录不按当前分数或
//! action 反推。
//!
//! 只读既有闭市缓存与已发布 NAV；显式 refresh 命令是唯一写入方，GET 只读且从不
//! 触发网络、调度或后台扫描。成熟结果首次保存后固定原始起止价／来源／观察时间，
//! 后续复权修正或用户反馈不改写。

use crate::{assets, finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) const HORIZONS: [i64; 3] = [3, 5, 10];
/// 供体 FLAT_THRESHOLD_PCT = 2.0，单位 ppm（1% = 10_000 ppm）。
const FLAT_THRESHOLD_PPM: i64 = 20_000;
/// 单价以 1e-4 定点保存，与基金净值 unit_nav_raw 同口径。
const PRICE_SCALE: f64 = 10_000.0;
/// 单价定点上限（1e7）：确定性拒绝损坏序列，i128/ppm 计算不溢出。
const MAX_PRICE_RAW: i64 = 100_000_000_000;
/// 留存序列最近报价在此天数内视为仍在更新：起点未现记未成熟；更久的缺口记
/// 缺价（停牌、退市或缓存未再更新）。
const FRESH_DAYS: i64 = assets::STALE_DAYS;

/// 单一证券的留存有效报价：日期升序、价格为正且在定点范围内。
struct Series {
    source: String,
    period: &'static str,
    quotes: Vec<(String, i64)>,
}

impl Series {
    fn empty(period: &'static str) -> Self {
        Self {
            source: String::new(),
            period,
            quotes: Vec::new(),
        }
    }
}

fn price_raw(close: f64) -> Option<i64> {
    if !close.is_finite() || close <= 0.0 {
        return None;
    }
    let raw = (close * PRICE_SCALE).round() as i64;
    (1..=MAX_PRICE_RAW).contains(&raw).then_some(raw)
}

/// 观察与投影／主动刷新共用同一数据面：场内取闭市缓存（仅沪深京），基金取已
/// 发布 NAV；港美股与过期缓存无可观察价格，不补公开行情。
fn load_series(conn: &Connection, item: &Value, today: &str) -> Series {
    let code = item["code"].as_str().unwrap_or_default();
    if item["kind"] == "fund" {
        let Ok((source, bars)) = crate::opportunities::nav_bars(conn, code, today) else {
            return Series::empty("published-nav");
        };
        Series {
            source,
            period: "published-nav",
            quotes: bars
                .into_iter()
                .filter(|b| b.date.as_str() <= today)
                .filter_map(|b| price_raw(b.close).map(|raw| (b.date, raw)))
                .collect(),
        }
    } else {
        let Some((value, bars)) = crate::market::cached_closed_bars(
            item["market"].as_str().unwrap_or_default(),
            code,
            today,
        ) else {
            return Series::empty("closed-day");
        };
        Series {
            source: value["source"]
                .as_str()
                .unwrap_or("web.ifzq.gtimg.cn")
                .to_string(),
            period: "closed-day",
            quotes: bars
                .into_iter()
                .filter(|b| b.date.as_str() <= today)
                .filter_map(|b| price_raw(b.close).map(|raw| (b.date, raw)))
                .collect(),
        }
    }
}

/// 供体方向政策；未知原始信号返回 None。
fn classify(signal: &str, return_ppm: i64) -> Option<bool> {
    match signal {
        "buy" | "add" => Some(return_ppm > 0),
        "avoid" => Some(return_ppm < 0),
        "hold" | "watch" => Some(return_ppm.abs() < FLAT_THRESHOLD_PPM),
        _ => None,
    }
}

fn direction(signal: &str) -> Option<&'static str> {
    match signal {
        "buy" | "add" => Some("up"),
        "avoid" => Some("down"),
        "hold" | "watch" => Some("flat"),
        _ => None,
    }
}

/// 快照生成日：persist 恒写 generatedDay；旧数据回退 generatedAt 换算北京时间。
fn generation_day(p: &Value) -> Result<String, FinanceError> {
    if let Some(day) = p["generatedDay"]
        .as_str()
        .filter(|d| assets::is_valid_calendar_date(d))
    {
        return Ok(day.to_string());
    }
    if let Some(secs) = p["generatedAt"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Ok(assets::civil_from_days((secs + 8 * 3600) / 86400));
    }
    Err(FinanceError::Invalid("快照缺少生成日".into()))
}

/// 单候选单区间评估。窗口以交易日计：只数有效报价，不补节假日、不用自然日
/// 冒称交易日；生成日及之前的报价不作起点，未来报价不进入。
fn evaluate(
    series: &Series,
    signal: Option<&str>,
    generated_day: &str,
    today: &str,
    horizon: i64,
    observed_at: &str,
) -> Value {
    let Some(signal) = signal else {
        return json!({
            "status": "signal_unknown",
            "baseline": null,
            "end": null,
            "returnPpm": null,
            "hit": null,
            "quoteCount": 0,
            "seriesFirst": null,
            "seriesLast": null,
            "observedAt": observed_at,
            "observedDay": today,
        });
    };
    let after: Vec<&(String, i64)> = series
        .quotes
        .iter()
        .filter(|(d, _)| d.as_str() > generated_day)
        .collect();
    let status = if series.quotes.is_empty() {
        "missing_price"
    } else if series.quotes[0].0.as_str() > generated_day {
        // 留存窗口起点晚于生成日：两者之间的报价可能已被淘汰，真实起点不可知。
        "window_lost"
    } else if after.is_empty() {
        let fresh = assets::days_from_civil(today)
            .zip(assets::days_from_civil(&series.quotes.last().unwrap().0))
            .is_some_and(|(t, l)| t - l <= FRESH_DAYS);
        if fresh {
            "immature"
        } else {
            "missing_price"
        }
    } else if (after.len() as i64) < horizon {
        "immature"
    } else {
        "mature"
    };
    let mut row = json!({
        "status": status,
        "baseline": null,
        "end": null,
        "returnPpm": null,
        "hit": null,
        "quoteCount": after.len(),
        "seriesFirst": series.quotes.first().map(|(d, _)| d.clone()),
        "seriesLast": series.quotes.last().map(|(d, _)| d.clone()),
        "observedAt": observed_at,
        "observedDay": today,
    });
    match status {
        "mature" => {
            let (base_date, base_raw) = after[0];
            let (end_date, end_raw) = after[horizon as usize - 1];
            let return_ppm = ((i128::from(*end_raw) - i128::from(*base_raw)) * 1_000_000
                / i128::from(*base_raw)) as i64;
            row["baseline"] = json!({"date": base_date, "priceRaw": base_raw, "source": series.source, "period": series.period});
            row["end"] = json!({"date": end_date, "priceRaw": end_raw, "source": series.source, "period": series.period});
            row["returnPpm"] = return_ppm.into();
            row["hit"] = classify(signal, return_ppm).into();
        }
        // 未成熟时起点已可确定；窗口丢失与缺价不给出起点，避免误导。
        "immature" if !after.is_empty() => {
            let (date, raw) = after[0];
            row["baseline"] = json!({"date": date, "priceRaw": raw, "source": series.source, "period": series.period});
        }
        _ => {}
    }
    row
}

/// 成熟结果首次保存后固定；未成熟／缺价／窗口丢失可在明确更新时重试。
fn save(
    conn: &Connection,
    sid: &str,
    cid: &str,
    horizon: i64,
    result: &Value,
) -> Result<(), FinanceError> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT result_json FROM opportunity_observations WHERE snapshot_id=?1 AND candidate_id=?2 AND horizon=?3",
            params![sid, cid, horizon],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = existing {
        if crate::opportunity_records::decode(&old)?["status"] == "mature" {
            return Ok(());
        }
    }
    conn.execute(
        "INSERT INTO opportunity_observations(snapshot_id,candidate_id,horizon,result_json) VALUES(?1,?2,?3,?4)
         ON CONFLICT(snapshot_id,candidate_id,horizon) DO UPDATE SET result_json=excluded.result_json",
        params![sid, cid, horizon, result.to_string()],
    )?;
    Ok(())
}

fn count_map(counts: &BTreeMap<String, i64>) -> Value {
    json!({
        "mature": counts.get("mature").copied().unwrap_or(0),
        "immature": counts.get("immature").copied().unwrap_or(0),
        "missingPrice": counts.get("missing_price").copied().unwrap_or(0),
        "windowLost": counts.get("window_lost").copied().unwrap_or(0),
        "signalUnknown": counts.get("signal_unknown").copied().unwrap_or(0),
    })
}

/// 显式观察更新：载荷仅 requestId+snapshotId；服务端读取该批全部候选（≤100），
/// 在一个写事务中用既有闭市缓存／已发布 NAV 评估 3/5/10 个有效报价区间并保存，
/// 复用请求重放与异载荷冲突。完整事务失败整体回滚，不产生半批观察。
pub(crate) fn refresh(store: &Store, v: &Value) -> Result<Value, FinanceError> {
    crate::opportunity_records::validate(v, &["requestId", "snapshotId"])?;
    let sid = crate::opportunity_records::text(v, "snapshotId", 256)?.to_string();
    if let Some(result) = store.with_read(|c| crate::opportunity_records::replay(c, v))? {
        return Ok(result);
    }
    let projection = store.with_read(|conn| -> Result<Value, FinanceError> {
        let saved: Option<String> = conn
            .query_row(
                "SELECT projection_json FROM opportunity_snapshots WHERE snapshot_id=?1",
                [&sid],
                |r| r.get(0),
            )
            .optional()?;
        let p = crate::opportunity_records::decode(
            &saved.ok_or_else(|| FinanceError::NotFound("候选快照".into()))?,
        )?;
        generation_day(&p)?;
        Ok(p)
    })?;
    let today = crate::finance::today_cn();
    let observed_at = crate::finance::now_secs();
    store.with_write(|conn| {
        if let Some(result) = crate::opportunity_records::replay(conn, v)? {
            return Ok(result);
        }
        let day = generation_day(&projection)?;
        let items = projection["items"]
            .as_array()
            .ok_or_else(|| FinanceError::Db("候选结构无效".into()))?;
        let mut horizons: BTreeMap<i64, BTreeMap<String, i64>> =
            HORIZONS.iter().map(|h| (*h, BTreeMap::new())).collect();
        let mut candidates = 0i64;
        for item in items.iter().take(100) {
            let cid = item["candidateId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.chars().count() <= 320)
                .ok_or_else(|| FinanceError::Db("候选身份无效".into()))?;
            candidates += 1;
            let signal = item["signal"].as_str();
            let series = if signal.is_none() {
                Series::empty("none")
            } else {
                load_series(conn, item, &today)
            };
            for horizon in HORIZONS {
                let result = evaluate(&series, signal, &day, &today, horizon, &observed_at);
                *horizons
                    .get_mut(&horizon)
                    .unwrap()
                    .entry(result["status"].as_str().unwrap_or("missing_price").into())
                    .or_default() += 1;
                save(conn, &sid, cid, horizon, &result)?;
            }
        }
        let summary = json!({
            "snapshotId": sid,
            "observedDay": today,
            "candidates": candidates,
            "horizons": horizons
                .into_iter()
                .map(|(h, m)| (h.to_string(), count_map(&m)))
                .collect::<Map<_, _>>(),
        });
        crate::opportunity_records::remember(conn, v, &summary)?;
        Ok(summary)
    })
}

#[derive(Default)]
struct Bucket {
    total: i64,
    unobserved: i64,
    signal_unknown: i64,
    missing_price: i64,
    immature: i64,
    window_lost: i64,
    mature: i64,
    up: (i64, i64),
    down: (i64, i64),
    flat: (i64, i64),
}

/// 全部保存候选作为每个区间的完整覆盖分母：弱分、无反馈、无收益样本都不排除；
/// 未观察、未成熟、缺价、窗口丢失、旧信号不可评估分别计数，方向分别统计。
/// 方向命中率不是反馈有用率，也不是实际收益或卖空收益。
pub(crate) fn coverage(conn: &Connection) -> Result<Value, FinanceError> {
    let mut rows = BTreeMap::<(String, String, i64), Value>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT snapshot_id,candidate_id,horizon,result_json FROM opportunity_observations",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in mapped {
            let (sid, cid, horizon, raw) = row?;
            rows.insert(
                (sid, cid, horizon),
                crate::opportunity_records::decode(&raw)?,
            );
        }
    }
    let mut buckets: BTreeMap<i64, Bucket> =
        HORIZONS.iter().map(|h| (*h, Bucket::default())).collect();
    let mut stmt = conn.prepare("SELECT projection_json FROM opportunity_snapshots")?;
    let projections = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for raw in projections {
        let p = crate::opportunity_records::decode(&raw)?;
        let sid = p["snapshotId"].as_str().unwrap_or_default().to_string();
        // 生成日不可用的旧快照无法评估，候选保持未观察计数。
        let day = generation_day(&p).is_ok();
        let Some(items) = p["items"].as_array() else {
            continue;
        };
        for item in items.iter().take(100) {
            let Some(cid) = item["candidateId"].as_str().map(str::to_string) else {
                continue;
            };
            for horizon in HORIZONS {
                let bucket = buckets.get_mut(&horizon).unwrap();
                bucket.total += 1;
                let row = if day {
                    rows.get(&(sid.clone(), cid.clone(), horizon)).cloned()
                } else {
                    None
                };
                let sig = match item["signal"].as_str() {
                    Some(sig) if direction(sig).is_some() => sig,
                    _ => {
                        bucket.signal_unknown += 1;
                        continue;
                    }
                };
                match row
                    .as_ref()
                    .map(|r| r["status"].as_str().unwrap_or("unobserved"))
                    .unwrap_or("unobserved")
                {
                    "mature" => {
                        let hit = row
                            .as_ref()
                            .and_then(|r| r["returnPpm"].as_i64())
                            .and_then(|ppm| classify(sig, ppm));
                        bucket.mature += 1;
                        let slot = match direction(sig).unwrap_or_default() {
                            "up" => &mut bucket.up,
                            "down" => &mut bucket.down,
                            _ => &mut bucket.flat,
                        };
                        slot.0 += 1;
                        slot.1 += i64::from(hit == Some(true));
                    }
                    "immature" => bucket.immature += 1,
                    "missing_price" => bucket.missing_price += 1,
                    "window_lost" => bucket.window_lost += 1,
                    "signal_unknown" => bucket.signal_unknown += 1,
                    _ => bucket.unobserved += 1,
                }
            }
        }
    }
    let rate = |(total, hits): (i64, i64)| {
        json!({
            "total": total,
            "hits": hits,
            "hitRatePpm": if total > 0 { Some(hits * 1_000_000 / total) } else { None },
        })
    };
    let mut horizons = Map::new();
    for horizon in HORIZONS {
        let bucket = &buckets[&horizon];
        horizons.insert(
            horizon.to_string(),
            json!({
                "total": bucket.total,
                "unobserved": bucket.unobserved,
                "signalUnknown": bucket.signal_unknown,
                "missingPrice": bucket.missing_price,
                "immature": bucket.immature,
                "windowLost": bucket.window_lost,
                "mature": bucket.mature,
                "directions": {
                    "up": rate(bucket.up),
                    "down": rate(bucket.down),
                    "flat": rate(bucket.flat),
                },
            }),
        );
    }
    Ok(json!({
        "horizons": Value::Object(horizons),
        "metric": "direction-hit-not-useful-rate-not-actual-return",
        "flatThresholdPpm": FLAT_THRESHOLD_PPM,
    }))
}

/// 历史批次读取附带各区间观察结果；只读，缺省区间为 null。
pub(crate) fn attach(
    conn: &Connection,
    sid: &str,
    items: &mut [Value],
) -> Result<(), FinanceError> {
    let mut rows = BTreeMap::<(String, i64), Value>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT candidate_id,horizon,result_json FROM opportunity_observations WHERE snapshot_id=?1",
        )?;
        let mapped = stmt.query_map([sid], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in mapped {
            let (cid, horizon, raw) = row?;
            rows.insert((cid, horizon), crate::opportunity_records::decode(&raw)?);
        }
    }
    for item in items.iter_mut() {
        let Some(cid) = item["candidateId"].as_str().map(str::to_string) else {
            continue;
        };
        let mut horizons = Map::new();
        for horizon in HORIZONS {
            horizons.insert(
                horizon.to_string(),
                rows.get(&(cid.clone(), horizon))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        item["observation"] = json!({"horizons": Value::Object(horizons)});
    }
    Ok(())
}
#[cfg(test)]
#[path = "opportunity_observations_tests.rs"]
mod tests;
