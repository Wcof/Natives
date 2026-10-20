//! 策略目录与手动权重（v22）。
//!
//! 供体适配：`src/modules/strategy/strategy_catalog.py`（目录与默认权重）与
//! `strategy_engine.py::_compute_rank_score`（rank = base × weight）。供体的
//! watchlist_agent（Agent 建议）按 CONTEXT 排除；供体逐市场/逐 regime 权重表
//! 在单市场扫描下简化为每码一个手动覆盖（恢复即删覆盖回到默认）。权重以 ppm
//! 定点保存（1_000_000 = 1.0x），合法范围 [0.1x, 3.0x]，非法值显式拒绝而非
//! clamp。目录变更推进 strategy_config.revision；投影快照身份包含该 revision，
//! 参数变化即产生新候选，旧快照保留旧版本。

use crate::{finance::FinanceError, storage::Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

/// 权重合法范围：0.1x ～ 3.0x（ppm）。
pub(crate) const MIN_WEIGHT_PPM: i64 = 100_000;
pub(crate) const MAX_WEIGHT_PPM: i64 = 3_000_000;
const WEIGHT_SCALE: i64 = 1_000_000;

/// 目录默认行：(code, name, risk_level, horizon_days, default_weight_ppm)。
/// code 与投影 strategy_tags 一致；macd_positive 即供体 macd_golden（1.10x），
/// momentum 为供体未知码回退 1.0x 的显式化。
const DEFAULT_STRATEGIES: &[(&str, &str, &str, i64, i64)] = &[
    ("trend_follow", "趋势延续", "medium", 5, 1_150_000),
    ("macd_positive", "MACD金叉", "medium", 3, 1_100_000),
    ("volume_breakout", "放量突破", "high", 3, 1_180_000),
    ("pullback", "回踩确认", "low", 5, 1_050_000),
    ("rebound", "超跌反弹", "high", 3, 950_000),
    ("momentum", "动量延续", "medium", 3, 1_000_000),
    ("market_scan", "市场扫描", "medium", 3, 1_080_000),
];
pub(crate) fn known_code(code: &str) -> bool {
    DEFAULT_STRATEGIES.iter().any(|s| s.0 == code)
}

/// 首次使用补齐缺失默认行（幂等）；已存在行不被覆盖，保持用户视角稳定。
pub(crate) fn ensure_catalog(conn: &Connection) -> Result<(), FinanceError> {
    for (code, name, risk, horizon, weight) in DEFAULT_STRATEGIES {
        conn.execute(
            "INSERT OR IGNORE INTO strategy_catalog(code,name,description,version,enabled,market_scope,risk_level,params_json,default_weight_ppm,updated_at) VALUES(?1,?2,'','v1',1,'ALL',?3,?4,?5,'')",
            params![code, name, risk, json!({"horizonDays": horizon}).to_string(), weight],
        )?;
    }
    Ok(())
}

fn revision(conn: &Connection) -> Result<i64, FinanceError> {
    let r: i64 = conn.query_row("SELECT revision FROM strategy_config WHERE id=1", [], |r| {
        r.get(0)
    })?;
    Ok(r)
}

fn bump_revision(conn: &Connection) -> Result<i64, FinanceError> {
    conn.execute(
        "UPDATE strategy_config SET revision=revision+1, updated_at=?1 WHERE id=1",
        [crate::finance::now_secs()],
    )?;
    revision(conn)
}

/// 生效权重表：启用码 → ppm（手动覆盖优先，否则默认）。
pub(crate) fn effective_weights(conn: &Connection) -> Result<Value, FinanceError> {
    ensure_catalog(conn)?;
    let rev = revision(conn)?;
    let mut stmt = conn.prepare(
        "SELECT c.code, c.default_weight_ppm, o.weight_ppm, c.enabled FROM strategy_catalog c
         LEFT JOIN strategy_weight_overrides o ON o.code=c.code ORDER BY c.code",
    )?;
    let mut weights = serde_json::Map::new();
    let mut items = stmt.query([])?;
    while let Some(row) = items.next()? {
        let code: String = row.get(0)?;
        let default_ppm: i64 = row.get(1)?;
        let override_ppm: Option<i64> = row.get(2)?;
        let enabled: bool = row.get(3)?;
        let effective = override_ppm.unwrap_or(default_ppm);
        if !(MIN_WEIGHT_PPM..=MAX_WEIGHT_PPM).contains(&effective) {
            return Err(FinanceError::Db(format!("策略 {code} 权重超出合法范围")));
        }
        weights.insert(
            code,
            json!({"weightPpm": effective, "enabled": enabled, "overridden": override_ppm.is_some()}),
        );
    }
    Ok(json!({"revision": rev, "weights": Value::Object(weights)}))
}

/// 目录清单（含生效权重与参数）。
pub(crate) fn list(store: &Store) -> Result<Value, FinanceError> {
    store.with_snapshot(|conn| {
        ensure_catalog(conn)?;
        let rev = revision(conn)?;
        let mut stmt = conn.prepare(
            "SELECT c.code,c.name,c.description,c.version,c.enabled,c.market_scope,c.risk_level,c.params_json,c.default_weight_ppm,o.weight_ppm FROM strategy_catalog c LEFT JOIN strategy_weight_overrides o ON o.code=c.code ORDER BY c.code",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, Option<i64>>(9)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (code, name, description, version, enabled, market_scope, risk_level, params_json, default_ppm, override_ppm) = row?;
            let params: Value = serde_json::from_str(&params_json)
                .map_err(|_| FinanceError::Db("策略参数无法读取".into()))?;
            items.push(json!({
                "code": code,
                "name": name,
                "description": description,
                "version": version,
                "enabled": enabled,
                "marketScope": market_scope,
                "riskLevel": risk_level,
                "params": params,
                "defaultWeightPpm": default_ppm,
                "weightPpm": override_ppm.unwrap_or(default_ppm),
                "overridden": override_ppm.is_some(),
            }));
        }
        Ok(json!({"revision": rev, "items": items}))
    })
}

/// 目录更新：启用开关、手动权重（restore 清除覆盖）。非法值显式拒绝；
/// 乐观锁 expectedRevision 冲突为 409；requestId 幂等重放。
pub(crate) fn update(store: &Store, v: &Value) -> Result<Value, FinanceError> {
    // 可变载荷字段是开放集合（enabled/weightPpm/restore），不能复用机会记录的
    // 等字段数校验；这里按白名单手工校验。
    const BASE: [&str; 3] = ["requestId", "code", "expectedRevision"];
    const OPTIONAL: [&str; 3] = ["enabled", "weightPpm", "restore"];
    if !v.as_object().is_some_and(|o| {
        BASE.iter().all(|k| o.contains_key(*k))
            && o.keys()
                .all(|k| BASE.contains(&k.as_str()) || OPTIONAL.contains(&k.as_str()))
    }) {
        return Err(FinanceError::Invalid("字段无效".into()));
    }
    crate::opportunity_records::text(v, "requestId", 128)?
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        .then_some(())
        .ok_or_else(|| FinanceError::Invalid("requestId 无效".into()))?;
    let code = crate::opportunity_records::text(v, "code", 64)?.to_string();
    let expected = v["expectedRevision"]
        .as_i64()
        .filter(|n| *n >= 0)
        .ok_or_else(|| FinanceError::Invalid("expectedRevision 无效".into()))?;
    let enabled = match v.get("enabled") {
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(FinanceError::Invalid("enabled 必须为布尔值".into())),
        None => None,
    };
    enum WeightChange {
        None,
        Set(i64),
    }
    let weight = match v.get("weightPpm") {
        Some(Value::Number(n)) => {
            let ppm = n
                .as_i64()
                .ok_or_else(|| FinanceError::Invalid("weightPpm 必须为整数 ppm".into()))?;
            if !(MIN_WEIGHT_PPM..=MAX_WEIGHT_PPM).contains(&ppm) {
                return Err(FinanceError::Invalid(format!(
                    "weightPpm 必须在 {MIN_WEIGHT_PPM}~{MAX_WEIGHT_PPM}（0.1x~3.0x）内"
                )));
            }
            WeightChange::Set(ppm)
        }
        Some(Value::Null) | None => WeightChange::None,
        Some(_) => {
            return Err(FinanceError::Invalid(
                "weightPpm 必须为整数 ppm 或 null".into(),
            ))
        }
    };
    let restore = v.get("restore").and_then(Value::as_bool).unwrap_or(false);
    // restore 只清权重覆盖，与 enabled 开关可组合；但不能和手动写入新权重并存。
    if restore && matches!(weight, WeightChange::Set(_)) {
        return Err(FinanceError::Invalid(
            "restore 不能与 weightPpm 同时提交".into(),
        ));
    }
    if !restore && enabled.is_none() && matches!(weight, WeightChange::None) {
        return Err(FinanceError::Invalid("没有任何变更".into()));
    }
    if let Some(result) = store.with_read(|c| crate::opportunity_records::replay(c, v))? {
        return Ok(result);
    }
    store.with_write(|conn| {
        if let Some(result) = crate::opportunity_records::replay(conn, v)? {
            return Ok(result);
        }
        ensure_catalog(conn)?;
        let current = revision(conn)?;
        if current != expected {
            return Err(FinanceError::Conflict("策略配置已被修改，请重新读取".into()));
        }
        let exists: Option<String> = conn
            .query_row(
                "SELECT code FROM strategy_catalog WHERE code=?1",
                [&code],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(FinanceError::NotFound("策略".into()));
        }
        if let Some(e) = enabled {
            conn.execute(
                "UPDATE strategy_catalog SET enabled=?1, updated_at=?2 WHERE code=?3",
                params![i64::from(e), crate::finance::now_secs(), code],
            )?;
        }
        if restore {
            // restore：清除手动覆盖，回到目录默认权重。
            conn.execute(
                "DELETE FROM strategy_weight_overrides WHERE code=?1",
                [&code],
            )?;
        } else if let WeightChange::Set(ppm) = weight {
            conn.execute(
                "INSERT INTO strategy_weight_overrides(code,weight_ppm,updated_at) VALUES(?1,?2,?3)
                 ON CONFLICT(code) DO UPDATE SET weight_ppm=excluded.weight_ppm, updated_at=excluded.updated_at",
                params![code, ppm, crate::finance::now_secs()],
            )?;
        }
        let new_revision = bump_revision(conn)?;
        let result = json!({"code": code, "revision": new_revision});
        crate::opportunity_records::remember(conn, v, &result)?;
        Ok(result)
    })
}

/// 候选排序分适配：启用且匹配的策略取最高生效权重（供体按逐策略发射信号行，
/// 本投影单 rank 取 max，为确定性适配）；未匹配或全部停用回退 1.0x。
/// 基础分保持供体 scan() 原值（parity 锚点），rank 为权重层输出。
pub(crate) fn weighted_rank(weights: &Value, tags: &[&str], base_score: i32) -> (i32, Vec<Value>) {
    let mut best = 0i64;
    let mut matched = Vec::new();
    let map = &weights["weights"];
    for tag in tags {
        let entry = &map[*tag];
        if !entry.is_object() {
            continue;
        }
        let enabled = entry["enabled"].as_bool().unwrap_or(false);
        if !enabled {
            continue;
        }
        let ppm = entry["weightPpm"].as_i64().unwrap_or(WEIGHT_SCALE);
        if ppm > best {
            best = ppm;
        }
        matched.push(json!({"code": tag, "weightPpm": ppm}));
    }
    if matched.is_empty() {
        best = WEIGHT_SCALE;
    }
    let rank = ((i64::from(base_score) * best) / WEIGHT_SCALE).clamp(0, 100) as i32;
    (rank, matched)
}

#[cfg(test)]
#[path = "strategies_tests.rs"]
mod tests;
