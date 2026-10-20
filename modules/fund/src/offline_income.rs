//! Recorded lifetime book return; partial exits do not imply a cost allocation.
use crate::assets::{add_minor, AssetError};
use rusqlite::Connection;
use serde_json::{json, Value};

pub(crate) fn summary(conn: &Connection, today: &str) -> Result<Value, AssetError> {
    let mut value = 0;
    let mut cost = 0;
    let mut recovered = 0;
    let mut receivable = 0;
    let mut count = 0;
    let mut unknown = 0;
    let mut stale = 0;
    let mut stmt = conn.prepare(
        "SELECT id,value_minor,cost_minor,valued_at FROM offline_assets WHERE deleted_at IS NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, current, basis, date) = row?;
        count += 1;
        value = add_minor(value, current, "offline value")?;
        if let Some(basis) = basis {
            cost = add_minor(cost, basis, "offline cost")?;
        } else {
            unknown += 1;
        }
        if crate::assets::is_stale(&date, today) {
            stale += 1;
        }
        let mut exits = conn.prepare(
            "SELECT amount_minor,fee_minor,settled FROM offline_exit_history WHERE asset_id=?1",
        )?;
        for exit in exits.query_map([id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })? {
            let (amount, fee, settled) = exit?;
            let net = amount
                .checked_sub(fee)
                .ok_or_else(|| AssetError::Overflow("offline proceeds".into()))?;
            recovered = add_minor(recovered, net, "offline proceeds")?;
            if !settled {
                receivable = add_minor(receivable, net, "offline receivable")?;
            }
        }
    }
    let missing_history: i64 = conn.query_row("SELECT COUNT(*) FROM offline_assets a WHERE a.deleted_at IS NULL AND a.exit_date IS NOT NULL AND NOT EXISTS(SELECT 1 FROM offline_exit_history h WHERE h.asset_id=a.id)", [], |r|r.get(0))?;
    let profit = add_minor(
        add_minor(value, recovered, "offline book return")?,
        -cost,
        "offline book return",
    )?;
    Ok(
        json!({"scope":"recorded-offline-lifetime","count":count,"valueMinor":value.to_string(),
        "costMinor":if unknown==0 {Some(cost.to_string())}else{None},
        "bookProfitMinor":if unknown==0 && missing_history==0 {Some(profit.to_string())}else{None},
        "netRecoveredMinor":if missing_history==0 {Some(recovered.to_string())}else{None},"receivableMinor":if missing_history==0 {Some(receivable.to_string())}else{None},
        "unknownCostCount":unknown,"missingHistoryCount":missing_history,"staleCount":stale,"asOf":today}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        offline::{self, ExitInput, OfflineInput},
        storage::Store,
    };
    #[test]
    fn partial_exits_fees_unknown_cost_and_deleted_assets_are_not_double_counted() {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        let id = offline::create_asset(
            &store,
            &OfflineInput {
                product_type: "gold".into(),
                name: "Gold".into(),
                location: "".into(),
                value_minor: 12000,
                cost_minor: Some(10000),
                valued_at: today.clone(),
                risk_class: "gold".into(),
                note: "".into(),
                attrs_json: "{}".into(),
            },
        )
        .unwrap();
        offline::exit_asset(
            &store,
            id,
            &ExitInput {
                exit_date: today.clone(),
                exit_amount_minor: 5000,
                exit_fee_minor: 100,
                remaining_value_minor: Some(8000),
                settle_account_id: None,
                expected_version: 1,
                idempotency_key: "partial".into(),
            },
        )
        .unwrap();
        let project = || store.with_read(|c| summary(c, &today)).unwrap();
        let result = project();
        assert_eq!(result["bookProfitMinor"], "2900");
        assert_eq!(result["netRecoveredMinor"], "4900");
        assert_eq!(result["receivableMinor"], "4900");
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE offline_assets SET cost_minor=NULL WHERE id=?1",
                    [id],
                )
            })
            .unwrap();
        assert!(project()["bookProfitMinor"].is_null());
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE offline_assets SET cost_minor=10000 WHERE id=?1",
                    [id],
                )?;
                c.execute("DELETE FROM offline_exit_history WHERE asset_id=?1", [id])
            })
            .unwrap();
        let legacy = project();
        assert_eq!(legacy["missingHistoryCount"], 1);
        assert!(legacy["bookProfitMinor"].is_null());
        assert!(legacy["netRecoveredMinor"].is_null());
        store
            .with_write(|c| {
                c.execute(
                    "UPDATE offline_assets SET deleted_at='deleted' WHERE id=?1",
                    [id],
                )
            })
            .unwrap();
        let empty = project();
        assert_eq!(empty["count"], 0);
        assert_eq!(empty["valueMinor"], "0");
        assert_eq!(empty["netRecoveredMinor"], "0");
    }
}
