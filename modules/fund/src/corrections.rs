//! 交易更正／删除／恢复（方案 01 §5.2、03 §5；H04/H05 验收）。
//!
//! - 删除误录＝软删除（deleted_at），不物理清空；可恢复；恢复不重放现金。
//! - 删除／更正前回放整条时序验证：删除早期买入导致中途超卖 → 原子拒绝。
//! - 更正＝软删除原交易＋插入更正交易（source='correction'），同一事务；
//!   request_id 幂等，重试不双写。
//! - 删除买入不等于券商撤单；已结算更正须确认真实资金差额，同事务更新现金与持仓。

use rusqlite::{Connection, OptionalExtension};

use crate::ledger::LedgerError;
use crate::storage::Store;

/// 删除预览：回放验证 + 影响说明。
pub struct DeletePreview {
    pub tx_id: i64,
    pub tx_type: String,
    pub quantity_raw: i64,
    pub amount_raw: i64,
    pub fee_raw: i64,
    pub trade_date: String,
    /// 删除后该 (account, fund) 的期末数量（回放推演）。
    pub resulting_quantity_raw: i64,
    /// 阻塞项：非空时拒绝提交（如中途超卖）。
    pub blockers: Vec<String>,
}

fn tx_row(
    conn: &Connection,
    tx_id: i64,
) -> Result<(i64, i64, String, i64, i64, i64, String, String), LedgerError> {
    conn.query_row(
        "SELECT account_id, fund_id, type, quantity_raw, amount_raw, fee_raw, trade_date, state \
         FROM transactions WHERE id=?1 AND deleted_at IS NULL",
        [tx_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        },
    )
    .map_err(|e| LedgerError::Db(e.to_string()))
}

