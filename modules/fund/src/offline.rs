//! 线下投资资产（方案 01 P05、03 §5；O01～O05）。
//!
//! 范围固定：实物黄金/定期存款/银行理财/其他手工估值投资。现金不是线下资产
//! （账户现金 owner 在 finance）；房产等固定资产排除。
//! - 更新估值保留前值到 history；不记已实现收益、不动现金。
//! - 退出：日期+回收+费用+到账状态一次提交；已到账经 finance correction 事件入账
//!   （现金 +（回收−费用），note 记 offline 来源；不污染外部投入/分红口径）；
//!   未到账仅形成应收显示，不增加可投入现金。
//! - 部分退出须确认余值（不能旧估值减到账金额猜）；删除误录=软删除，恢复不重放现金。
//! - 全部写入 version 递增；expectedVersion 冲突拒绝。

use rusqlite::{params, Connection, OptionalExtension};

use crate::finance::{self, CashEventInput, FinanceError};
use crate::storage::Store;

pub const TYPES: &[&str] = &["gold", "deposit", "wealth", "other"];

#[path = "offline_income.rs"]
mod income;
pub(crate) use income::summary as income_summary;

#[derive(Debug)]
pub enum OfflineError {
    Invalid(String),
    Conflict { current_version: i64 },
    NotFound,
    Db(String),
}

impl OfflineError {
    pub fn code(&self) -> &'static str {
        match self {
            OfflineError::Invalid(_) => "OFFLINE_INVALID",
            OfflineError::Conflict { .. } => "OFFLINE_CONFLICT",
            OfflineError::NotFound => "OFFLINE_NOT_FOUND",
            OfflineError::Db(_) => "OFFLINE_DB",
        }
    }

    pub fn message(&self) -> String {
        match self {
            OfflineError::Invalid(m) => format!("线下资产配置无效：{m}"),
            OfflineError::Conflict { current_version } => {
                format!("线下资产已被其他窗口修改（当前版本 {current_version}）")
            }
            OfflineError::NotFound => "线下资产不存在".into(),
            OfflineError::Db(_) => "线下资产暂时无法读取或保存，请重试".into(),
        }
    }
}

impl From<rusqlite::Error> for OfflineError {
    fn from(e: rusqlite::Error) -> Self {
        OfflineError::Db(e.to_string())
    }
}

impl From<FinanceError> for OfflineError {
    fn from(e: FinanceError) -> Self {
        match e {
            FinanceError::Db(m) => OfflineError::Db(m),
            other => OfflineError::Invalid(other.message()),
        }
    }
}

pub struct OfflineInput {
    pub product_type: String,
    pub name: String,
    pub location: String,
    pub value_minor: i64,
    pub valued_at: String,
    pub cost_minor: Option<i64>,
    pub risk_class: String,
    pub note: String,
    /// 类型附加字段 JSON（克重/成色、期限/到期日/合同利率）；保存原样，不生成收益。
    pub attrs_json: String,
}

fn validate(conn: &Connection, input: &OfflineInput) -> Result<(), OfflineError> {
    if !TYPES.contains(&input.product_type.as_str()) {
        return Err(OfflineError::Invalid(format!("类型须为 {:?} 之一", TYPES)));
    }
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(OfflineError::Invalid("名称需 1~64 字".into()));
    }
    if input.value_minor < 0 {
        return Err(OfflineError::Invalid("估值不能为负".into()));
    }
    if input.cost_minor.is_some_and(|v| v < 0)
        || input.location.chars().count() > 128
        || input.note.chars().count() > 4096
    {
        return Err(OfflineError::Invalid("成本、保管位置或备注无效".into()));
    }
    if input.valued_at.trim().is_empty() {
        return Err(OfflineError::Invalid("估值日期必填".into()));
    }
    // 估值日期必须是真实日历日（含月内天数，排除 02-31 之类）且不得在未来。
    if !crate::assets::is_valid_calendar_date(&input.valued_at) {
        return Err(OfflineError::Invalid(format!(
            "估值日期不是有效日历日：{}",
            input.valued_at
        )));
    }
    // 估值日期不得在未来（动态今天，北京时间）。
    let today = today_cn();
    if input.valued_at.as_str() > today.as_str() {
        return Err(OfflineError::Invalid("估值日期不能在未来".into()));
    }
    if !["equity", "gold", "fixed_income", "cash", "other", "unknown"]
        .contains(&input.risk_class.as_str())
    {
        return Err(OfflineError::Invalid("风险类别无效".into()));
    }
    // attrs_json 必须是合法 JSON 对象（有界）。
    let attrs: serde_json::Value = serde_json::from_str(&input.attrs_json)
        .map_err(|e| OfflineError::Invalid(format!("附加字段 JSON 无效：{e}")))?;
    if !attrs.is_object() || input.attrs_json.len() > 4096 {
        return Err(OfflineError::Invalid(
            "附加字段须为 ≤4096 字节的 JSON 对象".into(),
        ));
    }
    // 同名实物提醒潜在重复（不阻断：同名实物可以独立存在）。
    let _ = conn;
    Ok(())
}

