//! assets 模块：已记录投资资产（P1，方案 01 F2 / 05 §3-§4）。
//!
//! - 人工资产是用户声明的**人民币总估值快照**：数量/单价不存，不随交易或
//!   行情自动更新；新增基金买卖不会自动增减现金记录。
//! - 股票与 ETF 共用证券识别键 `security:<market>:<code>`（首版市场
//!   SSE/SZSE/BSE，代码 6 位数字）；现金账户内固定键 `cash`；黄金
//!   `gold:<归一化名称>`。(account_id, instrument_key) 唯一。
//! - 旧基金持仓继续由流水/期初回放，绝不复制为第二份持仓；同账户同代码
//!   只能提示**疑似重复**（旧 funds.code 无市场维度），用户确认后才处理。
//! - 旧基金净值币种默认 unknown：仅用户确认为 CNY 的基金才纳入人民币现值；
//!   净值统一从 fund_nav 取「不晚于查询日、净值为正」的最新行。
//! - 所有计算复用 fixed 定点数与同一读快照；金额溢出显式报错，不截断。

use crate::fixed::{Fixed, SCALE_SHARE};
use crate::ledger;
use crate::portfolio;
use crate::storage::Store;
pub use crate::valuation::{
    apply_quotes, collect, finalize, quote_symbols, store_valuation_cache,
    summary_with_cached_quotes, valuation_batch, Collected, QuoteInput,
};
use rusqlite::{params, Connection, OptionalExtension};

pub const PRODUCT_TYPES: &[&str] = &["cash", "stock", "etf", "gold"];
pub const RISK_CLASSES: &[&str] = &["equity", "gold", "fixed_income", "cash", "other", "unknown"];
pub const NAV_CURRENCIES: &[&str] = &["unknown", "CNY", "unsupported"];
pub const MARKETS: &[&str] = &["SSE", "SZSE", "BSE"];
/// 估值超过该自然日数标记「较旧」（方案 05 §4 P1，产品阈值）。
pub const STALE_DAYS: i64 = 7;

#[derive(Debug, PartialEq, Eq)]
pub enum AssetError {
    NotFound(String),
    /// (account, instrument_key) 已存在。
    Duplicate(String),
    /// 同账户在持旧基金同代码，且未确认是不同市场/资产。
    OverlapReview {
        code: String,
    },
    Invalid(String),
    Db(String),
    /// 合计超过 i64 分位表示范围。截断会得到错误金额，必须显式报错
    /// （方案 03 验收：金额溢出 → 不给出错误合计）。
    Overflow(String),
}

impl From<rusqlite::Error> for AssetError {
    fn from(e: rusqlite::Error) -> Self {
        AssetError::Db(e.to_string())
    }
}

impl AssetError {
    pub fn code(&self) -> &'static str {
        match self {
            AssetError::NotFound(_) => "ASSET_NOT_FOUND",
            AssetError::Duplicate(_) => "ASSET_DUPLICATE",
            AssetError::OverlapReview { .. } => "ASSET_OVERLAP_REVIEW",
            AssetError::Invalid(_) => "ASSET_INVALID",
            AssetError::Db(_) => "ASSET_DB",
            AssetError::Overflow(_) => "ASSET_AMOUNT_OVERFLOW",
        }
    }

    pub fn message(&self) -> String {
        match self {
            AssetError::NotFound(m) => format!("记录不存在：{m}"),
            AssetError::Duplicate(m) => format!("该账户已有此项，请编辑已有记录：{m}"),
            AssetError::OverlapReview { code } => {
                format!("同账户存在代码 {code} 的在持基金：请确认是否为不同资产")
            }
            AssetError::Invalid(m) => format!("输入无效：{m}"),
            AssetError::Db(e) => format!("数据库错误：{e}"),
            AssetError::Overflow(m) => format!("金额超出可表示范围，已停止合计：{m}"),
        }
    }
}

