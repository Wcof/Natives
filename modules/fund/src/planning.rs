//! planning 模块：投资资产目标与配置检查（P2，方案 01 F3 / 05 §4 P2）。
//!
//! - 目标首期只有一个（固定 id=1）：金额 > 0，人民币分位；创建/修改时日期
//!   不早于北京时间当天；已保存目标到期后仍可修改金额，日期未改则保留原
//!   日期及「已到期」状态。
//! - 配置策略按主风险类别（5 类）保存目标与上下限（tenths = 百分比×10）；
//!   一次保存单事务验证并整体替换 5 行；目标总和严格 100.0%；
//!   `0 ≤ lower ≤ target ≤ upper ≤ 100`；unknown 不是可设置目标的类别。
//! - 比例比较用安全宽整数交叉相乘，不先把实际比例舍入成一位小数；
//!   端点视为「在区间」。
//! - 配置检查只给事实与差额，不给自动买卖建议；没有默认策略。

use crate::assets::{civil_from_days, days_from_civil, AssetsSummary};
use crate::fixed::Fixed;
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};

/// 5 类可设置目标的类别（unknown 不可设置）。
pub const TARGET_CLASSES: &[&str] = &["equity", "gold", "fixed_income", "cash", "other"];
/// 比例定点精度：tenths = 百分比(1 位小数) × 10。
pub const TENTHS_MAX: i64 = 1000;
/// 总和容差为 0：目标比例之和必须严格等于 100.0%。
pub const SUM_TARGET: i64 = TENTHS_MAX;

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    NotFound(String),
    Invalid(String),
    Db(String),
    /// 金额分位合计超出 i64 范围。截断会得到错误金额，必须显式报错。
    Overflow(String),
}

impl From<rusqlite::Error> for PlanError {
    fn from(e: rusqlite::Error) -> Self {
        PlanError::Db(e.to_string())
    }
}

impl From<crate::assets::AssetError> for PlanError {
    fn from(e: crate::assets::AssetError) -> Self {
        match e {
            crate::assets::AssetError::Db(m) => PlanError::Db(m),
            crate::assets::AssetError::Overflow(m) => PlanError::Overflow(m),
            other => PlanError::Invalid(other.message()),
        }
    }
}

impl PlanError {
    pub fn code(&self) -> &'static str {
        match self {
            PlanError::NotFound(_) => "PLAN_NOT_FOUND",
            PlanError::Invalid(_) => "PLAN_INVALID",
            PlanError::Db(_) => "PLAN_DB",
            PlanError::Overflow(_) => "ASSET_AMOUNT_OVERFLOW",
        }
    }

    pub fn message(&self) -> String {
        match self {
            PlanError::NotFound(m) => format!("记录不存在：{m}"),
            PlanError::Invalid(m) => format!("输入无效：{m}"),
            PlanError::Db(e) => format!("数据库错误：{e}"),
            PlanError::Overflow(m) => format!("金额超出可表示范围，已停止合计：{m}"),
        }
    }
}

// ---------------------------------------------------------------------------
// 目标
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalRow {
    pub target_minor: i64,
    pub target_date: String,
    /// 查询日 > target_date。
    pub expired: bool,
}