/// 回放某 (account, fund) 在排除集合后的期末数量（复用 portfolio 语义；
/// 这里直接实现避免跨模块快照依赖）。
fn replay_quantity(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
    exclude: i64,
) -> Result<i64, LedgerError> {
    let opening: Option<i64> = conn
        .query_row(
            "SELECT quantity_raw FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
            [account_id, fund_id],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                Ok(None)
            } else {
                Err(e)
            }
        })
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    let mut qty = opening.unwrap_or(0);
    let mut stmt = conn
        .prepare(
            "SELECT type, quantity_raw, trade_date FROM transactions \
             WHERE account_id=?1 AND fund_id=?2 AND state='confirmed' AND deleted_at IS NULL AND id != ?3 \
             ORDER BY trade_date, id",
        )
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    let rows = stmt
        .query_map([account_id, fund_id, exclude], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    let mut qty_at: Vec<(String, i64)> = Vec::new();
    for row in rows {
        let (t, q, d) = row.map_err(|e| LedgerError::Db(e.to_string()))?;
        qty_at.push((t, q));
        let _ = d;
    }
    // 逐笔回放，检测中途超卖（数量不可为负）。
    for (t, q) in qty_at {
        match t.as_str() {
            "BUY" => {
                qty = qty
                    .checked_add(q)
                    .ok_or_else(|| LedgerError::Invalid("数量溢出".into()))?
            }
            "SELL" => {
                qty -= q;
                if qty < 0 {
                    return Err(LedgerError::Oversell {
                        available_raw: qty + q,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(qty)
}

/// 转账腿保护（R2 T03 / 方案 03）：账户间持仓转移的两腿是成本平移的成对
/// 记录，单独删除／更正会破坏双边成本守恒，一律拒绝；纠正请用反向转账。
fn reject_transfer_leg(conn: &Connection, tx_id: i64) -> Result<(), LedgerError> {
    let source: Option<String> = conn
        .query_row(
            "SELECT source FROM transactions WHERE id=?1",
            [tx_id],
            |r| r.get(0),
        )
        .optional()?;
    if source.as_deref() == Some("transfer") {
        return Err(LedgerError::Invalid(
            "账户间转账流水不能单独删除或更正；请发起反向转账纠正".into(),
        ));
    }
    Ok(())
}

/// 删除预览（无副作用）。
pub fn preview_delete(store: &Store, tx_id: i64) -> Result<DeletePreview, LedgerError> {
    store.with_read(|conn| {
        reject_transfer_leg(conn, tx_id)?;
        let (account_id, fund_id, tx_type, qty, amount, fee, date, _state) = tx_row(conn, tx_id)?;
        let mut blockers = Vec::new();
        let resulting = match replay_quantity(conn, account_id, fund_id, tx_id) {
            Ok(q) => q,
            Err(LedgerError::Oversell { available_raw }) => {
                blockers.push(format!(
                    "删除该笔后时序出现中途超卖（可用 {available_raw}）"
                ));
                available_raw
            }
            Err(e) => return Err(e),
        };
        if _state == "pending" {
            blockers.push("待核对交易请先完成确认或拒绝，而不是删除".into());
        }
        Ok(DeletePreview {
            tx_id,
            tx_type,
            quantity_raw: qty,
            amount_raw: amount,
            fee_raw: fee,
            trade_date: date,
            resulting_quantity_raw: resulting,
            blockers,
        })
    })
}

/// 软删除（提交）。幂等：已删除的交易再次删除成功返回 false。
pub fn delete_transaction(
    store: &Store,
    tx_id: i64,
    reason: &str,
    now: &str,
) -> Result<bool, LedgerError> {
    delete_transaction_versioned(store, tx_id, reason, now, None)
}
pub fn delete_transaction_versioned(
    store: &Store,
    tx_id: i64,
    reason: &str,
    now: &str,
    expected: Option<i64>,
) -> Result<bool, LedgerError> {
    // 幂等：已删除的交易重复删除成功且无副作用。
    let already: bool = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT 1 FROM transactions WHERE id=?1 AND deleted_at IS NOT NULL",
                [tx_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|_| true)
            .or_else(|e| {
                if e == rusqlite::Error::QueryReturnedNoRows {
                    Ok(false)
                } else {
                    Err(e)
                }
            })
        })
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    if already {
        return Ok(false);
    }
    // 先预览验证（不持写锁）。
    let preview = preview_delete(store, tx_id)?;
    if !preview.blockers.is_empty() {
        return Err(LedgerError::Invalid(preview.blockers.join("；")));
    }
    store.with_write(|conn| {
        require_version(conn, tx_id, expected)?;
        // 资金状态保护（方案 H05）：误录删除≠退款。已入账（settled）／应收（receivable）
        // 的资金事实已存在，删除交易必须先在现金核对中处理；待核对（unreconciled）
        // 行随交易软删除（无现金副作用）。
        let cash_status: Option<String> = conn
            .query_row(
                "SELECT status FROM trade_cash WHERE tx_id=?1",
                [tx_id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| {
                if e == rusqlite::Error::QueryReturnedNoRows {
                    Ok(None)
                } else {
                    Err(e)
                }
            })
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        match cash_status.as_deref() {
            Some("settled") => {
                return Err(LedgerError::Invalid(
                    "该交易资金已入账核对；删除误录请先在现金核对中处理资金事实".into(),
                ));
            }
            Some("receivable") => {
                return Err(LedgerError::Invalid(
                    "该交易存在未到账应收资金；删除误录请先在现金核对中处理".into(),
                ));
            }
            Some("unreconciled") => {
                conn.execute("DELETE FROM trade_cash WHERE tx_id=?1", [tx_id])
                    .map_err(|e| LedgerError::Db(e.to_string()))?;
            }
            _ => {}
        }
        let (account_id, fund_id): (i64, i64) = conn
            .query_row(
                "SELECT account_id, fund_id FROM transactions WHERE id=?1",
                [tx_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        let n = conn
            .execute(
                "UPDATE transactions SET deleted_at=?2, deleted_reason=?3,version=version+1 \
                 WHERE id=?1 AND deleted_at IS NULL",
                rusqlite::params![tx_id, now, reason],
            )
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        if n == 1 {
            // 同事务同步持仓投影（审计 P0-3：删除后投影必须消失）。
            crate::portfolio::recompute_position(conn, account_id, fund_id)?;
        }
        Ok(n == 1)
    })
}

/// 恢复软删除交易；恢复后必须仍满足时序（可能因后续卖出已恢复数量而失败）。
pub fn restore_transaction(store: &Store, tx_id: i64) -> Result<bool, LedgerError> {
    restore_transaction_versioned(store, tx_id, None)
}
pub fn restore_transaction_versioned(
    store: &Store,
    tx_id: i64,
    expected: Option<i64>,
) -> Result<bool, LedgerError> {
    store.with_write(|conn| {
        require_version(conn,tx_id,expected)?;
        let corrected:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM transactions WHERE correction_of=?1 AND deleted_at IS NULL)",[tx_id],|r|r.get(0))?;
        if corrected{return Err(LedgerError::Conflict("该交易已被有效更正替代，请操作更正后的交易".into()));}
        let (account_id, fund_id): (i64, i64) = conn
            .query_row(
                "SELECT account_id, fund_id FROM transactions WHERE id=?1 AND deleted_at IS NOT NULL",
                [tx_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        // 先置回，再全量回放验证；失败则回滚（整个 with_write 事务）。
        conn.execute(
            "UPDATE transactions SET deleted_at=NULL, deleted_reason='',version=version+1 WHERE id=?1",
            [tx_id],
        )
        .map_err(|e| LedgerError::Db(e.to_string()))?;
        let qty = replay_quantity_in_tx(conn, account_id, fund_id)?;
        let _ = qty;
        // 同事务同步持仓投影。
        crate::portfolio::recompute_position(conn, account_id, fund_id)?;
        Ok(true)
    })
}

fn replay_quantity_in_tx(
    conn: &Connection,
    account_id: i64,
    fund_id: i64,
) -> Result<i64, LedgerError> {
    let opening: Option<i64> = conn
        .query_row(
            "SELECT quantity_raw FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
            [account_id, fund_id],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| {
            if e == rusqlite::Error::QueryReturnedNoRows {
                Ok(None)
            } else {
                Err(e)
            }
        })
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    let mut qty = opening.unwrap_or(0);
    let mut stmt = conn
        .prepare(
            "SELECT type, quantity_raw FROM transactions \
             WHERE account_id=?1 AND fund_id=?2 AND state='confirmed' AND deleted_at IS NULL \
             ORDER BY trade_date, id",
        )
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    let rows = stmt
        .query_map([account_id, fund_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    for row in rows {
        let (t, q) = row.map_err(|e| LedgerError::Db(e.to_string()))?;
        match t.as_str() {
            "BUY" => {
                qty = qty
                    .checked_add(q)
                    .ok_or_else(|| LedgerError::Invalid("数量溢出".into()))?
            }
            "SELL" => {
                qty -= q;
                if qty < 0 {
                    return Err(LedgerError::Oversell {
                        available_raw: qty + q,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(qty)
}

/// 更正预览输入：新数量／金额／费用／日期（服务端定点重算，不信任页面金额）。
#[derive(Clone)]
pub struct CorrectionInput {
    pub tx_id: i64,
    pub quantity_raw: i64,
    pub price_raw: i64,
    pub fee_raw: i64,
    pub trade_date: String,
    pub request_id: String,
    pub reason: String,
}

pub struct CorrectionPreview {
    pub original_quantity_raw: i64,
    pub original_amount_raw: i64,
    pub new_amount_raw: i64,
    pub resulting_quantity_raw: i64,
    pub blockers: Vec<String>,
    pub cash_difference_minor: Option<i64>,
    pub requires_cash_confirmation: bool,
}

/// 金额分 = quantity_raw(1e4) × price_raw(1e4) ÷ 1e6；checked 防溢出。
fn checked_amount(qty: i64, price: i64) -> Result<i64, LedgerError> {
    i64::try_from((qty as i128) * (price as i128) / 1_000_000)
        .map_err(|_| LedgerError::Invalid("金额计算溢出".into()))
}

/// 更正预览：按新字段推演删除原交易+插入更正交易的期末数量。
pub fn preview_correction(
    store: &Store,
    input: &CorrectionInput,
) -> Result<CorrectionPreview, LedgerError> {
    if input.quantity_raw <= 0 {
        return Err(LedgerError::Invalid("更正数量必须为正".into()));
    }
    if input.price_raw <= 0 {
        return Err(LedgerError::Invalid("更正价格必须为正".into()));
    }
    if input.fee_raw < 0
        || input.reason.trim().is_empty()
        || input.reason.len() > 4096
        || input.request_id.is_empty()
        || input.request_id.len() > 128
        || !crate::assets::is_valid_calendar_date(&input.trade_date)
        || input.trade_date > crate::finance::today_cn()
    {
        return Err(LedgerError::Invalid(
            "更正日期、费用、理由或请求标识无效".into(),
        ));
    }
    if input.trade_date.is_empty() {
        return Err(LedgerError::Invalid("更正日期必填".into()));
    }
    store.with_read(|conn| {
        reject_transfer_leg(conn, input.tx_id)?;
        let (account_id, fund_id, tx_type, orig_qty, _orig_amount, orig_fee, _date, state) =
            tx_row(conn, input.tx_id)?;
        if state!="confirmed"{return Err(LedgerError::Invalid("请先确认待核对交易".into()));}
        let code:String=conn.query_row("SELECT code FROM funds WHERE id=?1",[fund_id],|r|r.get(0))?;
        crate::ledger::validate_quantity(&code,crate::fixed::Fixed::nav(input.quantity_raw))?;
        let new_amount = checked_amount(input.quantity_raw, input.price_raw)?;
        let mut blockers = Vec::new();
        // 推演：排除原交易后按新交易回放。
        let opening: Option<i64> = conn
            .query_row(
                "SELECT quantity_raw FROM opening_snapshots WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
                [account_id, fund_id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| {
                if e == rusqlite::Error::QueryReturnedNoRows {
                    Ok(None)
                } else {
                    Err(e)
                }
            })
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        let mut qty = opening.unwrap_or(0);
        let mut stmt = conn
            .prepare(
                "SELECT id, type, quantity_raw, trade_date FROM transactions \
                 WHERE account_id=?1 AND fund_id=?2 AND state='confirmed' AND deleted_at IS NULL \
                 ORDER BY trade_date, id",
            )
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([account_id, fund_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        let mut ops: Vec<(i64, String, i64, String)> = rows
            .collect::<Result<_, _>>()
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        let next_id:i64=conn.query_row("SELECT COALESCE(MAX(id),0)+1 FROM transactions",[],|r|r.get(0))?;
        ops.push((
            next_id,
            tx_type.clone(),
            input.quantity_raw,
            input.trade_date.clone(),
        ));
        ops.sort_by(|a, b| (a.3.clone(), a.0).cmp(&(b.3.clone(), b.0)));
        for (id, t, q, _) in ops {
            if id == input.tx_id {
                continue; // 原交易被替换
            }
            match t.as_str() {
                "BUY" => qty = qty.checked_add(q).ok_or_else(||LedgerError::Invalid("数量溢出".into()))?,
                "SELL" => {
                    qty -= q;
                    if qty < 0 {
                        blockers.push(format!(
                            "更正后时序出现中途超卖（该时点可用 {}/股）",
                            qty + q
                        ));
                        break;
                    }
                }
                _ => {}
            }
        }
        let prior:Option<(i64,String)>=conn.query_row("SELECT delta_minor,status FROM trade_cash WHERE tx_id=?1",[input.tx_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let delta=if tx_type=="BUY"{new_amount.checked_add(input.fee_raw).and_then(i64::checked_neg)}else{new_amount.checked_sub(input.fee_raw).filter(|n|*n>=0)}.ok_or_else(||LedgerError::Invalid("金额或费用无效".into()))?;
        let cash_difference_minor=prior.as_ref().filter(|p|p.1=="settled").map(|p|delta.checked_sub(p.0).ok_or_else(||LedgerError::Invalid("资金差额溢出".into()))).transpose()?;
        let _ = (orig_qty, orig_fee);
        Ok(CorrectionPreview {
            original_quantity_raw: orig_qty,
            original_amount_raw: _orig_amount,
            new_amount_raw: new_amount,
            resulting_quantity_raw: qty,
            blockers,
            requires_cash_confirmation:cash_difference_minor.is_some_and(|n|n!=0),
            cash_difference_minor,
        })
    })
}

/// 更正提交：软删除原交易＋插入更正交易，一个事务；request_id 幂等。
/// 幂等判等：同 request_id 的既有更正与新请求逐字段比较（含原交易溯源）。
fn same_correction(
    prev: (i64, i64, i64, i64, i64, i64, String),
    input: &CorrectionInput,
) -> Option<i64> {
    let (id, prev_tx, qty, price, fee, prev_amount, date) = prev;
    let amount = match checked_amount(input.quantity_raw, input.price_raw) {
        Ok(a) => a,
        Err(_) => return None,
    };
    if prev_tx == input.tx_id
        && qty == input.quantity_raw
        && price == input.price_raw
        && fee == input.fee_raw
        && prev_amount == amount
        && date == input.trade_date
    {
        Some(id)
    } else {
        None
    }
}

pub fn submit_correction(
    store: &Store,
    input: &CorrectionInput,
    now: &str,
) -> Result<i64, LedgerError> {
    submit_correction_versioned(store, input, now, None)
}
pub fn submit_correction_versioned(
    store: &Store,
    input: &CorrectionInput,
    now: &str,
    expected: Option<i64>,
) -> Result<i64, LedgerError> {
    submit_correction_with_cash(store, input, now, expected, None)
}

pub fn submit_correction_with_cash(
    store: &Store,
    input: &CorrectionInput,
    now: &str,
    expected: Option<i64>,
    cash: Option<&serde_json::Value>,
) -> Result<i64, LedgerError> {
    if cash.is_some_and(|p| {
        p.as_object().is_none_or(|m| {
            m.keys()
                .any(|k| !matches!(k.as_str(), "confirmed" | "settledAt" | "note"))
        }) || p["confirmed"].as_bool() != Some(true)
            || p["note"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty() || s.len() > 4096)
            || p["settledAt"]
                .as_str()
                .is_none_or(|s| !crate::assets::is_valid_calendar_date(s))
    }) {
        return Err(LedgerError::Invalid("资金更正确认字段无效".into()));
    }
    let confirmation = serde_json::json!({"reason":input.reason,"cash":cash}).to_string();
    // 幂等优先（重试不重放预览——原交易可能已被首次更正软删除）：
    // 同 key 且内容一致 → 返回同一结果；同 key 内容不同 → 拒绝（审计 P0-3）。
    let existing: Option<(i64, i64, i64, i64, i64, i64, String)> = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT id, correction_of, quantity_raw, price_raw, fee_raw, amount_raw, trade_date                  FROM transactions WHERE request_id=?1 AND source='correction'",
                [input.request_id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })
        .map_err(|e| LedgerError::Db(e.to_string()))?;
    if let Some(prev) = existing {
        return match same_correction(prev, input) {
            Some(id) => {
                store.with_read(|c| require_confirmation(c, id, &confirmation))?;
                Ok(id)
            }
            None => Err(LedgerError::Invalid(
                "requestId 已用于不同内容的更正；请使用新的 request_id".into(),
            )),
        };
    }
    let preview = preview_correction(store, input)?;
    if !preview.blockers.is_empty() {
        return Err(LedgerError::Invalid(preview.blockers.join("；")));
    }
    store.with_write(|conn| {
        // 并发兜底：预览后另一请求可能已提交同 key 更正。
        let existing: Option<(i64, i64, i64, i64, i64, i64, String)> = conn
            .query_row(
                "SELECT id, correction_of, quantity_raw, price_raw, fee_raw, amount_raw, trade_date \
                 FROM transactions WHERE request_id=?1 AND source='correction'",
                [input.request_id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        if let Some(prev) = existing {
            return match same_correction(prev, input) {
                Some(id) => {require_confirmation(conn,id,&confirmation)?;Ok(id)},
                None => Err(LedgerError::Invalid(
                    "requestId 已用于不同内容的更正；请使用新的 request_id".into(),
                )),
            };
        }
        require_version(conn,input.tx_id,expected)?;
        let (account_id, fund_id, tx_type, _, _, _, _, _): (
            i64,
            i64,
            String,
            i64,
            i64,
            i64,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT account_id, fund_id, type, quantity_raw, amount_raw, fee_raw, trade_date, state \
                 FROM transactions WHERE id=?1 AND deleted_at IS NULL",
                [input.tx_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .map_err(|e| LedgerError::Db(e.to_string()))?;
        let amount = checked_amount(input.quantity_raw, input.price_raw)?;
        let prior:Option<(i64,String,String)>=conn.query_row("SELECT delta_minor,status,settled_at FROM trade_cash WHERE tx_id=?1",[input.tx_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map(Some).or_else(|e|if e==rusqlite::Error::QueryReturnedNoRows{Ok(None)}else{Err(e)})?;
        let new_delta=if tx_type=="BUY"{amount.checked_add(input.fee_raw).and_then(i64::checked_neg)}else{amount.checked_sub(input.fee_raw).filter(|v|*v>=0)}.ok_or_else(||LedgerError::Invalid("金额或费用无效".into()))?;
        let mut settled_date=prior.as_ref().map(|p|p.2.clone()).unwrap_or_default();
        if let Some((old_delta,status,_))=&prior{
            if status=="settled"&&new_delta!=*old_delta {
                let proof=cash.filter(|p|p["confirmed"].as_bool()==Some(true)).ok_or_else(||LedgerError::Invalid("已核对交易更正金额需要确认真实资金差额、到账日期及依据".into()))?;
                let date=proof["settledAt"].as_str().unwrap_or("");let note=proof["note"].as_str().unwrap_or("");
                if !crate::assets::is_valid_calendar_date(date)||date>crate::finance::today_cn().as_str()||date<input.trade_date.as_str()||note.trim().is_empty()||note.len()>4096{return Err(LedgerError::Invalid("资金更正日期或依据无效".into()));}
                let delta=new_delta.checked_sub(*old_delta).ok_or_else(||LedgerError::Invalid("资金差额溢出".into()))?;
                if delta<0 {let available=crate::finance::cash_position(conn).map_err(|e|LedgerError::Invalid(e.message()))?.accounts.into_iter().find(|a|a.account_id==account_id).and_then(|a|a.available_minor).ok_or_else(||LedgerError::Invalid("先核对现金与预留".into()))?;if delta.checked_neg().is_none_or(|v|v>available){return Err(LedgerError::Invalid("更正差额超过可用现金".into()));}}
                crate::finance::adjust_account_cash(conn,account_id,delta,date).map_err(|e|LedgerError::Invalid(e.message()))?;settled_date=date.to_string();
            }
        }
        conn.execute(
            "UPDATE transactions SET deleted_at=?2, deleted_reason=?3,version=version+1 WHERE id=?1 AND deleted_at IS NULL",
            rusqlite::params![input.tx_id, now, format!("corrected: {}", input.reason)],
        )
        .map_err(|e| LedgerError::Db(e.to_string()))?;
        conn.execute(
            "INSERT INTO transactions(account_id, fund_id, type, state, quantity_raw, price_raw, \
             amount_raw, fee_raw, trade_date, source, request_id, created_at, correction_of) \
             VALUES(?1,?2,?3,'confirmed',?4,?5,?6,?7,?8,'correction',?9,?10,?11)",
            rusqlite::params![
                account_id,
                fund_id,
                tx_type,
                input.quantity_raw,
                input.price_raw,
                amount,
                input.fee_raw,
                input.trade_date,
                input.request_id,
                now,
                input.tx_id
            ],
        )
        .map_err(|e| LedgerError::Db(e.to_string()))?;
        let new_id=conn.last_insert_rowid();
        conn.execute("UPDATE transactions SET correction_confirmation_json=?2 WHERE id=?1",rusqlite::params![new_id,confirmation])?;
        if let Some((_,status,_))=prior {conn.execute("INSERT INTO trade_cash(tx_id,account_id,delta_minor,status,settled_at,changed_revision,note) VALUES(?1,?2,?3,?4,?5,(SELECT CAST(value AS INTEGER)+1 FROM app_meta WHERE key='data_revision'),?6)",rusqlite::params![new_id,account_id,new_delta,status,settled_date,input.reason])?;}
        // 同事务同步持仓投影，失败时资金差额及更正记录一起回滚。
        crate::portfolio::recompute_position(conn, account_id, fund_id)?;
        Ok(new_id)
    })
}

#[cfg(test)]
fn today_for_test() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64
        + 8 * 3600;
    let (y, m, d) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z / 86400 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + i64::from(if m <= 2 { 1 } else { 0 }), m, d)
}

#[cfg(test)]
#[path = "corrections_tests.rs"]
mod tests;

fn require_version(conn: &Connection, id: i64, expected: Option<i64>) -> Result<(), LedgerError> {
    if let Some(expected) = expected {
        let version: i64 = conn
            .query_row("SELECT version FROM transactions WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(|_| LedgerError::NotFound("交易".into()))?;
        if expected != version {
            return Err(LedgerError::Conflict("交易版本已变化".into()));
        }
    }
    Ok(())
}

fn require_confirmation(c: &Connection, id: i64, expected: &str) -> Result<(), LedgerError> {
    let saved: String = c.query_row(
        "SELECT correction_confirmation_json FROM transactions WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    if !saved.is_empty() && saved != expected {
        return Err(LedgerError::Conflict(
            "requestId 已用于不同资金依据或更正理由".into(),
        ));
    }
    Ok(())
}
