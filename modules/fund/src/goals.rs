//! 多财务目标（02 §6）：finance 目标域拆分（架构规模门禁）。
//! CRUD、互斥校验、变更记录与目标进度；金额与错误类型沿用 finance。

use crate::assets::{add_minor, is_valid_calendar_date};
use crate::finance::{now_secs, parse_amount_minor, FinanceError};
use rusqlite::{params, Connection, OptionalExtension};

// ---------------------------------------------------------------------------
// 多财务目标（02 §6）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinanceGoalRow {
    pub id: i64,
    pub name: String,
    pub target_minor: i64,
    pub target_date: String,
    pub scope_kind: String,
    pub asset_bindings: String,
    pub is_primary: bool,
    pub archived: bool,
    pub monthly_plan_minor: Option<i64>,
    pub version: i64,
    pub note: String,
}

fn row_to_goal(r: &rusqlite::Row) -> rusqlite::Result<FinanceGoalRow> {
    Ok(FinanceGoalRow {
        id: r.get(0)?,
        name: r.get(1)?,
        target_minor: r.get(2)?,
        target_date: r.get(3)?,
        scope_kind: r.get(4)?,
        asset_bindings: r.get(5)?,
        is_primary: r.get::<_, i64>(6)? == 1,
        archived: r.get::<_, i64>(7)? == 1,
        monthly_plan_minor: r.get(8)?,
        version: r.get(9)?,
        note: r.get(10)?,
    })
}

const GOAL_COLS: &str = "id, name, target_minor, target_date, scope_kind, asset_bindings,
                         is_primary, archived, monthly_plan_minor, version, note";

pub fn list_goals(
    conn: &Connection,
    include_archived: bool,
) -> Result<Vec<FinanceGoalRow>, FinanceError> {
    let sql = if include_archived {
        format!("SELECT {GOAL_COLS} FROM finance_goals ORDER BY archived, is_primary DESC, id")
    } else {
        format!(
            "SELECT {GOAL_COLS} FROM finance_goals WHERE archived = 0 ORDER BY is_primary DESC, id"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_goal)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_goal_by_id(conn: &Connection, id: i64) -> Result<Option<FinanceGoalRow>, FinanceError> {
    let sql = format!("SELECT {GOAL_COLS} FROM finance_goals WHERE id = ?1");
    let row = conn.query_row(&sql, params![id], row_to_goal).optional()?;
    Ok(row)
}

pub fn primary_goal(conn: &Connection) -> Result<Option<FinanceGoalRow>, FinanceError> {
    let sql =
        format!("SELECT {GOAL_COLS} FROM finance_goals WHERE is_primary=1 AND archived=0 LIMIT 1");
    let row = conn.query_row(&sql, [], row_to_goal).optional()?;
    Ok(row)
}

/// 当前目标唯一权威投影（R2 T03 / A52）：活跃目标中 is_primary 优先。
/// 新首页、空间投资卡与旧 goal API 对同一行读取；唯一性由 domain
/// （create/restore 拒绝第二个活跃目标）与部分唯一索引共同保证。
pub fn current_goal(conn: &Connection) -> Result<Option<FinanceGoalRow>, FinanceError> {
    let sql = format!(
        "SELECT {GOAL_COLS} FROM finance_goals WHERE archived = 0 ORDER BY is_primary DESC, id LIMIT 1"
    );
    let row = conn.query_row(&sql, [], row_to_goal).optional()?;
    Ok(row)
}

pub struct GoalInput {
    pub name: String,
    pub target_text: String,
    pub target_date: String,
    pub scope_kind: String,
    /// 专项绑定的账户 ID 列表（scope_kind=specific 时必填且非空）。
    pub account_ids: Vec<i64>,
    pub monthly_plan_text: Option<String>,
    pub note: String,
}

fn validate_goal_input(
    conn: &Connection,
    input: &GoalInput,
    today: &str,
) -> Result<(i64, Option<i64>), FinanceError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(FinanceError::Invalid("目标名称需 1~64 字符".into()));
    }
    let target = parse_amount_minor(&input.target_text, "目标金额")?;
    if target <= 0 {
        return Err(FinanceError::Invalid(
            "目标金额必须大于零（G>0 是保存条件）".into(),
        ));
    }
    // 方案 11 §3.2：期限选填。空字符串是明确的"未设置期限"语义，
    // 不伪造日期；有值时仍是真实日历日期。
    if !input.target_date.is_empty() {
        if !is_valid_calendar_date(&input.target_date) {
            return Err(FinanceError::Invalid("截止日期不是有效日历日期".into()));
        }
        if !today.is_empty() && input.target_date.as_str() < today {
            return Err(FinanceError::Invalid("新建目标截止日期不能早于今天".into()));
        }
    }
    if !matches!(input.scope_kind.as_str(), "A" | "B" | "specific") {
        return Err(FinanceError::Invalid(format!(
            "未知范围类型 {}",
            input.scope_kind
        )));
    }
    if input.scope_kind == "specific" && input.account_ids.is_empty() {
        return Err(FinanceError::Invalid(
            "专项目标必须绑定至少一个账户（未指定覆盖资产的目标不能用全组合充当）".into(),
        ));
    }
    if input.scope_kind != "specific" && !input.account_ids.is_empty() {
        return Err(FinanceError::Invalid("仅专项目标允许绑定账户".into()));
    }
    let mut seen = std::collections::HashSet::new();
    for id in &input.account_ids {
        if !seen.insert(*id)
            || !conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(FinanceError::Invalid("绑定账户不存在或重复".into()));
        }
    }
    let monthly = match input.monthly_plan_text.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(text) => {
            let amount = parse_amount_minor(text, "每月投入")?;
            if amount <= 0 {
                return Err(FinanceError::Invalid("每月投入必须大于零".into()));
            }
            Some(amount)
        }
    };
    Ok((target, monthly))
}