/// 分位合计累加：溢出即报错，不回退到饱和值（饱和值会成为错误金额）。
pub(crate) fn add_minor(current: i64, delta: i64, what: &str) -> Result<i64, AssetError> {
    current
        .checked_add(delta)
        .ok_or_else(|| AssetError::Overflow(format!("{what}：{current} + {delta}")))
}

// ---------------------------------------------------------------------------
// 日期工具（北京时间口径，与 api::positions::today_iso 一致；无外部依赖）
// ---------------------------------------------------------------------------

/// days-from-civil（Howard Hinnant 算法）。
pub(crate) fn days_from_civil(date: &str) -> Option<i64> {
    if date.len() != 10 || date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' {
        return None;
    }
    let y: i64 = date[0..4].parse().ok()?;
    let m: i64 = date[5..7].parse().ok()?;
    let d: i64 = date[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// 真实日历日期校验（YYYY-MM-DD，含月内天数）。
pub(crate) fn is_valid_calendar_date(date: &str) -> bool {
    let Some(days) = days_from_civil(date) else {
        return false;
    };
    // 反向换算必须一致，排除 02-30 之类的组合。
    civil_from_days(days) == date
}

pub(crate) fn civil_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 本地业务日期 YYYY-MM-DD（UTC+8 北京口径；本地记账以日为粒度无外部依赖）。
pub(crate) fn today_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    civil_from_days((secs / 86400) as i64)
}

/// today 往前推 days 个日历日；today 须为内部受控的 YYYY-MM-DD 输入。
pub(crate) fn minus_days(today: &str, days: i64) -> String {
    civil_from_days(days_from_civil(today).unwrap_or_default() - days)
}

pub(crate) fn is_stale(valued_at: &str, today: &str) -> bool {
    match (days_from_civil(valued_at), days_from_civil(today)) {
        (Some(v), Some(t)) => t - v > STALE_DAYS,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 人工资产录入
// ---------------------------------------------------------------------------

/// 表单输入（字段由 API 层从 JSON 提取；instrument_key 由 Fund 生成，
/// 不信任页面传来的键）。
pub struct ManualAssetInput {
    pub account_name: String,
    pub product_type: String,
    /// 股票/ETF：市场（SSE|SZSE|BSE）与 6 位代码。
    pub market: Option<String>,
    pub code: Option<String>,
    pub name: String,
    pub risk_class: String,
    /// 人民币总估值（十进制字符串，≤2 位小数，可带千分位）。
    pub market_value_text: String,
    pub valued_at: String,
    /// 仅当用户明确确认与同代码在持基金是不同资产时为 true。
    pub confirm_distinct: bool,
}

fn normalize_name(name: &str) -> Result<String, AssetError> {
    let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return Err(AssetError::Invalid("名称不能为空".into()));
    }
    if trimmed.chars().count() > 64 {
        return Err(AssetError::Invalid("名称过长（上限 64 字符）".into()));
    }
    Ok(trimmed.to_string())
}

fn parse_market_value(text: &str) -> Result<i64, AssetError> {
    let cleaned: String = text.chars().filter(|c| *c != ',').collect();
    let fixed = Fixed::parse(cleaned.trim(), 2)
        .map_err(|_| AssetError::Invalid("金额最多两位小数".into()))?;
    if fixed.is_negative() {
        return Err(AssetError::Invalid("金额不能为负".into()));
    }
    Ok(fixed.raw())
}

/// 由表单字段生成识别键（方案 05 §3：不直接信任页面传来的键）。
pub(crate) fn build_instrument_key(
    product_type: &str,
    market: Option<&str>,
    code: Option<&str>,
    name: &str,
) -> Result<String, AssetError> {
    match product_type {
        "cash" => Ok("cash".to_string()),
        "stock" | "etf" => {
            let market = market
                .map(|m| m.trim().to_uppercase())
                .ok_or_else(|| AssetError::Invalid("请选择市场".into()))?;
            if !MARKETS.contains(&market.as_str()) {
                return Err(AssetError::Invalid(format!(
                    "市场仅支持 {}（人民币市场）",
                    MARKETS.join("/")
                )));
            }
            let code = code.map(|c| c.trim()).unwrap_or("");
            if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(AssetError::Invalid("证券代码必须为 6 位数字".into()));
            }
            Ok(format!("security:{market}:{code}"))
        }
        "gold" => {
            let name = normalize_name(name)?;
            Ok(format!("gold:{}", name.to_lowercase()))
        }
        other => Err(AssetError::Invalid(format!("未知产品类型 {other}"))),
    }
}

