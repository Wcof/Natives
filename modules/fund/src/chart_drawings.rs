//! 图形领域操作（mode/T04 审计更正，R-M3/R-M4）。
//!
//! 划线业务与存储操作从 api/drawings.rs 收口到本模块：period／toolType
//! 白名单、points/options 有界性是业务规则；HTTP 适配只做 JSON 解析与
//! wire 映射。行读取错误整体传播；DB 错误只暴露稳定 code。

use crate::storage::Store;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const ID_MAX: usize = 64;
const RAW_SYMBOL_MAX: usize = 48;
const SYMBOL_MAX: usize = 32;
const PERIOD_MAX: usize = 16;
const TOOL_TYPE_MAX: usize = 24;
const POINTS_MAX_ITEMS: usize = 512;
const POINTS_MAX_BYTES: usize = 65_536;
const OPTIONS_MAX_BYTES: usize = 4_096;

/// 前端真实保存入口（chart-drawings.js / stock-drawings.js /
/// trends-drawings.js）会发送的 period 集合；超集拒绝。
const PERIODS_ALLOWED: [&str; 5] = ["day", "week", "month", "minute", "7d"];
/// 画线工具白名单 = partials 工具按钮 + 形状渲染器（stock-drawings-shapes.js）
/// 实际渲染的 toolType 全集；"cursor" 是选择模式不是图形，不入库。
const TOOL_TYPES_ALLOWED: [&str; 9] = [
    "trendline",
    "horizontal",
    "channel",
    "polyline",
    "rectangle",
    "circle",
    "fibonacci",
    "measure",
    "text",
];

/// 领域错误（稳定 code；HTTP 状态由适配层映射）。
#[derive(Debug)]
pub(crate) enum DrawingError {
    Invalid(String),
    Db,
    Conflict,
}

impl DrawingError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "APP_PARAM_INVALID",
            Self::Db => "DRAWINGS_DB",
            Self::Conflict => "DRAWINGS_CONFLICT",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Invalid(message) => message.clone(),
            Self::Db => "drawings storage read/write failed".into(),
            Self::Conflict => "drawing changed; reload before retrying".into(),
        }
    }
}

impl From<rusqlite::Error> for DrawingError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Db
    }
}

fn invalid(message: impl Into<String>) -> DrawingError {
    DrawingError::Invalid(message.into())
}

/// 保存划线的已校验输入。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrawingDraft {
    pub id: String,
    pub symbol: String,
    pub period: String,
    pub tool_type: String,
    pub points_json: String,
    pub options_json: String,
    pub expected_digest: Option<String>,
}

impl DrawingDraft {
    pub(crate) fn from_raw(val: &Value) -> Result<Self, DrawingError> {
        let id = match val.get("id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() && id.chars().count() <= ID_MAX => id.to_owned(),
            _ => return Err(invalid(format!("id length must be 1..={ID_MAX}"))),
        };
        let raw_symbol = match val.get("symbol").and_then(|v| v.as_str()) {
            Some(symbol) if !symbol.is_empty() && symbol.chars().count() <= RAW_SYMBOL_MAX => {
                symbol.to_owned()
            }
            _ => {
                return Err(invalid(format!(
                    "symbol length must be 1..={RAW_SYMBOL_MAX}"
                )))
            }
        };
        let symbol = crate::market::normalize_symbol(&raw_symbol);
        if symbol.is_empty() || symbol.chars().count() > SYMBOL_MAX {
            return Err(invalid(format!("symbol length must be 1..={SYMBOL_MAX}")));
        }
        // 缺省与显式 null 走默认（现行契约）；显式非法类型拒绝，不静默回退。
        let period = match val.get("period") {
            None | Some(Value::Null) => "day".to_owned(),
            Some(v) => {
                let text = v
                    .as_str()
                    .ok_or_else(|| invalid("period must be a string"))?;
                if text.is_empty() || text.chars().count() > PERIOD_MAX {
                    return Err(invalid(format!("period length must be 1..={PERIOD_MAX}")));
                }
                text.to_owned()
            }
        };
        if !PERIODS_ALLOWED.contains(&period.as_str()) {
            return Err(invalid("period not supported"));
        }
        let tool_value = val.get("toolType").or_else(|| val.get("tool"));
        let tool_type = match tool_value {
            None | Some(Value::Null) => "trendline".to_owned(),
            Some(v) => {
                let text = v
                    .as_str()
                    .ok_or_else(|| invalid("toolType must be a string"))?;
                if text.is_empty() || text.chars().count() > TOOL_TYPE_MAX {
                    return Err(invalid(format!(
                        "toolType length must be 1..={TOOL_TYPE_MAX}"
                    )));
                }
                text.to_owned()
            }
        };
        if !TOOL_TYPES_ALLOWED.contains(&tool_type.as_str()) {
            return Err(invalid("toolType not supported"));
        }
        let points_json = checked_payload(val, "points")?;
        let options_json = checked_payload(val, "options")?;
        validate_geometry(
            &tool_type,
            val.get("points").unwrap_or(&Value::Null),
            val.get("options").unwrap_or(&Value::Null),
        )?;
        let expected_digest = match val.get("expectedDigest") {
            None | Some(Value::Null) => None,
            Some(value) => Some(checked_digest(value.as_str())?.to_owned()),
        };
        Ok(Self {
            id,
            symbol,
            period,
            tool_type,
            points_json,
            options_json,
            expected_digest,
        })
    }
}