/// 新建目标。唯一当前目标契约下，新建即唯一活跃目标，自动成为当前目标（首页按
/// is_primary 读取，不要求用户再手动设主；02 §6.2 的「不随机切换」由唯一约束保证）。
pub fn create_goal(conn: &Connection, input: &GoalInput, today: &str) -> Result<i64, FinanceError> {
    // 唯一当前目标（方案 01 §3；审计四-1）：domain 层与 API 层同样保证——
    // 已存在未归档目标时拒绝新建（重复提交/双窗口），历史归档目标不受限。
    let has_current: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_goals WHERE archived=0)",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| v == 1)
        .map_err(|e| FinanceError::Db(e.to_string()))?;
    if has_current {
        return Err(FinanceError::Conflict(
            "已存在当前财务目标；请修改而非新建".into(),
        ));
    }
    let (target, monthly) = validate_goal_input(conn, input, today)?;
    if input.scope_kind == "specific" {
        check_account_exclusive(conn, 0, &input.account_ids)?;
    }
    let bindings =
        serde_json::to_string(&input.account_ids).map_err(|e| FinanceError::Db(e.to_string()))?;
    let now = now_secs();
    conn.execute(
        "INSERT INTO finance_goals(name, target_minor, target_date, scope_kind, asset_bindings,
                                   is_primary, archived, monthly_plan_minor, version, note,
                                   created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,1,0,?6,1,?7,?8,?8)",
        params![
            input.name.trim(),
            target,
            input.target_date,
            input.scope_kind,
            bindings,
            monthly,
            input.note,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 编辑目标（带版本冲突检测：expected_version 不匹配返回 Conflict）。
/// 关键修改记录前后值（goal_changes）。范围变更时绑定互斥校验。
pub fn update_goal(
    conn: &Connection,
    id: i64,
    input: &GoalInput,
    expected_version: i64,
    today: &str,
) -> Result<i64, FinanceError> {
    let existing =
        get_goal_by_id(conn, id)?.ok_or_else(|| FinanceError::NotFound(format!("目标 {id}")))?;
    if existing.version != expected_version {
        return Err(FinanceError::Conflict(format!(
            "目标已被他处修改（当前版本 {}，请求版本 {expected_version}）",
            existing.version
        )));
    }
    let (target, monthly) = validate_goal_input(
        conn,
        input,
        if input.target_date == existing.target_date {
            ""
        } else {
            today
        },
    )?;
    if existing.archived {
        return Err(FinanceError::Invalid("请先恢复归档目标再编辑".into()));
    }
    // 已到期目标允许保留原日期只改金额（02 §6.3）。
    let date_valid =
        input.target_date == existing.target_date || input.target_date.as_str() >= today;
    if !date_valid {
        return Err(FinanceError::Invalid(
            "截止日期不能早于今天（保留原日期请显式不改）".into(),
        ));
    }
    // 专项绑定互斥：同一账户不得同时归属多个活跃专项目标。
    if input.scope_kind == "specific" {
        let bindings = serde_json::to_string(&input.account_ids)
            .map_err(|e| FinanceError::Db(e.to_string()))?;
        check_account_exclusive(conn, id, &input.account_ids)?;
        let now = now_secs();
        conn.execute(
            "UPDATE finance_goals SET name=?2, target_minor=?3, target_date=?4, scope_kind=?5,
             asset_bindings=?6, monthly_plan_minor=?7, note=?8, version=version+1, updated_at=?9
             WHERE id=?1",
            params![
                id,
                input.name.trim(),
                target,
                input.target_date,
                input.scope_kind,
                bindings,
                monthly,
                input.note,
                now
            ],
        )?;
    } else {
        let now = now_secs();
        conn.execute(
            "UPDATE finance_goals SET name=?2, target_minor=?3, target_date=?4, scope_kind=?5,
             asset_bindings='[]', monthly_plan_minor=?6, note=?7, version=version+1, updated_at=?8
             WHERE id=?1",
            params![
                id,
                input.name.trim(),
                target,
                input.target_date,
                input.scope_kind,
                monthly,
                input.note,
                now
            ],
        )?;
    }
    record_goal_changes(conn, id, &existing, input, monthly)?;
    Ok(get_goal_by_id(conn, id)?.map(|g| g.version).unwrap_or(0))
}

fn check_account_exclusive(
    conn: &Connection,
    self_id: i64,
    account_ids: &[i64],
) -> Result<(), FinanceError> {
    for goal in list_goals(conn, false)? {
        if goal.id == self_id || goal.scope_kind != "specific" {
            continue;
        }
        let list: Vec<i64> = serde_json::from_str(&goal.asset_bindings)
            .map_err(|_| FinanceError::Invalid("现有目标账户绑定损坏，需核对".into()))?;
        if account_ids.iter().any(|id| list.contains(id)) {
            return Err(FinanceError::Conflict("账户已归属其他活跃专项目标".into()));
        }
    }
    Ok(())
}

fn record_goal_changes(
    conn: &Connection,
    id: i64,
    existing: &FinanceGoalRow,
    input: &GoalInput,
    new_monthly: Option<i64>,
) -> Result<(), FinanceError> {
    let now = now_secs();
    let mut changes: Vec<(&str, String, String)> = Vec::new();
    if existing.name != input.name.trim() {
        changes.push(("name", existing.name.clone(), input.name.trim().to_string()));
    }
    if existing.target_minor
        != parse_amount_minor(&input.target_text, "目标金额").unwrap_or(existing.target_minor)
    {
        changes.push((
            "target_minor",
            existing.target_minor.to_string(),
            parse_amount_minor(&input.target_text, "目标金额")
                .unwrap_or(existing.target_minor)
                .to_string(),
        ));
    }
    if existing.target_date != input.target_date {
        changes.push((
            "target_date",
            existing.target_date.clone(),
            input.target_date.to_string(),
        ));
    }
    if existing.scope_kind != input.scope_kind {
        changes.push((
            "scope_kind",
            existing.scope_kind.clone(),
            input.scope_kind.clone(),
        ));
    }
    if existing.monthly_plan_minor != new_monthly {
        changes.push((
            "monthly_plan_minor",
            existing
                .monthly_plan_minor
                .map(|v| v.to_string())
                .unwrap_or_default(),
            new_monthly.map(|v| v.to_string()).unwrap_or_default(),
        ));
    }
    let bindings = serde_json::to_string(&input.account_ids).unwrap();
    if existing.asset_bindings != bindings {
        changes.push(("asset_bindings", existing.asset_bindings.clone(), bindings));
    }
    if existing.note != input.note {
        changes.push(("note", existing.note.clone(), input.note.clone()));
    }
    for (field, old, new) in changes {
        conn.execute(
            "INSERT INTO goal_changes(goal_id, field, old_value, new_value, actor, created_at)
             VALUES(?1,?2,?3,?4,'user',?5)",
            params![id, field, old, new, now],
        )?;
    }
    Ok(())
}

/// 设主目标：唯一（部分唯一索引兜底）；归档目标不可设主。
pub fn set_primary_goal(conn: &Connection, id: i64) -> Result<(), FinanceError> {
    let goal =
        get_goal_by_id(conn, id)?.ok_or_else(|| FinanceError::NotFound(format!("目标 {id}")))?;
    if goal.archived {
        return Err(FinanceError::Invalid("归档目标不能设为主目标".into()));
    }
    let now = now_secs();
    conn.execute(
        "UPDATE finance_goals SET is_primary=0, updated_at=?1",
        params![now],
    )?;
    conn.execute(
        "UPDATE finance_goals SET is_primary=1, updated_at=?2 WHERE id=?1",
        params![id, now],
    )?;
    Ok(())
}

/// 归档：不删除资产/流水/记录；主目标归档后首页无主目标（提示选择）。
pub fn archive_goal(conn: &Connection, id: i64) -> Result<(), FinanceError> {
    let now = now_secs();
    let affected = conn.execute(
        "UPDATE finance_goals SET archived=1, is_primary=0, version=version+1, updated_at=?2 WHERE id=?1 AND archived=0",
        params![id, now],
    )?;
    if affected == 0 {
        return Err(FinanceError::NotFound(format!("活跃目标 {id}")));
    }
    Ok(())
}

/// 恢复归档目标。唯一当前目标契约：已存在其他活跃目标时拒绝恢复；
/// 恢复成功即恢复为当前目标（is_primary=1，与 create_goal 同口径）。
pub fn restore_goal(conn: &Connection, id: i64) -> Result<(), FinanceError> {
    let goal =
        get_goal_by_id(conn, id)?.ok_or_else(|| FinanceError::NotFound(format!("目标 {id}")))?;
    let has_current: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM finance_goals WHERE archived=0)",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| v == 1)
        .map_err(|e| FinanceError::Db(e.to_string()))?;
    if has_current {
        return Err(FinanceError::Conflict(
            "已存在当前财务目标；请先归档后再恢复此目标".into(),
        ));
    }
    if goal.scope_kind == "specific" {
        let ids: Vec<i64> = serde_json::from_str(&goal.asset_bindings)
            .map_err(|_| FinanceError::Invalid("目标绑定损坏".into()))?;
        check_account_exclusive(conn, id, &ids)?;
    }
    let now = now_secs();
    let affected = conn.execute(
        "UPDATE finance_goals SET archived=0, is_primary=1, version=version+1, updated_at=?2 WHERE id=?1 AND archived=1",
        params![id, now],
    )?;
    if affected == 0 {
        return Err(FinanceError::NotFound(format!("归档目标 {id}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 目标进度与投入日历（02 §6.3）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalProgress {
    pub computable: bool,
    /// 不可计算原因：assets_empty | incomplete | stale | unbound | scope_asset_missing。
    pub reason_codes: Vec<String>,
    pub scope_value_minor: Option<i64>,
    pub ratio_tenths: Option<i64>,
    /// 进度百万分比（1 ppm = 0.0001%）：前端动态小数显示的精度源，
    /// 避免低于 0.1% 的真实进度在 tenths 粒度塌缩为 0。
    pub ratio_ppm: Option<i64>,
    /// 金额缺口 = max(0, G−S)；目标超额 = max(0, S−G)。
    pub gap_minor: Option<i64>,
    pub surplus_minor: Option<i64>,
    /// 剩余可投入次数与零收益所需每次投入。
    pub remaining_investments: Option<i64>,
    pub required_monthly_minor: Option<i64>,
    pub expired: bool,
}

/// 月末日期序列（含截止日当月，若截止日 ≥ 该月末则该月计入）。
/// 零收益所需月投入 = ceil(缺口 / 剩余次数)（02 §6.3，按真实日历枚举月末）。
pub fn month_ends_until(today: &str, target_date: &str) -> Vec<String> {
    let mut result = Vec::new();
    let Some(mut day) = crate::assets::days_from_civil(today) else {
        return result;
    };
    let target_day = crate::assets::days_from_civil(target_date);
    let Some(target_day) = target_day else {
        return result;
    };
    // 从本月起逐月推进，最多 1200 个月防御。
    for _ in 0..1200 {
        // 本月月末 = 下月1日 − 1 天。
        let (y, m, _d) = split_date(&crate::assets::civil_from_days(day));
        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        let next_first = format!("{ny:04}-{nm:02}-01");
        let Some(nf) = crate::assets::days_from_civil(&next_first) else {
            break;
        };
        let month_end = crate::assets::civil_from_days(nf - 1);
        let month_end_day = nf - 1;
        if month_end_day > target_day {
            break;
        }
        // 今天投入日已过（月末 < 今天）不计；今天是月末且未确认完成——保守计入未来判定之外，
        // 只统计严格大于今天的月末（今天的投入是否完成由用户核对，不预记）。
        if month_end_day > day {
            result.push(month_end);
        }
        day = nf;
        if month_end_day == target_day {
            break;
        }
    }
    result
}

fn split_date(date: &str) -> (i64, i64, i64) {
    let parts: Vec<&str> = date.split('-').collect();
    (
        parts.first().and_then(|v| v.parse().ok()).unwrap_or(0),
        parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
        parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0),
    )
}

/// 目标范围当前覆盖 S。summary/cash 由调用方在同一读快照内取得。
pub fn goal_scope_value(
    conn: &Connection,
    goal: &FinanceGoalRow,
    summary: &crate::assets::AssetsSummary,
    cash_minor: Option<i64>,
) -> GoalProgress {
    let expired = crate::planning::is_expired(&goal.target_date, &today_cn());
    let mut progress = GoalProgress {
        computable: false,
        reason_codes: Vec::new(),
        scope_value_minor: None,
        ratio_tenths: None,
        ratio_ppm: None,
        gap_minor: None,
        surplus_minor: None,
        remaining_investments: None,
        required_monthly_minor: None,
        expired,
    };
    match goal.scope_kind.as_str() {
        "A" => {
            // 管理资产 A：资产聚合完整值（含现金类人工资产）。
            if !summary.has_entries {
                progress.reason_codes.push("assets_empty".into());
                return progress;
            }
            if summary.quality != "ready" {
                if summary.missing_price_count > 0
                    || summary.currency_unconfirmed_count > 0
                    || summary.currency_unsupported_count > 0
                    || summary.overlap_review_count > 0
                {
                    progress.reason_codes.push("incomplete".into());
                }
                if summary.old_valuation_count > 0 {
                    progress.reason_codes.push("stale".into());
                }
                return progress;
            }
            let value = summary.complete_value_minor.unwrap_or(0);
            finish_progress(&mut progress, value, goal, summary);
        }
        "B" => {
            // 可配置资产 B = A − R。R 未配置时 B 不可计算（不默认 0 预留）。
            if !summary.has_entries {
                progress.reason_codes.push("assets_empty".into());
                return progress;
            }
            if summary.quality != "ready" {
                progress.reason_codes.push("incomplete".into());
                return progress;
            }
            let Some(a) = summary.complete_value_minor else {
                progress.reason_codes.push("incomplete".into());
                return progress;
            };
            let Some(r) = cash_position_scope_reserved(conn) else {
                progress.reason_codes.push("reserve_unconfigured".into());
                return progress;
            };
            let Ok(value) = add_minor(a, -r, "可配置资产") else {
                return progress;
            };
            finish_progress(&mut progress, value, goal, summary);
        }
        "specific" => {
            // 专项目标：绑定账户的资产合计（估值完整才计算）。
            if !summary.has_entries {
                progress.reason_codes.push("assets_empty".into());
                return progress;
            }
            let Ok(bindings) = serde_json::from_str::<Vec<i64>>(&goal.asset_bindings) else {
                progress.reason_codes.push("scope_asset_missing".into());
                return progress;
            };
            if bindings.is_empty() {
                progress.reason_codes.push("unbound".into());
                return progress;
            }
            let mut value = 0;
            for account in bindings {
                let funds: Vec<_> = summary
                    .funds
                    .iter()
                    .filter(|f| f.account_id == account)
                    .collect();
                let manual: Vec<_> = summary
                    .manual
                    .iter()
                    .filter(|m| m.account_id == account)
                    .collect();
                if funds.is_empty() && manual.is_empty() {
                    progress.reason_codes.push("scope_asset_missing".into());
                    return progress;
                }
                if funds.iter().any(|f| f.value_minor.is_none() || f.stale)
                    || manual.iter().any(|m| m.stale || m.overlap_review)
                {
                    progress.reason_codes.push("incomplete".into());
                    return progress;
                }
                for amount in funds
                    .iter()
                    .map(|f| f.value_minor.unwrap())
                    .chain(manual.iter().map(|m| m.market_value_minor))
                {
                    match add_minor(value, amount, "专项目标覆盖") {
                        Ok(v) => value = v,
                        Err(_) => {
                            progress.reason_codes.push("amount_overflow".into());
                            return progress;
                        }
                    }
                }
            }
            finish_progress(&mut progress, value, goal, summary);
        }
        other => {
            progress.reason_codes.push(format!("unknown_scope_{other}"));
        }
    }
    let _ = cash_minor;
    progress
}

fn cash_position_scope_reserved(conn: &Connection) -> Option<i64> {
    crate::finance::cash_position(conn).ok()?.reserved_minor
}

fn finish_progress(
    progress: &mut GoalProgress,
    value: i64,
    goal: &FinanceGoalRow,
    _summary: &crate::assets::AssetsSummary,
) {
    progress.scope_value_minor = Some(value);
    let target = goal.target_minor.max(1) as i128;
    let ratio = ((value as i128) * 1000 + target / 2) / target;
    let Ok(ratio) = i64::try_from(ratio) else {
        progress.reason_codes.push("amount_overflow".into());
        return;
    };
    progress.ratio_tenths = Some(ratio);
    // ppm 粒度（0.0001%），四舍五入；溢出时保持 None，前端回退 tenths。
    let ratio_ppm = ((value as i128) * 1_000_000 + target / 2) / target;
    if let Ok(ratio_ppm) = i64::try_from(ratio_ppm) {
        progress.ratio_ppm = Some(ratio_ppm);
    }
    progress.gap_minor = Some((goal.target_minor - value).max(0));
    progress.surplus_minor = Some((value - goal.target_minor).max(0));
    // 投入日历：默认每月末；已逾期不再计算月投入要求。
    if !progress.expired {
        let months = month_ends_until(&today_cn(), &goal.target_date);
        progress.remaining_investments = Some(months.len() as i64);
        let gap = progress.gap_minor.unwrap_or(0);
        if gap > 0 && !months.is_empty() {
            progress.required_monthly_minor =
                Some(gap / months.len() as i64 + i64::from(gap % months.len() as i64 != 0));
        }
    }
    progress.computable = true;
}

/// 北京时间今天（YYYY-MM-DD）；与 assets::civil_from_days 同口径（08h 偏移）。
fn today_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 8 * 3600;
    crate::assets::civil_from_days((secs / 86400) as i64)
}

#[cfg(test)]
#[path = "goals_tests.rs"]
mod tests;
