//! import 模块：CSV 导入（实施方案 §7.3，模板版本 1）。
//!
//! 流程：解析 → 校验 → 预览（不入库）→ 确认 → 单事务提交 → 结果收据。
//! - 只支持 UTF-8 CSV（可有 BOM，双引号转义）；文件 ≤ 5 MiB、行 ≤ 20,000、
//!   单字段 ≤ 4 KiB（大小限制由 host 壳执行，这里校验行数与字段）。
//! - 模板「已确认交易」：account,fund_code,trade_date,type,quantity,price
//!   [+amount,fee,external_id]；amount 缺省 = quantity×price。
//! - 模板「期初持仓快照」：account,fund_code,as_of,quantity
//!   [+cost_amount,market_value,earnings,nav,nav_date]；仅用于尚无流水的
//!   建账；成本缺失可由市值−收益推算但显式标记 derived，无法推算保留未知。
//! - 幂等身份：import_runs UNIQUE(source_id, template_version, file_sha256,
//!   params_hash) —— 精确重复返回原结果不再次入账。
//! - 行身份：有 external_id 以 (account_id, external_id) 去重；无则批次+行号
//!   +内容摘要，仅保证同一确认批次重试幂等。

use crate::fixed::{Fixed, SCALE_SHARE};
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub const TEMPLATE_TX: i64 = 1;
pub const TEMPLATE_SNAPSHOT: i64 = 2;
pub const MAX_ROWS: usize = 20_000;
pub const MAX_FIELD_BYTES: usize = 4 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    /// 解析/校验失败（含行号与原因列表已在预览结果中）。
    Invalid(String),
    /// 快照模板用于已有流水的账户/基金：冲突，不覆写。
    SnapshotConflict(String),
    /// 同批次身份但内容不同。
    BatchConflict(String),
    Db(String),
}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        ImportError::Db(e.to_string())
    }
}

impl From<crate::ledger::LedgerError> for ImportError {
    fn from(e: crate::ledger::LedgerError) -> Self {
        match e {
            crate::ledger::LedgerError::Oversell { available_raw } => {
                ImportError::Invalid(format!("可卖份额不足：当前 {}", Fixed::nav(available_raw)))
            }
            crate::ledger::LedgerError::Conflict(m) => ImportError::BatchConflict(m),
            crate::ledger::LedgerError::NotFound(m) => ImportError::Invalid(format!("不存在: {m}")),
            crate::ledger::LedgerError::Invalid(m) => ImportError::Invalid(m),
            crate::ledger::LedgerError::Db(m) => ImportError::Db(m),
        }
    }
}

impl From<crate::portfolio::PortfolioError> for ImportError {
    fn from(e: crate::portfolio::PortfolioError) -> Self {
        match e {
            crate::portfolio::PortfolioError::Oversell {
                available_raw,
                requested_raw,
                date,
            } => ImportError::Invalid(format!(
                "时序超卖：日期 {date} 请求卖出 {} 份，可用 {} 份",
                Fixed::nav(requested_raw),
                Fixed::nav(available_raw)
            )),
            crate::portfolio::PortfolioError::Db(e) => ImportError::Db(e),
        }
    }
}

impl ImportError {
    pub fn code(&self) -> &'static str {
        match self {
            ImportError::Invalid(_) => "IMPORT_INVALID",
            ImportError::SnapshotConflict(_) => "IMPORT_SNAPSHOT_CONFLICT",
            ImportError::BatchConflict(_) => "IMPORT_BATCH_CONFLICT",
            ImportError::Db(_) => "IMPORT_DB",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ImportError::Invalid(m) => format!("导入失败：{m}"),
            ImportError::SnapshotConflict(m) => {
                format!("期初快照冲突（已有记账历史）：{m}")
            }
            ImportError::BatchConflict(m) => format!("导入批次冲突：{m}"),
            ImportError::Db(e) => format!("数据库错误：{e}"),
        }
    }
}