fn checked_payload(val: &Value, key: &str) -> Result<String, DrawingError> {
    let value = match val.get(key) {
        None | Some(Value::Null) => {
            return Ok(if key == "points" {
                "[]".into()
            } else {
                "{}".into()
            });
        }
        Some(value) => value,
    };
    match key {
        "points" => {
            let arr = value
                .as_array()
                .ok_or_else(|| invalid("points must be an array"))?;
            if arr.len() > POINTS_MAX_ITEMS {
                return Err(invalid(format!(
                    "points items must be 1..={POINTS_MAX_ITEMS}"
                )));
            }
        }
        _ => {
            if !value.is_object() {
                return Err(invalid("options must be an object"));
            }
        }
    }
    let text = value.to_string();
    let max = if key == "points" {
        POINTS_MAX_BYTES
    } else {
        OPTIONS_MAX_BYTES
    };
    if text.len() > max {
        return Err(invalid(format!("{key} too large (max {max} bytes)")));
    }
    Ok(text)
}

fn checked_digest(value: Option<&str>) -> Result<&str, DrawingError> {
    match value {
        Some(text)
            if text.len() == 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) =>
        {
            Ok(text)
        }
        _ => Err(invalid("expectedDigest must be a lowercase SHA-256 digest")),
    }
}

fn valid_minute_timestamp(date: &str) -> bool {
    if date.len() != 25 || date.get(10..11) != Some("T") || date.get(19..) != Some("+08:00") {
        return false;
    }
    let Some(clock) = date.get(11..19) else {
        return false;
    };
    let parts: Vec<_> = clock.split(':').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|c| c.is_ascii_digit()))
        && parts[0].parse::<u8>().is_ok_and(|v| v < 24)
        && parts[1].parse::<u8>().is_ok_and(|v| v < 60)
        && parts[2].parse::<u8>().is_ok_and(|v| v < 60)
}

fn validate_geometry(tool: &str, points: &Value, options: &Value) -> Result<(), DrawingError> {
    let points = points
        .as_array()
        .ok_or_else(|| invalid("points required"))?;
    let count = match tool {
        "horizontal" | "text" => 1,
        "channel" => 3,
        _ => 2,
    };
    if (tool == "polyline" && points.len() < 2) || (tool != "polyline" && points.len() != count) {
        return Err(invalid("wrong anchor count for toolType"));
    }
    for point in points {
        let date = point
            .get("date")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("point.date required"))?;
        if !date
            .get(..10)
            .is_some_and(crate::assets::is_valid_calendar_date)
            || !(date.len() == 10 || valid_minute_timestamp(date))
            || date.len() > 64
            || date.chars().any(char::is_control)
            || !point
                .get("price")
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite)
            || point
                .as_object()
                .is_none_or(|p| p.keys().any(|k| k != "date" && k != "price"))
        {
            return Err(invalid(
                "points require business date and finite price, without pixel/index coordinates",
            ));
        }
    }
    if options.get("coordinateVersion").and_then(Value::as_u64) != Some(2) {
        return Err(invalid("coordinateVersion must be 2"));
    }
    let context = &options["viewContext"];
    let kind = context["seriesKind"].as_str().unwrap_or("");
    let fq = context["fq"].as_str().unwrap_or("");
    if !["OHLC", "minute", "NAV"].contains(&kind)
        || (kind == "NAV" && fq != "not-applicable")
        || (kind != "NAV" && !["bf", "qfq", "hfq"].contains(&fq))
    {
        return Err(invalid("viewContext has unsupported seriesKind/fq"));
    }
    for key in ["name", "text"] {
        if let Some(value) = options.get(key) {
            if !value.as_str().is_some_and(|v| {
                v.chars().count() <= 256 && !v.chars().any(|c| c.is_control() && c != '\n')
            }) {
                return Err(invalid(
                    "drawing text must be a string of at most 256 characters",
                ));
            }
        }
    }
    for key in ["visible", "locked"] {
        if options.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(invalid("visible and locked must be boolean"));
        }
    }
    if let Some(value) = options.get("color") {
        if !value.as_str().is_some_and(|c| {
            c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            return Err(invalid("color must be #RRGGBB or absent"));
        }
    }
    if let Some(value) = options.get("width") {
        if !value
            .as_f64()
            .is_some_and(|w| w.is_finite() && (0.5..=10.0).contains(&w))
        {
            return Err(invalid("width must be 0.5..=10"));
        }
    }
    if let Some(value) = options.get("dash") {
        if !value
            .as_str()
            .is_some_and(|d| ["solid", "dashed", "dashed-dense", "dashed-sparse"].contains(&d))
        {
            return Err(invalid("dash not supported"));
        }
    }
    Ok(())
}

