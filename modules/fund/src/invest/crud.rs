//! 账户与标的的维护。
//!
//! 两者都是「配置」，不是「账本」：写错可以改，不产生金额后果。
//! 真正影响数字的是 `flows`（见 `flows.rs`）。

use super::model::{
    AccountView, CreateAccount, CreateInstrument, InstrumentView, UpdateAccount, UpdateInstrument,
    ASSET_CLASSES, CHANNELS, MARKETS,
};
use crate::finance::{now_secs, FinanceError};
use crate::fixed::Fixed;
use crate::storage::Store;

fn fmt_price(raw: i64) -> String {
    Fixed::nav(raw).to_string_value()
}

fn ensure_in(value: &str, allowed: &[&str], field: &str) -> Result<(), FinanceError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(FinanceError::Invalid(format!(
            "{field} 取值非法：{value}（允许 {}）",
            allowed.join(" / ")
        )))
    }
}

fn require_text(value: &str, field: &str) -> Result<String, FinanceError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(FinanceError::Invalid(format!("{field} 不能为空")));
    }
    Ok(trimmed.to_string())
}

// ---------- 账户 ----------

pub fn list_accounts(store: &Store) -> Result<Vec<AccountView>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, channel, currency, note, archived_at
             FROM accounts ORDER BY sort_order, id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AccountView {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    channel: r.get(2)?,
                    currency: r.get(3)?,
                    note: r.get(4)?,
                    archived: r.get::<_, Option<String>>(5)?.is_some(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn create_account(store: &Store, payload: &CreateAccount) -> Result<AccountView, FinanceError> {
    let name = require_text(&payload.name, "账户名称")?;
    ensure_in(&payload.channel, &CHANNELS, "渠道")?;
    let currency = require_text(&payload.currency, "币种")?.to_uppercase();
    let ts = now_secs();
    store.with_write(|conn| {
        conn.execute(
            "INSERT INTO accounts(name, note, channel, currency, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![name, payload.note.trim(), payload.channel, currency, ts],
        )?;
        Ok(AccountView {
            id: conn.last_insert_rowid(),
            name,
            channel: payload.channel.clone(),
            currency,
            note: payload.note.trim().to_string(),
            archived: false,
        })
    })
}

pub fn update_account(
    store: &Store,
    id: i64,
    payload: &UpdateAccount,
) -> Result<AccountView, FinanceError> {
    let ts = now_secs();
    store.with_write(|conn| {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(FinanceError::NotFound(format!("账户 {id} 不存在")));
        }
        if let Some(name) = &payload.name {
            let name = require_text(name, "账户名称")?;
            conn.execute(
                "UPDATE accounts SET name=?1 WHERE id=?2",
                [&name, &id.to_string()],
            )?;
        }
        if let Some(note) = &payload.note {
            conn.execute(
                "UPDATE accounts SET note=?1 WHERE id=?2",
                [note.trim(), &id.to_string()],
            )?;
        }
        if let Some(archived) = payload.archived {
            let value: Option<&str> = if archived { Some(ts.as_str()) } else { None };
            conn.execute(
                "UPDATE accounts SET archived_at=?1 WHERE id=?2",
                rusqlite::params![value, id],
            )?;
        }
        conn.execute(
            "UPDATE accounts SET updated_at=?1 WHERE id=?2",
            [&ts, &id.to_string()],
        )?;
        Ok(())
    })?;
    read_account(store, id)
}

fn read_account(store: &Store, id: i64) -> Result<AccountView, FinanceError> {
    store.with_read(|conn| {
        conn.query_row(
            "SELECT id, name, channel, currency, note, archived_at FROM accounts WHERE id=?1",
            [id],
            |r| {
                Ok(AccountView {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    channel: r.get(2)?,
                    currency: r.get(3)?,
                    note: r.get(4)?,
                    archived: r.get::<_, Option<String>>(5)?.is_some(),
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                FinanceError::NotFound(format!("账户 {id} 不存在"))
            }
            other => FinanceError::Db(other.to_string()),
        })
    })
}

// ---------- 标的 ----------

pub fn list_instruments(store: &Store) -> Result<Vec<InstrumentView>, FinanceError> {
    store.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, code, name, asset_class, market, currency, group_code, share_class,
                    latest_price_raw, latest_price_at, price_stale
             FROM instruments
             WHERE archived_at IS NULL
             ORDER BY asset_class, market, code",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(InstrumentView {
                    id: r.get(0)?,
                    code: r.get(1)?,
                    name: r.get(2)?,
                    asset_class: r.get(3)?,
                    market: r.get(4)?,
                    currency: r.get(5)?,
                    group_code: r.get(6)?,
                    share_class: r.get(7)?,
                    latest_price: r.get::<_, Option<i64>>(8)?.map(fmt_price),
                    latest_price_at: r.get(9)?,
                    price_stale: r.get::<_, i64>(10)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn create_instrument(
    store: &Store,
    payload: &CreateInstrument,
) -> Result<InstrumentView, FinanceError> {
    let code = require_text(&payload.code, "代码")?;
    ensure_in(&payload.asset_class, &ASSET_CLASSES, "资产类别")?;
    ensure_in(&payload.market, &MARKETS, "市场")?;
    let currency = require_text(&payload.currency, "币种")?.to_uppercase();
    let name = payload.name.trim().to_string();
    let ts = now_secs();
    store.with_write(|conn| {
        let dup: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM instruments WHERE code=?1 AND market=?2)",
            [&code, &payload.market],
            |r| r.get(0),
        )?;
        if dup {
            return Err(FinanceError::Conflict(format!(
                "{}/{} 已存在",
                payload.market, code
            )));
        }
        conn.execute(
            "INSERT INTO instruments(code, name, asset_class, market, currency,
                                     group_code, share_class, created_at, updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
            rusqlite::params![
                code,
                name,
                payload.asset_class,
                payload.market,
                currency,
                payload.group_code,
                payload.share_class,
                ts
            ],
        )?;
        Ok(InstrumentView {
            id: conn.last_insert_rowid(),
            code,
            name,
            asset_class: payload.asset_class.clone(),
            market: payload.market.clone(),
            currency,
            group_code: payload.group_code.clone(),
            share_class: payload.share_class.clone(),
            latest_price: None,
            latest_price_at: None,
            price_stale: false,
        })
    })
}

pub fn update_instrument(
    store: &Store,
    id: i64,
    payload: &UpdateInstrument,
) -> Result<InstrumentView, FinanceError> {
    let ts = now_secs();
    store.with_write(|conn| {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM instruments WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(FinanceError::NotFound(format!("标的 {id} 不存在")));
        }
        if let Some(name) = &payload.name {
            conn.execute(
                "UPDATE instruments SET name=?1 WHERE id=?2",
                [name.trim(), &id.to_string()],
            )?;
        }
        if let Some(group) = &payload.group_code {
            conn.execute(
                "UPDATE instruments SET group_code=?1 WHERE id=?2",
                [group.trim(), &id.to_string()],
            )?;
        }
        if let Some(class) = &payload.share_class {
            conn.execute(
                "UPDATE instruments SET share_class=?1 WHERE id=?2",
                [class.trim(), &id.to_string()],
            )?;
        }
        if let Some(archived) = payload.archived {
            let value: Option<&str> = if archived { Some(ts.as_str()) } else { None };
            conn.execute(
                "UPDATE instruments SET archived_at=?1 WHERE id=?2",
                rusqlite::params![value, id],
            )?;
        }
        conn.execute(
            "UPDATE instruments SET updated_at=?1 WHERE id=?2",
            [&ts, &id.to_string()],
        )?;
        Ok(())
    })?;
    read_instrument(store, id)
}

fn read_instrument(store: &Store, id: i64) -> Result<InstrumentView, FinanceError> {
    store.with_read(|conn| {
        conn.query_row(
            "SELECT id, code, name, asset_class, market, currency, group_code, share_class,
                    latest_price_raw, latest_price_at, price_stale
             FROM instruments WHERE id=?1",
            [id],
            |r| {
                Ok(InstrumentView {
                    id: r.get(0)?,
                    code: r.get(1)?,
                    name: r.get(2)?,
                    asset_class: r.get(3)?,
                    market: r.get(4)?,
                    currency: r.get(5)?,
                    group_code: r.get(6)?,
                    share_class: r.get(7)?,
                    latest_price: r.get::<_, Option<i64>>(8)?.map(fmt_price),
                    latest_price_at: r.get(9)?,
                    price_stale: r.get::<_, i64>(10)? != 0,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                FinanceError::NotFound(format!("标的 {id} 不存在"))
            }
            other => FinanceError::Db(other.to_string()),
        })
    })
}