/// 单行预览/结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportLine {
    pub line_no: usize,
    pub account: String,
    pub fund_code: String,
    /// BUY / SELL / SNAPSHOT
    pub kind: String,
    pub trade_date: String,
    pub quantity_raw: i64,
    pub price_raw: Option<i64>,
    pub amount_raw: Option<i64>,
    pub fee_raw: i64,
    pub external_id: Option<String>,
    /// 快照推算成本标记（derived）或成本未知（unknown）。
    pub cost_state: String,
    pub error: Option<String>,
}

/// 预览结果（不入库）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportPreview {
    pub template_version: i64,
    pub total_rows: usize,
    pub valid_rows: usize,
    pub error_rows: usize,
    pub lines: Vec<ImportLine>,
}

/// 提交结果收据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ImportReceipt {
    pub source_id: String,
    pub template_version: i64,
    pub file_sha256: String,
    pub params_hash: String,
    pub accounts_created: usize,
    pub funds_created: usize,
    pub rows_imported: usize,
    pub rows_skipped: usize,
    /// 重复批次命中：返回原结果，未再次入账。
    pub replayed: bool,
}

/// CSV 最小解析器：RFC 4180 双引号转义；支持 BOM；字段数不要求一致由调用方校验。
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, ImportError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut field = String::new();
    let mut row: Vec<String> = Vec::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                if field.len() + c.len_utf8() > MAX_FIELD_BYTES {
                    return Err(ImportError::Invalid("字段超过 4KiB".into()));
                }
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            ',' => {
                row.push(std::mem::take(&mut field));
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => {
                if field.len() + c.len_utf8() > MAX_FIELD_BYTES {
                    return Err(ImportError::Invalid("字段超过 4KiB".into()));
                }
                field.push(c);
            }
        }
    }
    if in_quotes {
        return Err(ImportError::Invalid("引号未闭合".into()));
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    // 去掉末尾全空行。
    while rows.last().is_some_and(|r| r.iter().all(|f| f.is_empty())) {
        rows.pop();
    }
    Ok(rows)
}

fn validate_date(date: &str) -> Result<(), String> {
    if date.len() == 10
        && date.as_bytes()[4] == b'-'
        && date.as_bytes()[7] == b'-'
        && date.chars().all(|c| c.is_ascii_digit() || c == '-')
    {
        Ok(())
    } else {
        Err(format!("日期 {date} 须为 YYYY-MM-DD"))
    }
}

fn col(row: &[String], index: usize) -> Option<&str> {
    row.get(index).map(|s| s.trim()).filter(|s| !s.is_empty())
}

/// 解析并校验 CSV（预览，不入库）。
pub fn preview(template_version: i64, csv_text: &str) -> Result<ImportPreview, ImportError> {
    let rows = parse_csv(csv_text)?;
    if rows.is_empty() {
        return Err(ImportError::Invalid("空文件".into()));
    }
    if rows.len() - 1 > MAX_ROWS {
        return Err(ImportError::Invalid(format!("数据行超过 {MAX_ROWS}")));
    }
    let header: Vec<String> = rows[0].iter().map(|s| s.trim().to_string()).collect();
    let lines: Vec<ImportLine> = rows[1..]
        .iter()
        .enumerate()
        .map(|(i, row)| parse_line(template_version, &header, row, i + 2))
        .collect();
    let valid = lines.iter().filter(|l| l.error.is_none()).count();
    Ok(ImportPreview {
        template_version,
        total_rows: lines.len(),
        valid_rows: valid,
        error_rows: lines.len() - valid,
        lines,
    })
}