// serde_json's default object map is sorted; timestamps are excluded from the content version.
fn content(draft: &DrawingDraft) -> Result<Value, DrawingError> {
    Ok(
        json!({"id": draft.id, "symbol": draft.symbol, "period": draft.period, "toolType": draft.tool_type,
        "points": serde_json::from_str::<Value>(&draft.points_json).map_err(|_| DrawingError::Db)?,
        "options": serde_json::from_str::<Value>(&draft.options_json).map_err(|_| DrawingError::Db)?}),
    )
}
fn projection(draft: &DrawingDraft, updated_at: &str) -> Result<Value, DrawingError> {
    let mut value = content(draft)?;
    let digest = format!("{:x}", Sha256::digest(value.to_string().as_bytes()));
    value["digest"] = json!(digest);
    value["updatedAt"] = json!(updated_at);
    Ok(value)
}
fn read_current(conn: &rusqlite::Connection, id: &str) -> Result<Option<Value>, DrawingError> {
    let row = conn.query_row("SELECT symbol, period, tool_type, points_json, options_json, updated_at FROM chart_drawings WHERE id=?1", [id], |r| {
        Ok((DrawingDraft { id: id.into(), symbol: r.get(0)?, period: r.get(1)?, tool_type: r.get(2)?, points_json: r.get(3)?, options_json: r.get(4)?, expected_digest: None }, r.get::<_, String>(5)?))
    }).optional()?;
    row.map(|(draft, time)| projection(&draft, &time))
        .transpose()
}

pub(crate) fn list(store: &Store, symbol: &str, period: &str) -> Result<String, DrawingError> {
    if !PERIODS_ALLOWED.contains(&period) {
        return Err(invalid("period not supported"));
    }
    store.with_read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM chart_drawings WHERE symbol=?1 AND period=?2 ORDER BY created_at ASC, id ASC")?;
        let ids = stmt.query_map([symbol, period], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        let mut rows = Vec::with_capacity(ids.len());
        for id in ids { rows.push(read_current(conn, &id)?.ok_or(DrawingError::Db)?); }
        Ok(json!({"drawings": rows}).to_string())
    })
}

pub(crate) fn save(store: &Store, draft: &DrawingDraft, now: &str) -> Result<String, DrawingError> {
    store.with_write(|conn| {
        let current = read_current(conn, &draft.id)?;
        if let Some(current) = &current {
            if draft.expected_digest.as_deref() != current["digest"].as_str() ||
                current["symbol"] != draft.symbol || current["period"] != draft.period || current["toolType"] != draft.tool_type {
                return Err(DrawingError::Conflict);
            }
        } else if draft.expected_digest.is_some() { return Err(DrawingError::Conflict); }
        conn.execute("INSERT INTO chart_drawings(id, symbol, period, tool_type, points_json, options_json, created_at, updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?7) ON CONFLICT(id) DO UPDATE SET points_json=excluded.points_json, options_json=excluded.options_json, updated_at=excluded.updated_at",
            rusqlite::params![draft.id, draft.symbol, draft.period, draft.tool_type, draft.points_json, draft.options_json, now])?;
        Ok(json!({"success": true, "drawing": projection(draft, now)?}).to_string())
    })
}

pub(crate) fn delete(
    store: &Store,
    id: &str,
    expected_digest: Option<&str>,
) -> Result<(), DrawingError> {
    let expected = checked_digest(expected_digest)?;
    store.with_write(|conn| {
        let current = read_current(conn, id)?.ok_or(DrawingError::Conflict)?;
        if current["digest"].as_str() != Some(expected) {
            return Err(DrawingError::Conflict);
        }
        conn.execute("DELETE FROM chart_drawings WHERE id=?1", [id])?;
        Ok(())
    })
}