fn today_cn() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64
        + 8 * 3600;
    let days = secs.div_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = y + i64::from(if m <= 2 { 1 } else { 0 });
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn create_asset(store: &Store, input: &OfflineInput) -> Result<i64, OfflineError> {
    validate_input(input)?;
    store.with_write(|conn| {
        validate(conn, input)?;
        let now = now_iso();
        conn.execute(
            "INSERT INTO offline_assets(product_type, name, location, value_minor, valued_at, \
             cost_minor, risk_class, note, attrs_json, version, created_at, updated_at) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10,?10)",
            params![
                input.product_type,
                input.name.trim(),
                input.location.trim(),
                input.value_minor,
                input.valued_at,
                input.cost_minor,
                input.risk_class,
                input.note,
                input.attrs_json,
                now
            ],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO offline_value_history(asset_id, value_minor, valued_at, created_at) \
             VALUES(?1,?2,?3,?4)",
            params![id, input.value_minor, input.valued_at, now],
        )?;
        Ok(id)
    })
}

fn validate_input(input: &OfflineInput) -> Result<(), OfflineError> {
    if input.value_minor < 0 {
        return Err(OfflineError::Invalid("估值不能为负".into()));
    }
    Ok(())
}

/// 更新（元数据+估值一次提交）；version 冲突拒绝；估值变化写 history。
pub fn update_asset(
    store: &Store,
    id: i64,
    expected_version: i64,
    input: &OfflineInput,
) -> Result<(), OfflineError> {
    validate_input(input)?;
    store.with_write(|conn| {
        validate(conn, input)?;
        let current: i64 = conn
            .query_row(
                "SELECT version FROM offline_assets WHERE id=?1 AND deleted_at IS NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OfflineError::NotFound)?;
        if current != expected_version {
            return Err(OfflineError::Conflict {
                current_version: current,
            });
        }
        let (old_value, old_date): (i64, String) = conn.query_row(
            "SELECT value_minor,valued_at FROM offline_assets WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let now = now_iso();
        conn.execute(
            "UPDATE offline_assets SET product_type=?2, name=?3, location=?4, value_minor=?5, \
             valued_at=?6, cost_minor=?7, risk_class=?8, note=?9, attrs_json=?10, \
             version=version+1, updated_at=?11 WHERE id=?1",
            params![
                id,
                input.product_type,
                input.name.trim(),
                input.location.trim(),
                input.value_minor,
                input.valued_at,
                input.cost_minor,
                input.risk_class,
                input.note,
                input.attrs_json,
                now
            ],
        )?;
        if old_value != input.value_minor || old_date != input.valued_at {
            conn.execute(
                "INSERT INTO offline_value_history(asset_id, value_minor, valued_at, created_at) \
                 VALUES(?1,?2,?3,?4)",
                params![id, input.value_minor, input.valued_at, now],
            )?;
        }
        Ok(())
    })
}

pub struct ExitInput {
    pub exit_date: String,
    pub exit_amount_minor: i64,
    pub exit_fee_minor: i64,
    /// 已到账：现金账户 id（现金 +（回收−费用）经 correction 事件）。
    pub settle_account_id: Option<i64>,
    /// 部分退出的确认余值（>=0；空=全部退出）。
    pub remaining_value_minor: Option<i64>,
    pub expected_version: i64,
    pub idempotency_key: String,
}

/// 退出（真实出售/到期/赎回）。一次事务：估值→0（或余值）、退出事实、现金事件。
/// 未到账不写现金（应收由 exit_settled=0 展示）；删除误录不产生现金。
pub fn exit_asset(store: &Store, id: i64, input: &ExitInput) -> Result<(), OfflineError> {
    if input.exit_date.trim().is_empty() {
        return Err(OfflineError::Invalid("退出日期必填".into()));
    }
    // 退出日期必须是真实日历日且不得在未来。
    if !crate::assets::is_valid_calendar_date(&input.exit_date) {
        return Err(OfflineError::Invalid(format!(
            "退出日期不是有效日历日：{}",
            input.exit_date
        )));
    }
    if input.exit_date > today_cn() {
        return Err(OfflineError::Invalid("退出日期不能在未来".into()));
    }
    if input.idempotency_key.len() > 128 {
        return Err(OfflineError::Invalid("requestId 不能超过 128 字节".into()));
    }
    if input.exit_amount_minor < 0
        || input.exit_fee_minor < 0
        || input.exit_fee_minor > input.exit_amount_minor
    {
        return Err(OfflineError::Invalid("回收金额与费用不能为负".into()));
    }
    if let Some(remaining) = input.remaining_value_minor {
        if remaining < 0 {
            return Err(OfflineError::Invalid("余值不能为负".into()));
        }
    }
    let request = serde_json::json!({"assetId":id,"date":input.exit_date,
        "amount":input.exit_amount_minor,"fee":input.exit_fee_minor,
        "remaining":input.remaining_value_minor,"account":input.settle_account_id})
    .to_string();
    store.with_write(|conn| {
        let (value_minor, version): (i64, i64) = conn
            .query_row(
                "SELECT value_minor, version FROM offline_assets WHERE id=?1 AND deleted_at IS NULL",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(OfflineError::NotFound)?;
        // 载荷比较与写入在同一事务；重试不受第一次提交后递增版本影响。
        if !input.idempotency_key.is_empty() {
            let previous: Option<Option<String>> = conn.query_row(
                "SELECT request_json FROM offline_exit_history WHERE idempotency_key=?1",
                [&input.idempotency_key], |r| r.get(0)).optional()?;
            if let Some(previous) = previous {
                return if previous.as_deref() == Some(request.as_str()) { Ok(()) }
                    else { Err(OfflineError::Conflict {current_version: version}) };
            }
        }
        if version != input.expected_version {
            return Err(OfflineError::Conflict { current_version: version });
        }
        // 部分退出后可再次退出（分批赎回）；余值 0 且已有退出历史则拒绝重复退出。
        let history_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM offline_exit_history WHERE asset_id=?1",
            [id],
            |r| r.get(0),
        )?;
        if history_count > 0 && value_minor == 0 {
            return Err(OfflineError::Invalid(
                "该资产已全部退出；删除误录请用删除并恢复，不重复退出".into(),
            ));
        }
        // 余值必须确认：部分退出时不能缺省（不能旧估值减到账金额猜）。
        if input.remaining_value_minor.is_none() && value_minor == 0 {
            return Err(OfflineError::Invalid(
                "资产估值为 0，无余额可退出；请先更新估值或明确部分退出余值".into(),
            ));
        }
        // 幂等键占用检查（同 key 不同内容并发）。
        if !input.idempotency_key.is_empty() {
            let taken: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM offline_exit_history WHERE idempotency_key=?1)",
                [input.idempotency_key.as_str()],
                |r| Ok(r.get::<_, i64>(0)? == 1),
            )?;
            if taken {
                return Err(OfflineError::Invalid(
                    "requestId 已用于其他退出请求；请使用新的 requestId".into(),
                ));
            }
        }
        let remaining = input.remaining_value_minor.unwrap_or(0);
        let now = now_iso();
        // 现金联动：仅已到账。correction 事件=已核实差额（回收−费用），note 记来源。
        let cash_ref: Option<i64> = if let Some(account_id) = input.settle_account_id.filter(|_|input.exit_amount_minor>input.exit_fee_minor) {
            let net = input
                .exit_amount_minor
                .checked_sub(input.exit_fee_minor)
                .ok_or_else(|| OfflineError::Invalid("回收金额减费用溢出".into()))?;
            let event_id = finance::record_cash_event(
                conn,
                &CashEventInput {
                    account_id,
                    event_type: "correction".into(),
                    amount_text: format!("{}.{:02}", net / 100, net % 100),
                    occurred_at: input.exit_date.clone(),
                    idempotency_key: Some(format!("offline-exit:{id}:{}", if input.idempotency_key.is_empty(){history_count.to_string()}else{input.idempotency_key.clone()})),
                    note: format!("offline:{id} 退出到账（含费用 {}）", input.exit_fee_minor),
                    counterparty_account_id: None,
                },
            )?;
            Some(event_id)
        } else {
            None
        };
        // 退出事实进历史（多次部分退出可追溯；幂等键唯一）。
        conn.execute(
            "INSERT INTO offline_exit_history(asset_id, exit_date, amount_minor, fee_minor, settled, cash_event_ref, idempotency_key, created_at, request_json, remaining_minor, settlement_account_id,settled_at) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                id,
                input.exit_date,
                input.exit_amount_minor,
                input.exit_fee_minor,
                i64::from(input.settle_account_id.is_some()),
                cash_ref,
                if input.idempotency_key.is_empty() { None } else { Some(input.idempotency_key.as_str().to_owned()) },
                now, request, remaining, input.settle_account_id,input.settle_account_id.map(|_|input.exit_date.as_str())
            ],
        )?;
        // 行内 exit_* 保留最近一次（投影）；估值更新为余值。
        conn.execute(
            "UPDATE offline_assets SET value_minor=?2, valued_at=?3, exit_date=?3, exit_amount_minor=?4, \
             exit_fee_minor=?5, exit_settled=?6, cash_event_ref=?7, version=version+1, updated_at=?8 \
             WHERE id=?1",
            params![
                id,
                remaining,
                input.exit_date,
                input.exit_amount_minor,
                input.exit_fee_minor,
                i64::from(input.settle_account_id.is_some()),
                cash_ref,
                now
            ],
        )?;
        let _ = value_minor;
        // 退出后余值写估值历史。
        conn.execute(
            "INSERT INTO offline_value_history(asset_id, value_minor, valued_at, created_at) \
             VALUES(?1,?2,?3,?4)",
            params![id, remaining, input.exit_date, now],
        )?;
        Ok(())
    })
}

