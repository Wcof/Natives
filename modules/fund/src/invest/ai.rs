//! AI 评估：模块只做「组装输入」与「保存结果」，**不发起推理**。
//!
//! 原因（ADR-0037 / ADR-0038 / ADR-0044）：模型凭据只存在于 Keychain，
//! 推理由 Extension 侧 broker 发起。模块拿不到 Key，也不应该自己发 HTTP。
//!
//! 因此拆成三步，前端一次点击即可串起来，用户无感：
//! 1. `GET  /api/v2/ai/context`      → 模块把算好的指标组装成输入
//! 2. （前端调 broker 推理，本模块不参与）
//! 3. `POST /api/v2/ai/evaluations`  → 回存结构化结果与输入快照
//!
//! 与格物的一致之处：把指标算好再喂给模型、结构化校验、存 `snapshot_json`。
//! 不同之处：不存 baseUrl/apiKey——那是被本仓库 ADR 明确禁止的形态。

use super::flows::list_flows;
use super::holdings::list_holdings;
use super::model::FLOW_TYPES;
use crate::finance::{now_secs, FinanceError};
use crate::storage::Store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 评分语气。只用三档，避免模型输出「强烈建议」这类越界表述。
pub const TONES: [&str; 3] = ["steady", "watch", "alert"];
/// 评估风格，写入 prompt 措辞，不影响任何数据。
pub const STYLES: [&str; 3] = ["conservative", "neutral", "aggressive"];

#[derive(Debug, Clone, Serialize)]
pub struct AiSettingsView {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub style: String,
    pub show_on_overview: bool,
}

#[derive(Debug, Deserialize)]
pub struct SaveSettings {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub style: Option<String>,
    pub show_on_overview: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationView {
    pub id: i64,
    pub account_id: i64,
    pub instrument_id: i64,
    pub provider_id: String,
    pub model_id: String,
    pub score: i64,
    pub tone: String,
    pub label: String,
    pub summary: String,
    pub concerns: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveEvaluation {
    pub account_id: i64,
    pub instrument_id: i64,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model_id: String,
    pub score: i64,
    pub tone: String,
    pub label: String,
    pub summary: String,
    #[serde(default)]
    pub concerns: Vec<String>,
    /// 模型实际看到的输入，用于日后回溯「当时为什么这么说」。
    pub snapshot_json: String,
}

pub fn read_settings(store: &Store) -> Result<AiSettingsView, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT provider_id, model_id, style, show_on_overview
             FROM ai_settings WHERE scope='default'",
        )?;
        let mut rows = stmt.query_map([], |r| {
            Ok(AiSettingsView {
                provider_id: r.get(0)?,
                model_id: r.get(1)?,
                style: r
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "neutral".into()),
                show_on_overview: r.get::<_, i64>(3)? != 0,
            })
        })?;
        match rows.next() {
            Some(v) => Ok(v?),
            None => Ok(AiSettingsView {
                provider_id: None,
                model_id: None,
                style: "neutral".to_string(),
                show_on_overview: true,
            }),
        }
    })
}

pub fn save_settings(
    store: &Store,
    payload: &SaveSettings,
) -> Result<AiSettingsView, FinanceError> {
    if let Some(style) = &payload.style {
        if !STYLES.contains(&style.as_str()) {
            return Err(FinanceError::Invalid(format!(
                "评估风格非法：{style}（允许 {}）",
                STYLES.join(" / ")
            )));
        }
    }
    let ts = now_secs();
    let current = read_settings(store)?;
    let style = payload.style.clone().unwrap_or(current.style);
    let show = payload.show_on_overview.unwrap_or(current.show_on_overview);
    let provider = payload.provider_id.clone().or(current.provider_id);
    let model = payload.model_id.clone().or(current.model_id);
    store.with_write(|conn| {
        conn.execute(
            "INSERT INTO ai_settings(scope, provider_id, model_id, style, show_on_overview, updated_at)
             VALUES('default',?1,?2,?3,?4,?5)
             ON CONFLICT(scope) DO UPDATE SET
               provider_id=excluded.provider_id,
               model_id=excluded.model_id,
               style=excluded.style,
               show_on_overview=excluded.show_on_overview,
               updated_at=excluded.updated_at",
            rusqlite::params![provider, model, style, show as i64, ts],
        )?;
        Ok::<(), FinanceError>(())
    })?;
    read_settings(store)
}

/// 组装喂给模型的输入。
///
/// 原则：**只给算好的结果，不给原始流水让模型自己算**。
/// 模型做算术必然出错，而一旦数字错了，后面所有判断都无意义。
pub fn build_context(
    store: &Store,
    account_id: i64,
    instrument_id: i64,
) -> Result<Value, FinanceError> {
    let holdings = list_holdings(store)?;
    let holding = holdings
        .iter()
        .find(|h| h.account_id == account_id && h.instrument_id == instrument_id)
        .ok_or_else(|| {
            FinanceError::NotFound(format!(
                "账户 {account_id} 下没有标的 {instrument_id} 的持仓"
            ))
        })?;
    let flows = list_flows(store, Some(account_id), Some(instrument_id))?;
    let recent: Vec<Value> = flows
        .iter()
        .take(12)
        .map(|f| {
            json!({
                "type": f.kind,
                "date": f.trade_date,
                "quantity": f.quantity,
                "price": f.price,
                "amount": f.amount,
                "fee": f.fee,
            })
        })
        .collect();
    let settings = read_settings(store)?;

    Ok(json!({
        "style": settings.style,
        "holding": {
            "code": holding.code,
            "name": holding.name,
            "asset_class": holding.asset_class,
            "market": holding.market,
            "currency": holding.currency,
            "account": holding.account_name,
            "share_class": holding.share_class,
            "group_code": holding.group_code,
        },
        "position": {
            "quantity": holding.quantity,
            "avg_cost": holding.avg_cost,
            "cost": holding.cost,
            "price": holding.price,
            "market_value": holding.market_value,
            "unrealized": holding.unrealized,
            "unrealized_ratio": holding.unrealized_ratio,
            "realized": holding.realized,
            "held_days": holding.held_days,
            "daily_gain": holding.daily_gain,
            "price_stale": holding.price_stale,
        },
        "flows": recent,
        "available_types": FLOW_TYPES,
    }))
}