fn validate_risk_class(risk_class: &str) -> Result<(), AssetError> {
    if !RISK_CLASSES.contains(&risk_class) {
        return Err(AssetError::Invalid(format!("未知风险类别 {risk_class}")));
    }
    Ok(())
}

fn validate_valued_at(valued_at: &str, today: &str) -> Result<(), AssetError> {
    if !is_valid_calendar_date(valued_at) {
        return Err(AssetError::Invalid("估值日期不是有效日历日期".into()));
    }
    if valued_at > today {
        return Err(AssetError::Invalid("估值日期不能晚于今天".into()));
    }
    Ok(())
}

/// 同账户在持旧基金中是否存在相同 6 位代码（清仓记录不触发）。
pub(crate) fn find_legacy_overlap(
    conn: &Connection,
    account_id: i64,
    code: &str,
) -> rusqlite::Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM positions p
             JOIN funds f ON f.id = p.fund_id
             WHERE p.account_id = ?1 AND p.quantity_raw > 0 AND f.code = ?2
             LIMIT 1",
            params![account_id, code],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

fn validate_overlap(
    conn: &Connection,
    account_id: i64,
    product_type: &str,
    instrument_key: &str,
    confirm_distinct: bool,
) -> Result<(), AssetError> {
    if product_type != "stock" && product_type != "etf" {
        return Ok(());
    }
    let Some(code) = instrument_key.rsplit(':').next() else {
        return Ok(());
    };
    if find_legacy_overlap(conn, account_id, code)? && !confirm_distinct {
        return Err(AssetError::OverlapReview {
            code: code.to_string(),
        });
    }
    Ok(())
}

/// 创建人工资产记录（写事务内调用；输入先做完整校验）。
pub fn create_manual_asset(
    conn: &Connection,
    input: &ManualAssetInput,
    today: &str,
) -> Result<i64, AssetError> {
    if !PRODUCT_TYPES.contains(&input.product_type.as_str()) {
        return Err(AssetError::Invalid(format!(
            "未知产品类型 {}",
            input.product_type
        )));
    }
    validate_risk_class(&input.risk_class)?;
    let name = normalize_name(&input.name)?;
    let instrument_key = build_instrument_key(
        &input.product_type,
        input.market.as_deref(),
        input.code.as_deref(),
        &input.name,
    )?;
    let value_minor = parse_market_value(&input.market_value_text)?;
    validate_valued_at(&input.valued_at, today)?;
    let account_id: i64 = conn
        .query_row(
            "SELECT id FROM accounts WHERE name = ?1",
            params![input.account_name.trim()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AssetError::NotFound(format!("账户 {}", input.account_name)))?;
    validate_overlap(
        conn,
        account_id,
        &input.product_type,
        &instrument_key,
        input.confirm_distinct,
    )?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO manual_assets(account_id, product_type, instrument_key, name, risk_class,
                                   market_value_minor, valued_at, legacy_overlap_distinct, created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
        params![
            account_id,
            input.product_type,
            instrument_key,
            name,
            input.risk_class,
            value_minor,
            input.valued_at,
            i64::from(input.confirm_distinct && (input.product_type == "stock" || input.product_type == "etf")),
            now,
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AssetError::Duplicate(input.instrument_key_placeholder())
        } else {
            AssetError::Db(e.to_string())
        }
    })?;
    Ok(conn.last_insert_rowid())
}