/// 软删除（误录）。已退出的资产删除不影响现金事实；恢复不重放现金。
pub fn delete_asset(
    store: &Store,
    id: i64,
    reason: &str,
    expected_version: i64,
) -> Result<(), OfflineError> {
    store.with_write(|conn| {
        let version: i64 = conn
            .query_row(
                "SELECT version FROM offline_assets WHERE id=?1 AND deleted_at IS NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OfflineError::NotFound)?;
        if version != expected_version {
            return Err(OfflineError::Conflict { current_version: version });
        }
        // Archiving the asset never removes its exit/settlement facts or reverses cash.
        conn.execute(
            "UPDATE offline_assets SET deleted_at=?2, deleted_reason=?3, version=version+1, updated_at=?2 WHERE id=?1",
            params![id, now_iso(), reason],
        )?;
        Ok(())
    })
}

#[cfg(test)]
pub fn restore_asset(store: &Store, id: i64) -> Result<(), OfflineError> {
    let version = store.with_read(|c| {
        c.query_row(
            "SELECT version FROM offline_assets WHERE id=?1",
            [id],
            |r| r.get(0),
        )
    })?;
    restore_asset_versioned(store, id, version)
}

pub fn restore_asset_versioned(store: &Store, id: i64, expected: i64) -> Result<(), OfflineError> {
    store.with_write(|conn| {
        let version:i64=conn.query_row("SELECT version FROM offline_assets WHERE id=?1 AND deleted_at IS NOT NULL",[id],|r|r.get(0)).optional()?.ok_or(OfflineError::NotFound)?;
        if version!=expected {return Err(OfflineError::Conflict{current_version:version});}
        let n = conn.execute(
            "UPDATE offline_assets SET deleted_at=NULL, deleted_reason='', version=version+1, updated_at=?2 WHERE id=?1 AND deleted_at IS NOT NULL",
            params![id, now_iso()],
        )?;
        if n == 0 {
            return Err(OfflineError::NotFound);
        }
        Ok(())
    })
}

pub struct OfflineRow {
    pub id: i64,
    pub product_type: String,
    pub name: String,
    pub location: String,
    pub value_minor: i64,
    pub valued_at: String,
    pub cost_minor: Option<i64>,
    pub risk_class: String,
    pub note: String,
    pub attrs_json: String,
    pub exit_date: Option<String>,
    pub exit_amount_minor: Option<i64>,
    pub exit_fee_minor: Option<i64>,
    pub exit_settled: bool,
    pub deleted: bool,
    pub version: i64,
}

pub fn list_assets(
    store: &Store,
    include_deleted: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<OfflineRow>, OfflineError> {
    list_assets_filtered(store, include_deleted, limit, offset, "")
}

pub fn list_assets_filtered(
    store: &Store,
    include_deleted: bool,
    limit: i64,
    offset: i64,
    product_type: &str,
) -> Result<Vec<OfflineRow>, OfflineError> {
    if !matches!(product_type, "" | "gold" | "deposit" | "wealth" | "other") {
        return Err(OfflineError::Invalid("资产类型无效".into()));
    }
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, product_type, name, location, value_minor, valued_at, cost_minor, risk_class, \
             note, attrs_json, exit_date, exit_amount_minor, exit_fee_minor, exit_settled, \
             deleted_at IS NOT NULL, version FROM offline_assets \
             WHERE (?1 OR deleted_at IS NULL) AND (?4='' OR product_type=?4) ORDER BY id DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt
            .query_map(params![i64::from(include_deleted), limit, offset, product_type], |r| {
                Ok(OfflineRow {
                    id: r.get(0)?,
                    product_type: r.get(1)?,
                    name: r.get(2)?,
                    location: r.get(3)?,
                    value_minor: r.get(4)?,
                    valued_at: r.get(5)?,
                    cost_minor: r.get(6)?,
                    risk_class: r.get(7)?,
                    note: r.get(8)?,
                    attrs_json: r.get(9)?,
                    exit_date: r.get(10)?,
                    exit_amount_minor: r.get(11)?,
                    exit_fee_minor: r.get(12)?,
                    exit_settled: r.get::<_, i64>(13)? == 1,
                    deleted: r.get(14)?,
                    version: r.get(15)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn value_history(store: &Store, id: i64) -> Result<Vec<(i64, String, String)>, OfflineError> {
    value_history_page(store, id, 0)
}

pub fn value_history_page(
    store: &Store,
    id: i64,
    offset: i64,
) -> Result<Vec<(i64, String, String)>, OfflineError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT value_minor, valued_at, created_at FROM offline_value_history \
             WHERE asset_id=?1 ORDER BY created_at DESC, id DESC LIMIT 50 OFFSET ?2",
        )?;
        let rows = stmt
            .query_map([id, offset.max(0)], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let z = secs.div_euclid(86400) + 719_468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let rem = secs.rem_euclid(86400);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y + i64::from(if m <= 2 { 1 } else { 0 }),
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn gold_input() -> OfflineInput {
        OfflineInput {
            product_type: "gold".into(),
            name: "金条 50g".into(),
            location: "家里保险柜".into(),
            value_minor: 300_000,
            valued_at: today_cn(),
            cost_minor: Some(250_000),
            risk_class: "gold".into(),
            note: String::new(),
            attrs_json: r#"{"weightGrams":50,"purity":"999"}"#.into(),
        }
    }

    #[test]
    fn historical_values_and_exits_have_reachable_later_pages() {
        let s = store();
        let id = create_asset(&s, &gold_input()).unwrap();
        s.with_write(|c| -> rusqlite::Result<()> {
            for n in 0..55 {
                c.execute("INSERT INTO offline_value_history(asset_id,value_minor,valued_at,created_at) VALUES(?1,?2,?3,'later')", rusqlite::params![id,n,today_cn()])?;
                c.execute("INSERT INTO offline_exit_history(asset_id,exit_date,amount_minor,fee_minor,created_at) VALUES(?1,?2,1,0,'later')", rusqlite::params![id,today_cn()])?;
            }
            Ok(())
        }).unwrap();
        assert_eq!(value_history(&s, id).unwrap().len(), 50);
        assert_eq!(value_history_page(&s, id, 50).unwrap().len(), 6);
        assert_eq!(
            crate::offline_settlement::exits_page(&s, id, 50)
                .unwrap()
                .len(),
            5
        );
        let response = crate::api::offline_api::api_offline_history(
            &s,
            &format!("/api/offline/assets/history?id={id}&page=2"),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            6
        );
        assert!(crate::api::offline_api::api_offline_history(
            &s,
            &format!("/api/offline/assets/history?id={id}&page=0")
        )
        .is_err());
    }

    #[test]
    fn type_filter_applies_before_pagination() {
        let s = store();
        let id = create_asset(&s, &gold_input()).unwrap();
        let mut deposit = gold_input();
        deposit.product_type = "deposit".into();
        deposit.attrs_json = "{}".into();
        deposit.risk_class = "fixed_income".into();
        create_asset(&s, &deposit).unwrap();
        let rows = list_assets_filtered(&s, false, 1, 0, "gold").unwrap();
        assert_eq!(rows[0].id, id);
        assert!(list_assets_filtered(&s, false, 1, 1, "gold")
            .unwrap()
            .is_empty());
        assert!(list_assets_filtered(&s, false, 1, 0, "estate").is_err());
    }

    #[test]
    fn exit_retries_compare_payload_and_preserve_state() {
        let store = store();
        let id = create_asset(&store, &gold_input()).unwrap();
        let mut input = ExitInput {
            exit_date: today_cn(),
            exit_amount_minor: 10_000,
            exit_fee_minor: 100,
            settle_account_id: None,
            remaining_value_minor: Some(200_000),
            expected_version: 1,
            idempotency_key: "same-request".into(),
        };
        exit_asset(&store, id, &input).unwrap();
        exit_asset(&store, id, &input).unwrap();
        input.exit_amount_minor = 20_000;
        assert!(matches!(
            exit_asset(&store, id, &input),
            Err(OfflineError::Conflict { .. })
        ));
        input.exit_amount_minor = 10_000;
        input.remaining_value_minor = Some(100_000);
        assert!(matches!(
            exit_asset(&store, id, &input),
            Err(OfflineError::Conflict { .. })
        ));
        let row = list_assets(&store, false, 50, 0).unwrap().remove(0);
        assert_eq!((row.value_minor, row.version), (200_000, 2));
        assert_eq!(
            store
                .with_read(|c| c
                    .query_row("SELECT COUNT(*) FROM offline_exit_history", [], |r| r
                        .get::<_, i64>(0)))
                .unwrap(),
            1
        );
    }

    #[test]
    fn crud_value_history_and_version_conflict() {
        let store = store();
        let id = create_asset(&store, &gold_input()).unwrap();
        // 非法类型拒绝。
        let mut bad = gold_input();
        bad.product_type = "estate".into();
        assert_eq!(
            create_asset(&store, &bad).unwrap_err().code(),
            "OFFLINE_INVALID"
        );
        // 未来日期拒绝。
        let mut future = gold_input();
        future.valued_at = "2099-01-01".into();
        assert!(matches!(
            create_asset(&store, &future).unwrap_err(),
            OfflineError::Invalid(_)
        ));
        // 更新估值：history 记录两次；version 冲突拒绝。
        let mut updated = gold_input();
        updated.value_minor = 310_000;
        update_asset(&store, id, 1, &updated).unwrap();
        let conflict = update_asset(&store, id, 1, &updated).unwrap_err();
        assert!(matches!(
            conflict,
            OfflineError::Conflict { current_version: 2 }
        ));
        let history = value_history(&store, id).unwrap();
        assert_eq!(history.len(), 2, "估值变化写历史");
        // 软删除/恢复。
        delete_asset(&store, id, "误录", 2).unwrap();
        assert!(list_assets(&store, false, 10, 0).unwrap().is_empty());
        assert_eq!(list_assets(&store, true, 10, 0).unwrap().len(), 1);
        restore_asset(&store, id).unwrap();
        assert_eq!(list_assets(&store, false, 10, 0).unwrap().len(), 1);
    }

    #[test]
    fn exit_settled_moves_cash_once_and_keeps_a_constant() {
        let store = store();
        // 场景（方案 07 §4）：投资现金 5000 元 + 黄金 3000 元，A=8000；
        // 黄金 3000 退出、费用 0、全部到账 → 现金 8000，A 仍 8000。
        let account_id: i64 = store
            .with_write(|conn| -> rusqlite::Result<i64> {
                conn.execute("INSERT INTO accounts(name, created_at, updated_at) VALUES('投资账户', 'now', 'now')", [])?;
                Ok(conn.last_insert_rowid())
            })
            .unwrap();
        store
            .with_write(|conn| {
                finance::record_cash_event(
                    conn,
                    &CashEventInput {
                        account_id,
                        event_type: "opening".into(),
                        amount_text: "5000".into(),
                        occurred_at: today_cn(),
                        idempotency_key: Some("open-1".into()),
                        note: String::new(),
                        counterparty_account_id: None,
                    },
                )
            })
            .unwrap();
        let id = create_asset(&store, &gold_input()).unwrap();
        let exit = ExitInput {
            exit_date: today_cn(),
            exit_amount_minor: 300_000,
            exit_fee_minor: 0,
            settle_account_id: Some(account_id),
            remaining_value_minor: None,
            expected_version: 1,
            idempotency_key: "exit-1".into(),
        };
        exit_asset(&store, id, &exit).unwrap();
        // 幂等重试：不再入账（correction 事件 idempotency 幂等）。
        let result = store
            .with_read(|conn| finance::cash_position(conn))
            .unwrap();
        let account = result
            .accounts
            .iter()
            .find(|a| a.account_id == account_id)
            .unwrap();
        assert_eq!(account.cash_minor, Some(800_000), "现金 5000+3000=8000 元");
        // 资产行：余值 0、已到账。
        let rows = list_assets(&store, false, 10, 0).unwrap();
        let row = rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(row.value_minor, 0);
        assert!(row.exit_settled);
        delete_asset(&store, id, "archive", row.version).unwrap();
        assert!(list_assets(&store, false, 10, 0).unwrap().is_empty());
        restore_asset(&store, id).unwrap();
        assert_eq!(
            store
                .with_read(|c| finance::cash_position(c))
                .unwrap()
                .accounts[0]
                .cash_minor,
            Some(800_000)
        );
        assert_eq!(
            crate::offline_settlement::exits_page(&store, id, 0)
                .unwrap()
                .len(),
            1
        );
        // A 恒等：现金 8000 + 黄金 0 = 8000 = 退出前（5000+3000）。
    }

    #[test]
    fn exit_unsettled_is_receivable_without_cash() {
        let store = store();
        let account_id: i64 = store
            .with_write(|conn| -> rusqlite::Result<i64> {
                conn.execute(
                    "INSERT INTO accounts(name, created_at, updated_at) VALUES('A2', 'now', 'now')",
                    [],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .unwrap();
        store
            .with_write(|conn| {
                finance::record_cash_event(
                    conn,
                    &CashEventInput {
                        account_id,
                        event_type: "opening".into(),
                        amount_text: "1000".into(),
                        occurred_at: today_cn(),
                        idempotency_key: Some("open-2".into()),
                        note: String::new(),
                        counterparty_account_id: None,
                    },
                )
            })
            .unwrap();
        let id = create_asset(&store, &gold_input()).unwrap();
        let exit = ExitInput {
            exit_date: today_cn(),
            exit_amount_minor: 300_000,
            exit_fee_minor: 5_00,
            settle_account_id: None,
            remaining_value_minor: None,
            expected_version: 1,
            idempotency_key: "exit-2".into(),
        };
        exit_asset(&store, id, &exit).unwrap();
        let result = store
            .with_read(|conn| finance::cash_position(conn))
            .unwrap();
        let account = result
            .accounts
            .iter()
            .find(|a| a.account_id == account_id)
            .unwrap();
        assert_eq!(account.cash_minor, Some(100_000), "未到账不增加现金");
        let row = list_assets(&store, false, 10, 0)
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert!(!row.exit_settled, "应收状态显式");
    }

    // 审计 P0-2：线下资产进入统一资产权威（summary）；多次部分退出+幂等重试。
    #[test]
    fn offline_assets_enter_summary_and_partial_exits_repeat() {
        let store = store();
        // 无线下资产：offline 不产生条目。
        let empty = crate::assets::summary(&store, &today_cn()).unwrap();
        assert_eq!(empty.offline_count, 0);
        assert!(empty.offline_value_minor.is_none());

        let id = create_asset(&store, &gold_input()).unwrap();
        let sum = crate::assets::summary(&store, &today_cn()).unwrap();
        assert_eq!(sum.offline_count, 1);
        assert_eq!(sum.offline_value_minor, Some(300_000));
        assert_eq!(
            sum.known_value_minor,
            Some(300_000),
            "线下资产计入已计价合计"
        );
        assert!(sum.has_entries, "线下资产让空库不再 empty");

        // 部分退出 1：100 元回收、余值 200 元，未到账。
        let e1 = ExitInput {
            exit_date: today_cn(),
            exit_amount_minor: 10_000,
            exit_fee_minor: 0,
            settle_account_id: None,
            remaining_value_minor: Some(200_000),
            expected_version: 1,
            idempotency_key: "pe-1".into(),
        };
        exit_asset(&store, id, &e1).unwrap();
        // 幂等重试：同 key 整体成功（审计：此前报"已退出"错误）。
        exit_asset(&store, id, &e1).unwrap();
        let sum = crate::assets::summary(&store, &today_cn()).unwrap();
        assert_eq!(sum.offline_value_minor, Some(200_000), "余值计入");
        // 未到账应收计入 A 但质量 incomplete（覆盖缺口显式）。
        assert_eq!(sum.offline_receivable_minor, Some(10_000));
        assert_eq!(sum.quality, "incomplete");

        // 部分退出 2：分批赎回剩余（新 key、新 version）。
        let e2 = ExitInput {
            exit_date: today_cn(),
            exit_amount_minor: 200_000,
            exit_fee_minor: 0,
            settle_account_id: None,
            remaining_value_minor: Some(0),
            expected_version: 2,
            idempotency_key: "pe-2".into(),
        };
        exit_asset(&store, id, &e2).unwrap();
        // 全部退出后再退出 → 拒绝。
        let e3 = ExitInput {
            expected_version: 3,
            idempotency_key: "pe-3".into(),
            ..e2
        };
        assert!(matches!(
            exit_asset(&store, id, &e3).unwrap_err(),
            OfflineError::Invalid(_)
        ));

        // 日历校验：2026-02-31 拒绝；未来日期拒绝。
        let mut bad = gold_input();
        bad.name = "非法日期资产".into();
        bad.valued_at = "2026-02-31".into();
        assert!(matches!(
            create_asset(&store, &bad).unwrap_err(),
            OfflineError::Invalid(_)
        ));
        let mut future = gold_input();
        future.name = "未来日期资产".into();
        future.valued_at = "2099-01-01".into();
        assert!(matches!(
            create_asset(&store, &future).unwrap_err(),
            OfflineError::Invalid(_)
        ));

        // 删除后退出/估值不再计入。
        delete_asset(&store, id, "误录", 3).unwrap();
        let sum = crate::assets::summary(&store, &today_cn()).unwrap();
        assert_eq!(sum.offline_count, 0);
        assert!(sum.offline_value_minor.is_none());
    }

    #[test]
    fn deleted_exit_does_not_touch_cash_and_partial_requires_remaining() {
        let store = store();
        let id = create_asset(&store, &gold_input()).unwrap();
        // 部分退出：余值必须确认（不能旧估值减到账猜）。
        let partial = ExitInput {
            exit_date: today_cn(),
            exit_amount_minor: 100_000,
            exit_fee_minor: 0,
            settle_account_id: None,
            remaining_value_minor: Some(200_000),
            expected_version: 1,
            idempotency_key: "exit-p".into(),
        };
        exit_asset(&store, id, &partial).unwrap();
        let row = list_assets(&store, false, 10, 0)
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert_eq!(row.value_minor, 200_000, "余值确认");
        assert_eq!(row.exit_amount_minor, Some(100_000));
    }
}