fn parse_line(
    template_version: i64,
    header: &[String],
    row: &[String],
    line_no: usize,
) -> ImportLine {
    let get = |name: &str| -> Option<&str> {
        header
            .iter()
            .position(|h| h == name)
            .and_then(|i| col(row, i))
    };
    let mut line = ImportLine {
        line_no,
        account: get("account").unwrap_or("").to_string(),
        fund_code: get("fund_code").unwrap_or("").to_string(),
        kind: if template_version == TEMPLATE_SNAPSHOT {
            "SNAPSHOT".into()
        } else {
            get("type").unwrap_or("").to_uppercase()
        },
        trade_date: get(if template_version == TEMPLATE_SNAPSHOT {
            "as_of"
        } else {
            "trade_date"
        })
        .unwrap_or("")
        .to_string(),
        quantity_raw: 0,
        price_raw: None,
        amount_raw: None,
        fee_raw: 0,
        external_id: get("external_id").map(|s| s.to_string()),
        cost_state: String::new(),
        error: None,
    };
    let fail = |line: &mut ImportLine, message: String| {
        line.error = Some(message);
        line.clone()
    };
    if line.account.is_empty() {
        return fail(&mut line, "account 缺失".into());
    }
    if line.fund_code.len() != 6 || !line.fund_code.chars().all(|c| c.is_ascii_digit()) {
        let code = line.fund_code.clone();
        return fail(&mut line, format!("fund_code {code} 须为 6 位数字"));
    }
    if let Err(e) = validate_date(&line.trade_date) {
        return fail(&mut line, e);
    }
    let quantity = match get("quantity").map(|s| Fixed::parse(s, SCALE_SHARE)) {
        Some(Ok(q)) if !q.is_negative() && !q.is_zero() => q,
        Some(Ok(_)) => return fail(&mut line, "quantity 必须为正".into()),
        Some(Err(_)) | None => return fail(&mut line, "quantity 非法或缺失".into()),
    };
    line.quantity_raw = quantity.raw();

    if template_version == TEMPLATE_SNAPSHOT {
        // 期初快照：成本 > 市值−收益推算 > 未知。
        let cost = get("cost_amount").map(|s| Fixed::parse(s, 2));
        let market = get("market_value").map(|s| Fixed::parse(s, 2));
        let earnings = get("earnings").map(|s| Fixed::parse(s, 2));
        match cost {
            Some(Ok(c)) => {
                line.amount_raw = Some(c.raw());
                line.cost_state = "explicit".into();
            }
            Some(Err(_)) => return fail(&mut line, "cost_amount 非法".into()),
            None => match (market, earnings) {
                (Some(Ok(m)), Some(Ok(e))) => {
                    let derived = m.checked_sub(&e);
                    match derived {
                        Some(d) if !d.is_negative() => {
                            line.amount_raw = Some(d.raw());
                            line.cost_state = "derived".into();
                        }
                        _ => {
                            line.cost_state = "unknown".into();
                        }
                    }
                }
                (Some(Err(_)), _) => return fail(&mut line, "market_value 非法".into()),
                (_, Some(Err(_))) => return fail(&mut line, "earnings 非法".into()),
                _ => {
                    line.cost_state = "unknown".into();
                }
            },
        }
        return line;
    }

    // 交易模板。
    if line.kind != "BUY" && line.kind != "SELL" {
        let kind = line.kind.clone();
        return fail(&mut line, format!("type {kind} 须为 BUY/SELL"));
    }
    match get("price").map(|s| Fixed::parse(s, SCALE_SHARE)) {
        Some(Ok(p)) => line.price_raw = Some(p.raw()),
        _ => return fail(&mut line, "price 非法或缺失".into()),
    }
    match get("fee").map(|s| Fixed::parse(s, 2)) {
        Some(Ok(f)) if !f.is_negative() => line.fee_raw = f.raw(),
        Some(Ok(_)) => return fail(&mut line, "fee 不能为负".into()),
        Some(Err(_)) => return fail(&mut line, "fee 非法".into()),
        None => line.fee_raw = 0,
    }
    match get("amount").map(|s| Fixed::parse(s, 2)) {
        Some(Ok(a)) => line.amount_raw = Some(a.raw()),
        Some(Err(_)) => return fail(&mut line, "amount 非法".into()),
        None => {
            // amount = quantity × price（ROUND_DOWN 2 位）。
            let price = Fixed::nav(line.price_raw.unwrap_or(0));
            line.amount_raw = quantity.checked_mul_to(&price, 2).map(|f| f.raw());
        }
    }
    line
}