impl ManualAssetInput {
    fn instrument_key_placeholder(&self) -> String {
        build_instrument_key(
            &self.product_type,
            self.market.as_deref(),
            self.code.as_deref(),
            &self.name,
        )
        .unwrap_or_default()
    }
}

/// 修改人工资产。账户/类型/识别键变化时重置 legacy_overlap_distinct（方案 05 §3）。
/// 更新估值应更新日期，不能保留旧日假装今日值（日期由本输入携带）。
pub fn update_manual_asset(
    conn: &Connection,
    id: i64,
    input: &ManualAssetInput,
    today: &str,
) -> Result<(), AssetError> {
    let existing: Option<(i64, String, String, String)> = conn
        .query_row(
            "SELECT account_id, product_type, instrument_key, name FROM manual_assets WHERE id = ?1 AND deleted_at IS NULL AND converted_ref IS NULL",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((old_account, old_type, old_key, _old_name)) = existing else {
        return Err(AssetError::NotFound(format!("id {id}")));
    };
    if !PRODUCT_TYPES.contains(&input.product_type.as_str()) {
        return Err(AssetError::Invalid(format!(
            "未知产品类型 {}",
            input.product_type
        )));
    }
    validate_risk_class(&input.risk_class)?;
    let name = normalize_name(&input.name)?;
    let instrument_key = build_instrument_key(
        &input.product_type,
        input.market.as_deref(),
        input.code.as_deref(),
        &input.name,
    )?;
    let value_minor = parse_market_value(&input.market_value_text)?;
    validate_valued_at(&input.valued_at, today)?;
    let account_id: i64 = conn
        .query_row(
            "SELECT id FROM accounts WHERE name = ?1",
            params![input.account_name.trim()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AssetError::NotFound(format!("账户 {}", input.account_name)))?;

    let identity_changed =
        old_account != account_id || old_type != input.product_type || old_key != instrument_key;
    let distinct = if identity_changed {
        // 身份变化后旧确认失效，需要重新评估/确认。
        validate_overlap(
            conn,
            account_id,
            &input.product_type,
            &instrument_key,
            input.confirm_distinct,
        )?;
        i64::from(
            input.confirm_distinct
                && (input.product_type == "stock" || input.product_type == "etf"),
        )
    } else {
        // 保留既有确认状态。
        let flag: i64 = conn.query_row(
            "SELECT legacy_overlap_distinct FROM manual_assets WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        flag
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let updated = conn
        .execute(
            "UPDATE manual_assets SET account_id=?2, product_type=?3, instrument_key=?4, name=?5,
                    risk_class=?6, market_value_minor=?7, valued_at=?8, legacy_overlap_distinct=?9, updated_at=?10
             WHERE id=?1",
            params![
                id,
                account_id,
                input.product_type,
                instrument_key,
                name,
                input.risk_class,
                value_minor,
                input.valued_at,
                distinct,
                now,
            ],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AssetError::Duplicate(input.instrument_key_placeholder())
            } else {
                AssetError::Db(e.to_string())
            }
        })?;
    if updated == 0 {
        return Err(AssetError::NotFound(format!("id {id}")));
    }
    Ok(())
}

/// 删除单条人工资产（调用方负责危险操作确认；本函数只做删除）。
pub fn delete_manual_asset(conn: &Connection, id: i64) -> Result<(), AssetError> {
    let deleted = conn.execute("DELETE FROM manual_assets WHERE id = ?1", params![id])?;
    if deleted == 0 {
        return Err(AssetError::NotFound(format!("id {id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 基金补充信息（币种确认 / 主类别；来源固定为用户确认）
// ---------------------------------------------------------------------------

pub fn set_fund_metadata(
    conn: &Connection,
    fund_id: i64,
    nav_currency: &str,
    risk_class: &str,
) -> Result<(), AssetError> {
    if !NAV_CURRENCIES.contains(&nav_currency) {
        return Err(AssetError::Invalid(format!("未知币种标记 {nav_currency}")));
    }
    validate_risk_class(risk_class)?;
    let exists: Option<i64> = conn
        .query_row("SELECT 1 FROM funds WHERE id = ?1", params![fund_id], |r| {
            r.get(0)
        })
        .optional()?;
    if exists.is_none() {
        return Err(AssetError::NotFound(format!("基金 id {fund_id}")));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO fund_asset_metadata(fund_id, nav_currency, risk_class, updated_at)
         VALUES(?1,?2,?3,?4)
         ON CONFLICT(fund_id) DO UPDATE SET nav_currency=excluded.nav_currency,
                                            risk_class=excluded.risk_class,
                                            updated_at=excluded.updated_at",
        params![fund_id, nav_currency, risk_class, now],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 已记录投资资产聚合（单一权威；卡片与详情共用）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundValuationRow {
    pub account_id: i64,
    pub account_name: String,
    pub fund_id: i64,
    pub fund_code: String,
    pub fund_name: String,
    pub quantity_raw: i64,
    pub cost_raw: i64,
    pub cost_unknown: bool,
    /// 最近一次持仓变动日期（不是估值日期）。
    pub holding_as_of: String,
    pub nav_raw: Option<i64>,
    pub nav_date: Option<String>,
    pub nav_source: Option<String>,
    pub nav_currency: String,
    pub risk_class: String,
    /// 人民币现值（分位 raw）；缺净值/币种未确认或不支持时为 None。
    pub value_minor: Option<i64>,
    /// 无现值的原因：missing_nav | missing_quote | overflow | currency_unknown | currency_unsupported。
    pub value_gap: Option<String>,
    pub stale: bool,
    /// N04 统一估值：标的类别与市场（来自 instrument_parts）。
    pub instrument_kind: String,
    pub market: String,
    /// 有效价格来源：nav | quote | none；price_raw 为 4 位定点（净值或行情）。
    pub value_source: String,
    pub price_raw: Option<i64>,
    /// 价格业务时间：净值日期或行情 asOf。
    pub price_as_of: Option<String>,
    pub day_change_raw: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualAssetRow {
    pub id: i64,
    pub account_id: i64,
    pub account_name: String,
    pub product_type: String,
    pub instrument_key: String,
    pub name: String,
    pub risk_class: String,
    pub market_value_minor: i64,
    pub valued_at: String,
    pub stale: bool,
    /// 与同账户同代码在持基金疑似重复且未确认：暂不计入金额。
    pub overlap_review: bool,
    pub legacy_overlap_distinct: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetsSummary {
    pub funds: Vec<FundValuationRow>,
    pub manual: Vec<ManualAssetRow>,
    pub priced_count: usize,
    pub missing_price_count: usize,
    pub currency_unconfirmed_count: usize,
    pub currency_unsupported_count: usize,
    pub old_valuation_count: usize,
    pub overlap_review_count: usize,
    pub unknown_class_count: usize,
    /// 基金可计价部分合计（分位 raw）。
    pub fund_value_minor: Option<i64>,
    /// 人工资产合计（排除疑似重复未确认项；分位 raw）。
    pub manual_value_minor: Option<i64>,
    /// 线下投资资产合计（未删除，含退出后余值；分位 raw）。
    pub offline_value_minor: Option<i64>,
    /// 线下资产条目数（含已退出余值行）。
    pub offline_count: usize,
    /// 线下退出未到账应收合计（已确认债权；计入 A 但标记覆盖不完整）。
    pub offline_receivable_minor: Option<i64>,
    /// 已计价部分合计（分位 raw）。
    pub known_value_minor: Option<i64>,
    /// 全量完整时的人民币合计（分位 raw）。
    pub complete_value_minor: Option<i64>,
    pub earliest_valuation: Option<String>,
    pub latest_valuation: Option<String>,
    pub quality: &'static str,
    pub has_entries: bool,
    // ---- N04 统一估值 ----
    /// 场内在持缺报价项数（knownA 覆盖缺口之一）。
    pub missing_quote_count: usize,
    /// 成本未知在持项数（不阻断市值，仅阻断收益）。
    pub unknown_cost_count: usize,
    /// 已计价场内在持项数（quote 或 nav）。
    pub quote_count: usize,
    /// 持仓浮动收益（市值−剩余成本的可计算部分合计）。
    pub floating_minor: Option<i64>,
    /// 市值可算但成本未知、浮动收益缺失的项数。
    pub floating_unknown_cost_count: usize,
    /// 估值批次身份：账本 revision＋各标的价格来源/业务时间/实际值版本。
    pub valuation_batch: String,
    /// 行情批次获取时刻（epoch 秒）；纯净值汇总为 None。
    pub valuation_fetched_at: Option<i64>,
    /// 估值为历史缓存（非本次获取）。
    pub valuation_stale: bool,
}

/// 已记录投资资产聚合（纯账本口径；场内持仓不含行情，N04 消费方应使用
/// `summary_with_cached_quotes` 或锁外取价后的 `apply_quotes`＋`finalize`）。
pub fn assets_summary(conn: &Connection, today: &str) -> Result<AssetsSummary, AssetError> {
    finalize(collect(conn, today)?, today)
}

pub(crate) fn fund_valuation_row(
    conn: &Connection,
    position: &portfolio::Position,
    today: &str,
) -> Result<FundValuationRow, AssetError> {
    let (account_name, code, fund_name): (String, String, String) = conn.query_row(
        "SELECT a.name, f.code, f.name FROM accounts a, funds f WHERE a.id = ?1 AND f.id = ?2",
        params![position.account_id, position.fund_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let meta: Option<(String, String)> = conn
        .query_row(
            "SELECT nav_currency, risk_class FROM fund_asset_metadata WHERE fund_id = ?1",
            params![position.fund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (nav_currency, risk_class) = meta.unwrap_or_else(|| ("unknown".into(), "unknown".into()));

    // 净值统一查 fund_nav：不晚于查询日、净值为正的最新行（方案 05 §2）。
    // N04：场内股票/ETF 的有效价格来自行情；fund_nav 仅用户确认估值可作为
    // 无行情时的回退来源（保留来源与日期，行情恢复后由 apply_quotes 覆盖）。
    let nav: Option<(String, i64, String)> = conn
        .query_row(
            "SELECT nav_date, unit_nav_raw, source FROM fund_nav
             WHERE fund_id = ?1 AND nav_date <= ?2 AND (unit_nav_raw > 0 OR (unit_nav_raw=0 AND source='user_confirmed'))
             ORDER BY nav_date DESC LIMIT 1",
            params![position.fund_id, today],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let nav = nav.filter(|(_, _, source)| {
        let Ok((kind, _, _)) = ledger::instrument_parts(&code) else {
            return true;
        };
        kind == "fund" || source == "user_confirmed"
    });

    let mut value_minor = None;
    let mut value_gap = None;
    let mut stale = false;
    if let Some((nav_date, unit_nav_raw, _source)) = &nav {
        stale = is_stale(nav_date, today);
        match nav_currency.as_str() {
            "CNY" => {
                // 份额(4位) × 净值(4位) → 人民币分(2位)：除以 10^6 向零截断。
                let product = (position.quantity_raw as i128) * (*unit_nav_raw as i128);
                let minor = product / 1_000_000;
                value_minor = Some(i64::try_from(minor).map_err(|_| {
                    AssetError::Overflow(format!("单个持仓估值超出可表示范围：{minor}"))
                })?);
            }
            "unknown" => value_gap = Some("currency_unknown".into()),
            _ => value_gap = Some("currency_unsupported".into()),
        }
    } else {
        value_gap = Some("missing_nav".into());
    }

    let (instrument_kind, market, _) =
        crate::ledger::instrument_parts(&code).unwrap_or(("fund", "CN", code.as_str()));
    Ok(FundValuationRow {
        account_id: position.account_id,
        account_name,
        fund_id: position.fund_id,
        value_source: if nav.is_some() { "nav" } else { "none" }.to_string(),
        price_raw: nav.as_ref().map(|(_, raw, _)| *raw),
        price_as_of: nav.as_ref().map(|(date, _, _)| date.clone()).clone(),
        day_change_raw: None,
        instrument_kind: instrument_kind.to_string(),
        market: market.to_string(),
        fund_code: code,
        fund_name,
        quantity_raw: position.quantity_raw,
        cost_raw: position.cost_raw,
        cost_unknown: position.cost_unknown,
        holding_as_of: position.as_of.clone(),
        nav_raw: nav.as_ref().map(|(_, raw, _)| *raw),
        nav_date: nav.as_ref().map(|(date, _, _)| date.clone()),
        nav_source: nav.as_ref().map(|(_, _, src)| src.clone()),
        nav_currency,
        risk_class,
        value_minor,
        value_gap,
        stale,
    })
}

pub(crate) fn touch_valuation_dates(summary: &mut AssetsSummary, date: Option<&str>) {
    let Some(date) = date else { return };
    if date.is_empty() {
        return;
    }
    if summary.earliest_valuation.is_none() || Some(date) < summary.earliest_valuation.as_deref() {
        summary.earliest_valuation = Some(date.to_string());
    }
    if summary.latest_valuation.is_none() || Some(date) > summary.latest_valuation.as_deref() {
        summary.latest_valuation = Some(date.to_string());
    }
}

pub(crate) fn finalize_summary(summary: &mut AssetsSummary) -> Result<(), AssetError> {
    summary.has_entries =
        !summary.funds.is_empty() || !summary.manual.is_empty() || summary.offline_count > 0;
    if !summary.has_entries {
        summary.quality = "empty";
        return Ok(());
    }
    let mut has_gaps = summary.missing_price_count > 0
        || summary.missing_quote_count > 0
        || summary.currency_unconfirmed_count > 0
        || summary.currency_unsupported_count > 0
        || summary.overlap_review_count > 0;
    if summary.fund_value_minor.is_some()
        || summary.manual_value_minor.is_some()
        || summary.offline_value_minor.is_some()
    {
        summary.known_value_minor = Some(add_minor(
            summary.fund_value_minor.unwrap_or(0),
            summary.manual_value_minor.unwrap_or(0),
            "已计价合计",
        )?);
        summary.known_value_minor = Some(add_minor(
            summary.known_value_minor.unwrap_or(0),
            summary.offline_value_minor.unwrap_or(0),
            "已计价合计",
        )?);
    }
    // 线下退出未到账应收：已确认债权计入 A 但目标/分配标记覆盖不完整（方案 03 §2）。
    if summary.offline_receivable_minor.is_some() {
        summary.known_value_minor = Some(add_minor(
            summary.known_value_minor.unwrap_or(0),
            summary.offline_receivable_minor.unwrap_or(0),
            "已计价合计",
        )?);
    }
    has_gaps = has_gaps || summary.offline_receivable_minor.is_some();
    if has_gaps {
        summary.quality = "incomplete";
    } else if summary.old_valuation_count > 0 {
        summary.quality = "stale";
    } else {
        summary.quality = "ready";
    }
    if !has_gaps {
        // 完整性成立才给完整合计；stale 只影响可用性不影响数值完整性。
        summary.complete_value_minor = summary.known_value_minor;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Store 便捷封装（业务 API 使用；写事务由 with_write 保证）
// ---------------------------------------------------------------------------

pub type StoreResult<T> = Result<T, AssetError>;

pub fn create_asset(store: &Store, input: &ManualAssetInput, today: &str) -> StoreResult<i64> {
    store.with_write(|conn| {
        if input.product_type == "cash"
            && !conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE name=?1)",
                [input.account_name.trim()],
                |r| r.get::<_, bool>(0),
            )?
        {
            crate::ledger::ensure_account(conn, &input.account_name)
                .map_err(|e| AssetError::Invalid(e.message()))?;
        }
        create_manual_asset(conn, input, today)
    })
}

pub fn update_asset(
    store: &Store,
    id: i64,
    input: &ManualAssetInput,
    today: &str,
) -> StoreResult<()> {
    store.with_write(|conn| update_manual_asset(conn, id, input, today))
}

pub fn delete_asset(store: &Store, id: i64) -> StoreResult<()> {
    store.with_write(|conn| delete_manual_asset(conn, id))
}

pub fn set_metadata(
    store: &Store,
    fund_id: i64,
    nav_currency: &str,
    risk_class: &str,
) -> StoreResult<()> {
    store.with_write(|conn| set_fund_metadata(conn, fund_id, nav_currency, risk_class))
}

pub fn summary(store: &Store, today: &str) -> Result<AssetsSummary, AssetError> {
    // 错误已是 AssetError（含 ASSET_AMOUNT_OVERFLOW），无需再包一层 Db。
    store.with_read(|conn| assets_summary(conn, today))
}

// 防止未使用告警：SCALE_SHARE 供 API 层解析份额相关输入使用。
const _: u32 = SCALE_SHARE;

/// AssetsSummary → JSON 投影（卡片与资产页共用；自 api/assets_api.rs 迁入，
/// mode/T04：查询投影的序列化属于所属领域）。
pub(crate) fn summary_json(summary: &AssetsSummary) -> String {
    let amount = |raw: Option<i64>| {
        raw.map(|v| {
            format!(
                "{{\"amount\":{},\"currency\":\"CNY\"}}",
                crate::wire::json_str(&crate::fixed::Fixed::amount(v).to_string_value())
            )
        })
        .unwrap_or_else(|| "null".into())
    };
    format!(
        "{{\"quality\":{quality},\"hasEntries\":{entries},\
         \"pricedCount\":{priced},\"missingPriceCount\":{missing},\
         \"currencyUnconfirmedCount\":{cur_unknown},\"currencyUnsupportedCount\":{cur_unsupported},\
         \"oldValuationCount\":{old},\"overlapReviewCount\":{overlap},\"unknownClassCount\":{unknown_cls},\
         \"fundValue\":{fund_value},\"manualValue\":{manual_value},\
         \"knownValue\":{known_value},\"completeValue\":{complete_value},\
         \"valuation\":{{\"batch\":{batch},\"fetchedAt\":{fetched_at},\"stale\":{stale},\
         \"missingQuotes\":{missing_quotes},\"unknownCosts\":{unknown_costs}}},\
         \"valuationDates\":{{\"earliest\":{earliest},\"latest\":{latest}}}}}",
        quality = crate::wire::json_str(summary.quality),
        entries = summary.has_entries,
        priced = summary.priced_count,
        missing = summary.missing_price_count,
        cur_unknown = summary.currency_unconfirmed_count,
        cur_unsupported = summary.currency_unsupported_count,
        old = summary.old_valuation_count,
        overlap = summary.overlap_review_count,
        unknown_cls = summary.unknown_class_count,
        fund_value = amount(summary.fund_value_minor),
        manual_value = amount(summary.manual_value_minor),
        known_value = amount(summary.known_value_minor),
        complete_value = amount(summary.complete_value_minor),
        batch = crate::wire::json_str(&summary.valuation_batch),
        fetched_at = summary
            .valuation_fetched_at
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".into()),
        stale = summary.valuation_stale,
        missing_quotes = summary.missing_quote_count,
        unknown_costs = summary.unknown_cost_count,
        earliest = summary.earliest_valuation.as_ref().map(|d| crate::wire::json_str(d)).unwrap_or_else(|| "null".into()),
        latest = summary.latest_valuation.as_ref().map(|d| crate::wire::json_str(d)).unwrap_or_else(|| "null".into()),
    )
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