pub fn get_goal(conn: &Connection, today: &str) -> rusqlite::Result<Option<GoalRow>> {
    // R03 单一目标权威：旧编辑页读 finance_goals（旧 A 范围主目标，无则第一条）；
    // 归档目标视同无目标（清空后的状态），过期判定只看日期。
    let row = conn
        .query_row(
            "SELECT target_minor, target_date FROM finance_goals
             WHERE archived = 0 AND scope_kind='A' ORDER BY is_primary DESC, id LIMIT 1",
            [],
            |r| {
                let target_date: String = r.get(1)?;
                // 空 target_date＝显式「未设置期限」（方案 11 §3.2 / A11），不判过期。
                let expired = !target_date.is_empty() && today > target_date.as_str();
                Ok(GoalRow {
                    target_minor: r.get(0)?,
                    target_date,
                    expired,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// 保存目标。金额 > 0（分位）；日期为有效日历日期且不早于今天，
/// 除非调用方显式保留原日期（已到期目标只改金额的场景）。
pub fn save_goal(
    conn: &Connection,
    target_text: &str,
    target_date: &str,
    keep_existing_date: bool,
    today: &str,
) -> Result<(), PlanError> {
    let cleaned: String = target_text.chars().filter(|c| *c != ',').collect();
    let target = Fixed::parse(cleaned.trim(), 2)
        .map_err(|_| PlanError::Invalid("目标金额最多两位小数".into()))?;
    if target.is_zero() || target.is_negative() {
        return Err(PlanError::Invalid("目标金额必须大于零".into()));
    }
    let final_date = if keep_existing_date {
        let existing: Option<String> = conn
            .query_row(
                "SELECT target_date FROM finance_goals
                 WHERE archived=0 AND scope_kind='A' ORDER BY is_primary DESC, id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        existing.ok_or_else(|| PlanError::Invalid("没有可保留的既有目标日期".into()))?
    } else {
        if !crate::assets::is_valid_calendar_date(target_date) {
            return Err(PlanError::Invalid("目标日期不是有效日历日期".into()));
        }
        if target_date < today {
            return Err(PlanError::Invalid("目标日期不能早于今天".into()));
        }
        target_date.to_string()
    };
    // R03 单一目标权威：旧编辑页写入委托到 finance_goals 的 A 范围主目标，
    // 不再维护 investment_goals 第二套事实。
    let existing: Option<(i64, i64)> = conn
        .query_row(
            "SELECT id, version FROM finance_goals WHERE archived=0 AND scope_kind='A' ORDER BY is_primary DESC, id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, version)) = existing {
        let g = crate::goals::get_goal_by_id(conn, id)
            .map_err(|e| PlanError::Invalid(e.message()))?
            .unwrap();
        crate::goals::update_goal(
            conn,
            id,
            &crate::goals::GoalInput {
                name: g.name,
                target_text: target_text.into(),
                target_date: final_date,
                scope_kind: "A".into(),
                account_ids: Vec::new(),
                monthly_plan_text: g
                    .monthly_plan_minor
                    .map(|v| format!("{}.{:02}", v / 100, v % 100)),
                note: g.note,
            },
            version,
            today,
        )
        .map_err(|e| PlanError::Invalid(e.message()))?;
    } else {
        let id = crate::goals::create_goal(
            conn,
            &crate::goals::GoalInput {
                name: "整体目标".into(),
                target_text: target_text.into(),
                target_date: final_date,
                scope_kind: "A".into(),
                account_ids: Vec::new(),
                monthly_plan_text: None,
                note: String::new(),
            },
            today,
        )
        .map_err(|e| PlanError::Invalid(e.message()))?;
        let primary: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_goals WHERE is_primary=1 AND archived=0)",
            [],
            |r| r.get(0),
        )?;
        if !primary {
            crate::goals::set_primary_goal(conn, id)
                .map_err(|e| PlanError::Invalid(e.message()))?;
        }
    }
    Ok(())
}

pub fn clear_goal(conn: &Connection) -> Result<(), PlanError> {
    // R03 单一目标权威：清空旧的 id=1 目标改为归档 finance_goals 活跃目标。
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    conn.execute(
        "UPDATE finance_goals SET archived=1, is_primary=0, version=version+1, updated_at=?1 WHERE id=(SELECT id FROM finance_goals WHERE archived=0 AND scope_kind='A' ORDER BY is_primary DESC,id LIMIT 1)",
        params![now],
    )?;
    Ok(())
}

/// 目标进度（方案 05 §4 P2）：只在估值完整且无较旧估值时计算。
/// 有记录且总额为零 → 0.0%；没有记录 → 不可计算（assets_empty）。
pub fn goal_progress(summary: &AssetsSummary, goal: &GoalRow) -> GoalProgress {
    if !summary.has_entries {
        return GoalProgress {
            computable: false,
            reason_codes: vec!["assets_empty".into()],
            ratio_tenths: None,
            current_value_minor: None,
        };
    }
    if summary.quality != "ready" {
        let mut reasons = Vec::new();
        if summary.missing_price_count > 0
            || summary.currency_unconfirmed_count > 0
            || summary.currency_unsupported_count > 0
            || summary.overlap_review_count > 0
        {
            reasons.push("incomplete".into());
        }
        if summary.old_valuation_count > 0 {
            reasons.push("stale".into());
        }
        return GoalProgress {
            computable: false,
            reason_codes: reasons,
            ratio_tenths: None,
            current_value_minor: None,
        };
    }
    // 完整且新鲜：complete_value 存在（has_entries 且无缺口）。
    let value = summary
        .complete_value_minor
        .unwrap_or_else(|| summary.known_value_minor.unwrap_or(0));
    // ratio = value / target，以 0.1% 为单位的定点（tenths）：
    // value(分) / target(分) × 1000，宽整数交叉相乘后舍入到 1 位小数（四舍五入）。
    let target = goal.target_minor.max(1) as i128;
    let numerator = (value as i128) * 1000;
    let ratio_tenths = ((numerator + target / 2) / target) as i64;
    GoalProgress {
        computable: true,
        reason_codes: Vec::new(),
        ratio_tenths: Some(ratio_tenths),
        current_value_minor: Some(value),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalProgress {
    pub computable: bool,
    pub reason_codes: Vec<String>,
    /// 进度（0.1% 定点；可超过 1000 显示真实比率）。
    pub ratio_tenths: Option<i64>,
    pub current_value_minor: Option<i64>,
}

// ---------------------------------------------------------------------------
// 配置策略
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationTargetRow {
    pub risk_class: String,
    pub target_tenths: i64,
    pub lower_tenths: i64,
    pub upper_tenths: i64,
}

pub fn get_allocation(conn: &Connection) -> rusqlite::Result<Vec<AllocationTargetRow>> {
    let mut stmt = conn.prepare(
        "SELECT risk_class, target_tenths, lower_tenths, upper_tenths
         FROM allocation_targets ORDER BY target_tenths DESC, risk_class",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AllocationTargetRow {
                risk_class: r.get(0)?,
                target_tenths: r.get(1)?,
                lower_tenths: r.get(2)?,
                upper_tenths: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 一次策略保存：验证 + 整体替换 5 行（同一事务；失败保持旧策略）。
/// 输入必须恰好覆盖 5 类，不得缺失或多出。
pub fn save_allocation(conn: &Connection, rows: &[AllocationTargetRow]) -> Result<(), PlanError> {
    if rows.len() != TARGET_CLASSES.len() {
        return Err(PlanError::Invalid(format!(
            "策略必须覆盖全部 {} 个类别",
            TARGET_CLASSES.len()
        )));
    }
    let mut sum: i64 = 0;
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        if !seen.insert(&row.risk_class) {
            return Err(PlanError::Invalid("配置类别重复".into()));
        }
        if !TARGET_CLASSES.contains(&row.risk_class.as_str()) {
            return Err(PlanError::Invalid(format!("未知类别 {}", row.risk_class)));
        }
        for value in [row.target_tenths, row.lower_tenths, row.upper_tenths] {
            if !(0..=TENTHS_MAX).contains(&value) {
                return Err(PlanError::Invalid(format!(
                    "{} 比例必须在 0.0%~100.0% 之间",
                    row.risk_class
                )));
            }
        }
        if row.lower_tenths > row.target_tenths || row.target_tenths > row.upper_tenths {
            return Err(PlanError::Invalid(format!(
                "{} 必须满足 下限 ≤ 目标 ≤ 上限",
                row.risk_class
            )));
        }
        sum += row.target_tenths;
    }
    if sum != SUM_TARGET {
        return Err(PlanError::Invalid(format!(
            "目标比例之和必须为 100.0%（当前 {}）",
            tenths_to_text(sum)
        )));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    conn.execute("DELETE FROM allocation_targets", [])?;
    for row in rows {
        conn.execute(
            "INSERT INTO allocation_targets(risk_class, target_tenths, lower_tenths, upper_tenths, updated_at)
             VALUES(?1,?2,?3,?4,?5)",
            params![row.risk_class, row.target_tenths, row.lower_tenths, row.upper_tenths, now],
        )?;
    }
    Ok(())
}

/// 0.1% 定点 → 百分比文本（1 位小数）。
pub fn tenths_to_text(tenths: i64) -> String {
    let sign = if tenths < 0 { "-" } else { "" };
    let tenths = tenths.abs();
    format!("{sign}{}.{:01}", tenths / 10, tenths % 10)
}

// ---------------------------------------------------------------------------
// 配置检查
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ClassCheck {
    pub risk_class: String,
    pub target_tenths: i64,
    pub lower_tenths: i64,
    pub upper_tenths: i64,
    /// 实际占比（0.1% 定点，四舍五入到 1 位小数）；仅用于展示。
    pub actual_tenths: Option<i64>,
    /// 实际金额（分位 raw）。
    pub amount_minor: Option<i64>,
    pub target_minor: Option<i64>,
    pub gap_minor: Option<i64>,
    /// within | below | above；不可计算时为 None。
    pub status: Option<&'static str>,
    /// 偏差（百分点×10，actual − target）。
    pub delta_tenths: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct AllocationCheck {
    pub configured: bool,
    pub computable: bool,
    pub reason_codes: Vec<String>,
    pub unknown_class_count: usize,
    pub classes: Vec<ClassCheck>,
    pub out_of_range: Vec<OutOfRangeItem>,
    /// 有效组合总额（分位 raw）；T=0 时不可计算。
    pub total_minor: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutOfRangeItem {
    pub risk_class: String,
    pub direction: &'static str,
    pub actual_tenths: i64,
}

/// 配置检查（方案 05 §4 P2）：估值完整、未过期、unknown=0 才计算实际占比；
/// 有效组合总额 T > 0 才有占比（零分母显式不可计算）。
/// 比较用宽整数交叉相乘：actual_tenths_raw = value × 1000 / T；
/// 端点在区间内；先比较真实值再展示舍入值。
/// 金额分桶同样不可截断：溢出返回 ASSET_AMOUNT_OVERFLOW，不给错误占比。
fn bucket_for(
    risk_class: &str,
    value_minor: i64,
    buckets: &mut Vec<(String, i64)>,
) -> Result<(), crate::assets::AssetError> {
    let key = TARGET_CLASSES
        .iter()
        .find(|c| **c == risk_class)
        .copied()
        .unwrap_or("other");
    if let Some(entry) = buckets.iter_mut().find(|(c, _)| c == key) {
        entry.1 = crate::assets::add_minor(entry.1, value_minor, "风险类别金额合计")?;
    }
    Ok(())
}

pub fn check_allocation(
    conn: &Connection,
    summary: &AssetsSummary,
) -> Result<AllocationCheck, crate::assets::AssetError> {
    let targets = get_allocation(conn)?;
    let mut check = AllocationCheck {
        configured: !targets.is_empty(),
        computable: false,
        reason_codes: Vec::new(),
        unknown_class_count: summary.unknown_class_count,
        classes: Vec::new(),
        out_of_range: Vec::new(),
        total_minor: None,
    };
    if !check.configured {
        return Ok(check);
    }

    if !summary.has_entries {
        check.reason_codes.push("empty_portfolio".into());
        return Ok(check);
    }
    // 基数口径（R04）：配置检查与模拟统一用 B = A − R（02 §1.1）。
    // R 未配置时 B 不可计算（不默认 0 预留），显式挂起原因。
    let reserved = match crate::finance::cash_position(conn) {
        Ok(cash) => cash.reserved_minor,
        Err(_) => None,
    };
    let Some(reserved) = reserved else {
        check.reason_codes.push("reserve_unconfigured".into());
        return Ok(check);
    };
    let mut has_gap = false;
    if summary.missing_price_count > 0 {
        check.reason_codes.push("missing_nav".into());
        has_gap = true;
    }
    if summary.currency_unconfirmed_count > 0 || summary.currency_unsupported_count > 0 {
        check.reason_codes.push("currency_issue".into());
        has_gap = true;
    }
    if summary.overlap_review_count > 0 {
        check.reason_codes.push("overlap_review".into());
        has_gap = true;
    }
    if summary.old_valuation_count > 0 {
        check.reason_codes.push("stale".into());
        has_gap = true;
    }
    if summary.unknown_class_count > 0 {
        check.reason_codes.push("unknown_class".into());
        has_gap = true;
    }
    if has_gap {
        return Ok(check);
    }

    // 金额分桶：基金（已计价）+ 人工（无重复争议）按 risk_class 求和。
    let mut buckets: Vec<(String, i64)> = TARGET_CLASSES
        .iter()
        .map(|c| (c.to_string(), 0i64))
        .collect();
    for fund in &summary.funds {
        if let Some(value) = fund.value_minor {
            bucket_for(&fund.risk_class, value, &mut buckets)?;
        }
    }
    for manual in &summary.manual {
        if !manual.overlap_review {
            bucket_for(&manual.risk_class, manual.market_value_minor, &mut buckets)?;
        }
    }
    // R04：B = A − R。预留从现金桶扣除（不足以扣时从其他桶按序扣，
    // 保持五类加总精确等于 B；02 §1.1/§2）。
    let cash_bucket = buckets
        .iter_mut()
        .find(|(k, _)| k == "cash")
        .expect("fixed classes");
    if reserved > cash_bucket.1 {
        check.reason_codes.push("reserve_exceeds_cash".into());
        return Ok(check);
    }
    cash_bucket.1 -= reserved;
    let mut total: i64 = 0;
    for (_, value) in &buckets {
        total = crate::assets::add_minor(total, *value, "配置组合总额")?;
    }
    check.total_minor = Some(total);
    if total <= 0 {
        check.reason_codes.push("zero_total".into());
        // 各类实际金额仍给出（都是 0），但没有占比与状态。
        for target in &targets {
            let amount = buckets
                .iter()
                .find(|(c, _)| c == &target.risk_class)
                .map(|(_, v)| *v)
                .unwrap_or(0);
            check.classes.push(ClassCheck {
                risk_class: target.risk_class.clone(),
                target_tenths: target.target_tenths,
                lower_tenths: target.lower_tenths,
                upper_tenths: target.upper_tenths,
                actual_tenths: None,
                amount_minor: Some(amount),
                target_minor: Some(0),
                gap_minor: Some(0),
                status: None,
                delta_tenths: None,
            });
        }
        return Ok(check);
    }

    let amounts = target_amounts(total, &targets);
    for (target, target_amount) in targets.iter().zip(amounts) {
        let amount = buckets
            .iter()
            .find(|(c, _)| c == &target.risk_class)
            .map(|(_, v)| *v)
            .unwrap_or(0);
        // 实际占比（tenths，四舍五入）：amount × 1000 / total。
        let raw_tenths = ((amount as i128) * 1000 + (total as i128) / 2) / (total as i128);
        let actual_tenths = raw_tenths as i64;
        // 端点视为在区间：比较真实值（amount×1000 vs lower×total），
        // 不把舍入后的 actual_tenths 当作判定依据（方案 05 §4 P2）。
        let status: &'static str =
            if (amount as i128) * 1000 < (target.lower_tenths as i128) * (total as i128) {
                "below"
            } else if (amount as i128) * 1000 > (target.upper_tenths as i128) * (total as i128) {
                "above"
            } else {
                "within"
            };
        if status != "within" {
            check.out_of_range.push(OutOfRangeItem {
                risk_class: target.risk_class.clone(),
                direction: status,
                actual_tenths,
            });
        }
        check.classes.push(ClassCheck {
            risk_class: target.risk_class.clone(),
            target_tenths: target.target_tenths,
            lower_tenths: target.lower_tenths,
            upper_tenths: target.upper_tenths,
            actual_tenths: Some(actual_tenths),
            amount_minor: Some(amount),
            target_minor: Some(target_amount),
            gap_minor: Some(target_amount - amount),
            status: Some(status),
            delta_tenths: Some(actual_tenths - target.target_tenths),
        });
    }
    check.computable = true;
    Ok(check)
}

/// 用最大余数法分配分位，固定类别顺序处理同余数，合计精确等于基数。
pub fn target_amounts(total: i64, targets: &[AllocationTargetRow]) -> Vec<i64> {
    let mut values: Vec<i64> = targets
        .iter()
        .map(|t| ((total as i128) * t.target_tenths as i128 / 1000) as i64)
        .collect();
    let used: i128 = values.iter().map(|v| *v as i128).sum();
    let mut order: Vec<usize> = (0..targets.len()).collect();
    order.sort_by_key(|i| {
        (
            std::cmp::Reverse((total as i128 * targets[*i].target_tenths as i128) % 1000),
            targets[*i].risk_class.clone(),
        )
    });
    for i in order
        .into_iter()
        .take((total as i128 - used).max(0) as usize)
    {
        values[i] += 1;
    }
    values
}

// ---------------------------------------------------------------------------
// Store 便捷封装
// ---------------------------------------------------------------------------

pub fn save_goal_tx(
    store: &Store,
    target: &str,
    date: &str,
    keep_date: bool,
    today: &str,
) -> Result<(), PlanError> {
    store.with_write(|conn| save_goal(conn, target, date, keep_date, today))
}

pub fn clear_goal_tx(store: &Store) -> Result<(), PlanError> {
    store.with_write(clear_goal)
}

pub fn save_allocation_tx(store: &Store, rows: &[AllocationTargetRow]) -> Result<(), PlanError> {
    store.with_write(|conn| save_allocation(conn, rows))
}

pub fn goal_with_progress(
    store: &Store,
    today: &str,
) -> Result<Option<(GoalRow, GoalProgress)>, PlanError> {
    let (summary, _) = store
        .with_read(|conn| crate::assets::summary_with_cached_quotes(conn, today))
        .map_err(|e| PlanError::Db(e.message()))?;
    store
        .with_read(|conn| get_goal(conn, today))
        .map_err(|e| PlanError::Db(e.to_string()))
        .map(|goal| {
            goal.map(|goal| {
                let progress = goal_progress(&summary, &goal);
                (goal, progress)
            })
        })
}

pub fn allocation_check(store: &Store, today: &str) -> Result<AllocationCheck, PlanError> {
    let (summary, _) = store
        .with_read(|conn| crate::assets::summary_with_cached_quotes(conn, today))
        .map_err(|e| PlanError::Db(e.message()))?;
    store
        .with_read(|conn| check_allocation(conn, &summary))
        .map_err(PlanError::from)
}

/// 目标日期过期判定工具（测试与 UI 共用语义）。
pub fn is_expired(target_date: &str, today: &str) -> bool {
    match (days_from_civil(target_date), days_from_civil(today)) {
        (Some(a), Some(b)) => b > a,
        _ => false,
    }
}

/// 供 UI/测试复用的日期推进（today + n 天）。
pub fn shift_days(date: &str, days: i64) -> Option<String> {
    days_from_civil(date).map(|d| civil_from_days(d + days))
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod tests;