pub fn save_evaluation(
    store: &Store,
    payload: &SaveEvaluation,
) -> Result<EvaluationView, FinanceError> {
    if !(0..=100).contains(&payload.score) {
        return Err(FinanceError::Invalid("评分需在 0-100 之间".into()));
    }
    if !TONES.contains(&payload.tone.as_str()) {
        return Err(FinanceError::Invalid(format!(
            "语气非法：{}（允许 {}）",
            payload.tone,
            TONES.join(" / ")
        )));
    }
    let label = payload.label.trim();
    let summary = payload.summary.trim();
    if label.is_empty() || summary.is_empty() {
        return Err(FinanceError::Invalid("定调与摘要不能为空".into()));
    }
    if payload.snapshot_json.trim().is_empty() {
        return Err(FinanceError::Invalid("必须附带输入快照".into()));
    }
    let concerns = serde_json::to_string(&payload.concerns)
        .map_err(|e| FinanceError::Db(format!("concers 序列化失败：{e}")))?;
    let ts = now_secs();
    store.with_write(|conn| {
        conn.execute(
            "INSERT INTO ai_evaluations(account_id, instrument_id, provider_id, model_id,
                                        prompt_version, score, tone, label, summary,
                                        concerns_json, snapshot_json, created_at)
             VALUES(?1,?2,?3,?4,'v1',?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                payload.account_id,
                payload.instrument_id,
                payload.provider_id.trim(),
                payload.model_id.trim(),
                payload.score,
                payload.tone,
                label,
                summary,
                concerns,
                payload.snapshot_json,
                ts
            ],
        )?;
        Ok(EvaluationView {
            id: conn.last_insert_rowid(),
            account_id: payload.account_id,
            instrument_id: payload.instrument_id,
            provider_id: payload.provider_id.trim().to_string(),
            model_id: payload.model_id.trim().to_string(),
            score: payload.score,
            tone: payload.tone.clone(),
            label: label.to_string(),
            summary: summary.to_string(),
            concerns: payload.concerns.clone(),
            created_at: ts,
        })
    })
}

pub fn list_evaluations(
    store: &Store,
    account_id: Option<i64>,
    instrument_id: Option<i64>,
) -> Result<Vec<EvaluationView>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, instrument_id, provider_id, model_id, score, tone,
                    label, summary, concerns_json, created_at
             FROM ai_evaluations
             WHERE (?1 IS NULL OR account_id = ?1)
               AND (?2 IS NULL OR instrument_id = ?2)
             ORDER BY created_at DESC, id DESC
             LIMIT 100",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, instrument_id], |r| {
                let concerns_raw: String = r.get(9)?;
                let concerns: Vec<String> = serde_json::from_str(&concerns_raw).unwrap_or_default();
                Ok(EvaluationView {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    instrument_id: r.get(2)?,
                    provider_id: r.get(3)?,
                    model_id: r.get(4)?,
                    score: r.get(5)?,
                    tone: r.get(6)?,
                    label: r.get(7)?,
                    summary: r.get(8)?,
                    concerns,
                    created_at: r.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// 趋势点：只保留画图需要的字段，不返回 snapshot_json（可能很大）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvaluationPoint {
    pub date: String,
    pub score: i64,
    pub tone: String,
    pub label: String,
}

/// 某持仓的评估历史（时间升序，便于直接画折线）。
///
/// 与 `list_evaluations` 的区别：那个是倒序的完整记录，这个是升序的轻量序列。
pub fn trend(
    store: &Store,
    account_id: i64,
    instrument_id: i64,
    limit: i64,
) -> Result<Vec<EvaluationPoint>, FinanceError> {
    let limit = limit.clamp(1, 200);
    let rows = store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT score, tone, label, created_at
             FROM ai_evaluations
             WHERE account_id=?1 AND instrument_id=?2
             ORDER BY created_at DESC, id DESC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, instrument_id, limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, FinanceError>(rows)
    })?;

    // created_at 是 Unix 秒；转成 YYYY-MM-DD 供 UI 展示
    Ok(rows
        .into_iter()
        .rev()
        .map(|(score, tone, label, created_at)| {
            let days = created_at
                .parse::<i64>()
                .map(|secs| secs / 86_400)
                .unwrap_or(0);
            let date = super::backfill::ordinal_to_ymd(days + 2_440_588);
            EvaluationPoint {
                date,
                score,
                tone,
                label,
            }
        })
        .collect())
}