/// 确认提交（单事务）；精确重复批次返回原结果（replayed=true）。
/// 预览中的 error 行不提交。
/// 遇任何一行校验失败或时序超卖，整批原子回滚。
pub fn commit(
    store: &Store,
    source_id: &str,
    template_version: i64,
    file_sha256: &str,
    params_hash: &str,
    preview: &ImportPreview,
) -> Result<ImportReceipt, ImportError> {
    let valid: Vec<&ImportLine> = preview.lines.iter().filter(|l| l.error.is_none()).collect();
    store
        .with_write(|conn| -> Result<ImportReceipt, ImportError> {
            // 精确重复批次：返回原结果。
            let existing: Option<String> = conn
                .query_row(
                    "SELECT result_json FROM import_runs
                     WHERE source_id=?1 AND template_version=?2 AND file_sha256=?3 AND params_hash=?4",
                    params![source_id, template_version, file_sha256, params_hash],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(json) = existing {
                let receipt: ImportReceipt = serde_json::from_str(&json)
                    .map_err(|e| ImportError::BatchConflict(format!("旧收据不可读: {e}")))?;
                return Ok(ImportReceipt {
                    replayed: true,
                    ..receipt
                });
            }

            let initial_accounts: usize =
                conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
            let initial_funds: usize =
                conn.query_row("SELECT COUNT(*) FROM funds", [], |r| r.get(0))?;

            let mut rows_imported = 0usize;
            let mut rows_skipped = 0usize;

            for line in &valid {
                if template_version == TEMPLATE_SNAPSHOT {
                    import_snapshot_line(conn, line, &mut rows_imported, &mut rows_skipped)?;
                } else {
                    import_tx_line(conn, line, source_id, &mut rows_imported, &mut rows_skipped)?;
                }
            }

            let final_accounts: usize =
                conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
            let final_funds: usize =
                conn.query_row("SELECT COUNT(*) FROM funds", [], |r| r.get(0))?;
            let accounts_created = final_accounts.saturating_sub(initial_accounts);
            let funds_created = final_funds.saturating_sub(initial_funds);

            let receipt = ImportReceipt {
                source_id: source_id.to_string(),
                template_version,
                file_sha256: file_sha256.to_string(),
                params_hash: params_hash.to_string(),
                accounts_created,
                funds_created,
                rows_imported,
                rows_skipped,
                replayed: false,
            };
            conn.execute(
                "INSERT INTO import_runs(source_id, template_version, file_sha256, params_hash, state, result_json, created_at)
                 VALUES(?1,?2,?3,?4,'committed',?5,?6)",
                params![
                    source_id,
                    template_version,
                    file_sha256,
                    params_hash,
                    serde_json::to_string(&receipt)
                        .map_err(|e| ImportError::Db(e.to_string()))?,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().to_string())
                        .unwrap_or_default(),
                ],
            )?;
            Ok(receipt)
        })
}

fn import_tx_line(
    conn: &Connection,
    line: &ImportLine,
    source_id: &str,
    imported: &mut usize,
    skipped: &mut usize,
) -> Result<(), ImportError> {
    // 构造 payload 与 SHA-256 摘要
    let payload = format!(
        "{source_id}|{}|{}|{}|{}|{}",
        line.line_no,
        line.kind,
        line.quantity_raw,
        line.price_raw.unwrap_or(0),
        line.trade_date
    );
    let payload_sha256 = app_runtime_core::sha256_hex(payload.as_bytes());
    // 无 external_id 时用批次+SHA-256 摘要做幂等键。
    let external_id = line
        .external_id
        .clone()
        .or_else(|| Some(format!("batch:{source_id}:{payload_sha256}")));

    let input = crate::ledger::TransactionInput {
        account_name: line.account.clone(),
        fund_code: line.fund_code.clone(),
        fund_name: String::new(),
        tx_type: line.kind.clone(),
        state: crate::ledger::STATE_CONFIRMED.to_string(),
        quantity: Fixed::nav(line.quantity_raw),
        price: Fixed::nav(line.price_raw.unwrap_or(0)),
        fee: Fixed::amount(line.fee_raw),
        amount: line.amount_raw.map(Fixed::amount),
        trade_date: line.trade_date.clone(),
        source: "import".to_string(),
        request_id: None,
        external_id,
        batch_id: Some(source_id.to_string()),
        line_no: Some(line.line_no as i64),
        payload_hash: Some(payload_sha256),
    };

    let (_tx_id, created) = crate::ledger::record_transaction_tx(conn, &input)?;
    if created {
        *imported += 1;
    } else {
        *skipped += 1;
    }
    Ok(())
}

