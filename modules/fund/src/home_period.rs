//! Selected whole-asset return coverage; live reference never writes or certifies a snapshot.
use crate::{
    assets::{self, AssetsSummary},
    finance::FinanceError,
    returns,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) fn project(
    conn: &Connection,
    summary: &AssetsSummary,
    from: &str,
    to: &str,
    today: &str,
) -> Result<Value, FinanceError> {
    let previous = assets::days_from_civil(from).map(|d| assets::civil_from_days(d - 1));
    let exact: Option<String> = if let Some(date) = &previous {
        conn.query_row("SELECT taken_at FROM value_snapshots WHERE scope='A' AND taken_at=?1 ORDER BY id DESC LIMIT 1",[date],|r|r.get(0)).optional()?
    } else {
        None
    };
    let baseline:Option<String>=match exact {
        Some(date)=>Some(date),None=>conn.query_row(
            "SELECT MIN(taken_at) FROM value_snapshots WHERE scope='A' AND taken_at>=?1 AND taken_at<=?2",params![from,to],|r|r.get(0))?,
    };
    let missing = |reason: &str| json!({"status":"incomplete","reason":reason,"scope":"A","requestedFrom":from,"requestedTo":to,"from":baseline,"to":to,"netReturnMinor":null});
    let Some(start) = baseline.as_deref() else {
        return Ok(missing("missing_baseline"));
    };
    let current =
        to == today && summary.complete_value_minor.is_some() && summary.old_valuation_count == 0;
    let end: Option<String> = if current {
        Some(to.into())
    } else {
        conn.query_row("SELECT MAX(taken_at) FROM value_snapshots WHERE scope='A' AND taken_at>=?1 AND taken_at<=?2",params![start,to],|r|r.get(0))?
    };
    let Some(end) = end else {
        return Ok(missing("missing_end"));
    };
    if current {
        let attribution = crate::portfolio_attribution::range_live(conn, start, &end, summary)?;
        if !summary.funds.is_empty() && attribution["status"] != "reference" {
            return Ok(missing(
                attribution["reason"]
                    .as_str()
                    .unwrap_or("missing_components"),
            ));
        }
        let captured:Option<i64>=conn.query_row("SELECT created_at FROM value_snapshots WHERE scope='A' AND taken_at=?1 ORDER BY id DESC LIMIT 1",[start],|r|r.get::<_,String>(0)).optional()?.and_then(|s|s.parse().ok());
        let has_other: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM manual_assets) OR EXISTS(SELECT 1 FROM offline_assets)",
            [],
            |r| r.get(0),
        )?;
        if has_other && captured.filter(|v| *v > 0).is_none() {
            return Ok(missing("missing_components"));
        }
        if let Some(captured) = captured.filter(|v| *v > 0) {
            let changed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM manual_assets WHERE CAST(created_at AS INTEGER)>?1 OR (CAST(deleted_at AS INTEGER)>?1 OR (length(deleted_at)=10 AND deleted_at>?2))) OR EXISTS(SELECT 1 FROM offline_assets WHERE CAST(created_at AS INTEGER)>?1 OR (CAST(deleted_at AS INTEGER)>?1 OR (length(deleted_at)=10 AND deleted_at>?2)))",params![captured,start],|r|r.get(0))?;
            if changed {
                return Ok(missing("unexplained_adjustment"));
            }
        }
    }
    let result = if current {
        returns::live_reference(conn, start, &end, summary)?
    } else {
        returns::daily_returns(conn, start, &end)?
    };
    if !result.computable {
        return Ok(missing(
            result.reason.as_deref().unwrap_or("unverified_history"),
        ));
    }
    let partial = (!from.is_empty() && Some(start) != previous.as_deref()) || end != to;
    Ok(
        json!({"scope":"A","status":if current{"reference"}else if partial{"partial"}else{"ready"},
        "reason":result.reason,"partialCoverage":partial,"requestedFrom":from,"requestedTo":to,
        "from":start,"to":end,"baselineDate":start,"endSource":if current{"current-valuation"}else{"value_snapshots"},
        "netReturnMinor":result.net_return_minor.map(|v|v.to_string()),"beginMinor":result.begin_total_minor.map(|v|v.to_string()),
        "endMinor":result.end_total_minor.map(|v|v.to_string()),"netExternalMinor":result.net_external_minor.map(|v|v.to_string()),
        "valuationAsOf":summary.latest_valuation,"valuationBatch":summary.valuation_batch,
        "verification":if current{"recorded-flow-reference-not-confirmed"}else{"checked-snapshots"}}),
    )
}