fn import_snapshot_line(
    conn: &Connection,
    line: &ImportLine,
    imported: &mut usize,
    skipped: &mut usize,
) -> Result<(), ImportError> {
    let account_id = crate::ledger::ensure_account(conn, &line.account)?;
    let fund_id = crate::ledger::ensure_fund(conn, &line.fund_code, "")?;

    // 仅用于尚无该账户/基金记账历史的建账（实施方案 §7.3）。
    let tx_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE account_id=?1 AND fund_id=?2",
        params![account_id, fund_id],
        |r| r.get(0),
    )?;
    if tx_count > 0 {
        return Err(ImportError::SnapshotConflict(format!(
            "账户 {account_id} 基金 {} 已有 {tx_count} 条流水",
            line.fund_code
        )));
    }
    let existing: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2",
            params![account_id, fund_id],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        *skipped += 1;
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO opening_snapshots(account_id, fund_id, as_of, quantity_raw, cost_raw, derived_cost, nav_raw, nav_date, created_at)
         VALUES(?1,?2,?3,?4,?5,0,NULL,NULL,?6)",
        params![
            account_id,
            fund_id,
            line.trade_date,
            line.quantity_raw,
            line.amount_raw,
            now,
        ],
    )?;
    crate::portfolio::recompute_position(conn, account_id, fund_id)?;
    *imported += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TX_CSV: &str = "account,fund_code,trade_date,type,quantity,price,fee,external_id\n\
        默认账户,000001,2026-01-05,BUY,100,1.00,1.00,ext-1\n\
        默认账户,000001,2026-01-06,BUY,50,1.20,0.50,ext-2\n\
        默认账户,000001,2026-01-07,SELL,60,1.30,0.50,ext-3\n";

    const SNAP_CSV: &str = "account,fund_code,as_of,quantity,market_value,earnings\n\
        券商,000009,2026-01-01,200,215.00,15.00\n\
        券商,000010,2026-01-01,100\n";

    #[test]
    fn preview_tx_csv_valid() {
        let p = preview(TEMPLATE_TX, TX_CSV).unwrap();
        assert_eq!(p.total_rows, 3);
        assert_eq!(p.error_rows, 0);
        assert_eq!(p.lines[0].amount_raw, Some(100_00));
        assert_eq!(p.lines[2].kind, "SELL");
    }

    #[test]
    fn preview_rejects_bad_rows() {
        let csv = "account,fund_code,trade_date,type,quantity,price\n\
            A,000001,2026-01-05,BUY,10,1.00\n\
            ,000001,2026-01-05,BUY,10,1.00\n\
            A,abc,2026-01-05,BUY,10,1.00\n\
            A,000001,2026/01/05,BUY,10,1.00\n";
        let p = preview(TEMPLATE_TX, csv).unwrap();
        assert_eq!(p.total_rows, 4);
        assert_eq!(p.valid_rows, 1);
        assert_eq!(p.error_rows, 3);
    }

    #[test]
    fn preview_snapshot_derived_and_unknown_cost() {
        let p = preview(TEMPLATE_SNAPSHOT, SNAP_CSV).unwrap();
        assert_eq!(p.valid_rows, 2);
        // 000009：市值 215 − 收益 15 = 200 推算成本。
        assert_eq!(p.lines[0].amount_raw, Some(200_00));
        assert_eq!(p.lines[0].cost_state, "derived");
        // 000010：无成本也无市值 → 未知成本。
        assert_eq!(p.lines[1].cost_state, "unknown");
        assert!(p.lines[1].amount_raw.is_none());
    }

    #[test]
    fn commit_tx_idempotent_and_replay() {
        let store = Store::open_in_memory().unwrap();
        let p = preview(TEMPLATE_TX, TX_CSV).unwrap();
        let first = commit(&store, "yjb", TEMPLATE_TX, "hash1", "params1", &p).unwrap();
        assert_eq!(first.rows_imported, 3);
        assert!(!first.replayed);
        // 持仓正确（黄金用例数值）。
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(position.quantity_raw, 900_000);
        assert_eq!(position.cost_raw, 96_90);
        // 精确重复批次：返回原结果不再次入账。
        let second = commit(&store, "yjb", TEMPLATE_TX, "hash1", "params1", &p).unwrap();
        assert!(second.replayed);
        assert_eq!(second.rows_imported, 3);
        // 重复导入不倍增持仓。
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(position.quantity_raw, 900_000);
    }

    #[test]
    fn commit_external_id_dedup_across_batches() {
        let store = Store::open_in_memory().unwrap();
        let p = preview(TEMPLATE_TX, TX_CSV).unwrap();
        commit(&store, "yjb", TEMPLATE_TX, "hash1", "params1", &p).unwrap();
        // 另一批次，但 external_id 相同 → skip 不倍增。
        let first = commit(&store, "other", TEMPLATE_TX, "hash2", "params2", &p).unwrap();
        assert_eq!(first.rows_skipped, 3);
        assert_eq!(first.rows_imported, 0);
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(position.quantity_raw, 900_000);
    }

    #[test]
    fn commit_snapshot_then_tx_conflict() {
        let store = Store::open_in_memory().unwrap();
        let sp = preview(TEMPLATE_SNAPSHOT, SNAP_CSV).unwrap();
        let receipt = commit(&store, "snap", TEMPLATE_SNAPSHOT, "h", "p", &sp).unwrap();
        assert_eq!(receipt.rows_imported, 2);
        // 快照后持仓存在且成本未知的一只显示 unknown。
        let positions = store
            .with_read(|c| crate::portfolio::list_positions(c))
            .unwrap();
        assert_eq!(positions.len(), 2);
        // 再用交易模板导入同账户/基金：快照不阻止交易（快照建账后可继续记账）。
        let tx_csv = "account,fund_code,trade_date,type,quantity,price\n券商,000009,2026-02-01,BUY,10,1.00\n";
        let tp = preview(TEMPLATE_TX, tx_csv).unwrap();
        let receipt = commit(&store, "t2", TEMPLATE_TX, "h2", "p2", &tp).unwrap();
        assert_eq!(receipt.rows_imported, 1);
    }

    #[test]
    fn csv_quotes_and_bom() {
        let csv = "\u{feff}account,fund_code,trade_date,type,quantity,price\n\
            \"我的,账户\",000001,2026-01-05,BUY,10,\"1.00\"\n";
        let p = preview(TEMPLATE_TX, csv).unwrap();
        assert_eq!(p.error_rows, 0);
        assert_eq!(p.lines[0].account, "我的,账户");
    }

    #[test]
    fn commit_tx_rolls_back_whole_batch_on_oversell() {
        let store = Store::open_in_memory().unwrap();
        // 第 1 行：买入 10 份
        // 第 2 行：卖出 20 份（超卖错误！）
        let csv = "account,fund_code,trade_date,type,quantity,price,fee\n\
            A,000001,2026-01-05,BUY,10,1.00,0\n\
            A,000001,2026-01-06,SELL,20,1.00,0\n";
        let p = preview(TEMPLATE_TX, csv).unwrap();
        assert_eq!(p.valid_rows, 2);
        let err = commit(&store, "batch-err", TEMPLATE_TX, "h_err", "p_err", &p).unwrap_err();
        assert!(matches!(err, ImportError::Invalid(_)));

        // 验证整批回滚：第 1 行买入也不得遗留入库，持仓为空
        let position = store
            .with_read(|c| crate::portfolio::replay_position(c, 1, 1))
            .unwrap();
        assert!(position.is_none(), "整批导入遇任何行错误必须原子回滚");

        let tx_count: i64 = store
            .with_read(|c| c.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(tx_count, 0, "事务回滚后 transactions 表必须为 0 条");
    }
}
