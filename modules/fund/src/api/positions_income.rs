//! 持仓收益的日期、真实数量与资金流投影；不把当前数量回推历史。
use crate::{assets, fixed::Fixed};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

type Operation = (String, i64, i64, i64, String, String);
pub(super) struct IncomeBook {
    opening: Option<(i64, Option<i64>, String)>,
    operations: Vec<Operation>,
    truncated: bool,
    calibrations: Vec<(String, i64)>,
}

pub(super) fn load(conn: &Connection, account: i64, fund: i64) -> rusqlite::Result<IncomeBook> {
    let opening = conn
        .query_row(
            "SELECT quantity_raw,cost_raw,as_of FROM opening_snapshots
         WHERE account_id=?1 AND fund_id=?2 AND deleted_at IS NULL",
            params![account, fund],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let mut stmt = conn.prepare(
        "SELECT type,quantity_raw,amount_raw,fee_raw,trade_date,source FROM transactions
         WHERE account_id=?1 AND fund_id=?2 AND state='confirmed' AND deleted_at IS NULL
         ORDER BY trade_date,id LIMIT 10001",
    )?;
    let mut operations = stmt
        .query_map(params![account, fund], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<Vec<Operation>, _>>()?;
    let mut statement=conn.prepare("SELECT as_of_date,quantity_raw FROM position_calibrations WHERE account_id=?1 AND fund_id=?2 ORDER BY as_of_date,id LIMIT 10001")?;
    let mut calibrations = statement
        .query_map(params![account, fund], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let truncated = operations.len() > 10000 || calibrations.len() > 10000;
    calibrations.truncate(10000);
    operations.truncate(10000);
    Ok(IncomeBook {
        opening,
        operations,
        truncated,
        calibrations,
    })
}

pub(super) fn nav_prices(
    conn: &Connection,
    fund: i64,
    today: &str,
) -> rusqlite::Result<BTreeMap<String, i64>> {
    let mut stmt = conn.prepare(
        "SELECT nav_date,unit_nav_raw FROM fund_nav WHERE fund_id=?1 AND nav_date<=?2
         AND (unit_nav_raw>0 OR (unit_nav_raw=0 AND source='user_confirmed'))
         ORDER BY nav_date DESC LIMIT 400",
    )?;
    let rows = stmt
        .query_map(params![fund, today], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect();
    rows
}

pub(super) fn market_prices(value: &Value, today: &str) -> BTreeMap<String, i64> {
    if value["adjustment"] != "bf" {
        return BTreeMap::new();
    }
    value["candles"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .take(400)
        .filter_map(|bar| {
            let date = bar["time"].as_str()?;
            if !assets::is_valid_calendar_date(date) || date > today {
                return None;
            }
            let raw = Fixed::parse(&bar["close"].as_f64()?.to_string(), 4)
                .ok()?
                .raw();
            (raw >= 0).then(|| (date.to_string(), raw))
        })
        .collect()
}

fn unavailable(reason: &str, date: Option<&str>) -> Value {
    json!({"amount":null,"rateBps":null,"date":date,"baselineDate":null,"reason":reason})
}

fn result(amount: i128, basis: Option<i128>, date: &str, baseline: Option<&str>) -> Value {
    let Ok(minor) = i64::try_from(amount) else {
        return unavailable("overflow", Some(date));
    };
    let rate = basis.filter(|b| *b > 0).and_then(|b| {
        amount
            .checked_mul(10000)
            .and_then(|v| i64::try_from(v / b).ok())
    });
    json!({"amount":Fixed::amount(minor).to_string_value(),"rateBps":rate.map(|r|r.to_string()),
        "date":date,"baselineDate":baseline,
        "reason":if rate.is_none() {Some("missing_rate_basis")} else {None::<&str>}})
}

impl IncomeBook {
    fn quantity(&self, date: &str) -> i128 {
        let mut quantity = self
            .opening
            .as_ref()
            .filter(|(_, _, d)| d.as_str() <= date)
            .map(|(q, _, _)| *q as i128)
            .unwrap_or(0);
        let mut index = 0;
        for (kind, q, _, _, day, _) in self.operations.iter().filter(|op| op.4.as_str() <= date) {
            while index < self.calibrations.len() && self.calibrations[index].0 < *day {
                quantity = self.calibrations[index].1 as i128;
                index += 1;
            }
            quantity += if kind == "BUY" {
                *q as i128
            } else {
                -(*q as i128)
            };
        }
        while index < self.calibrations.len() && self.calibrations[index].0.as_str() <= date {
            quantity = self.calibrations[index].1 as i128;
            index += 1;
        }
        quantity
    }

    pub(super) fn holding_days(&self, today: &str) -> Option<i64> {
        if self.truncated {
            return None;
        }
        let (mut quantity, mut since) = self
            .opening
            .as_ref()
            .filter(|(_, _, d)| d.as_str() <= today)
            .map(|(q, _, d)| (*q as i128, Some(d.as_str())))
            .unwrap_or((0, None));
        let mut index = 0;
        let apply = |quantity: &mut i128, since: &mut Option<&str>, cal: &(String, i64)| {
            if cal.1 == 0 {
                *since = None;
            }
            *quantity = cal.1 as i128;
        };
        for (kind, q, _, _, day, _) in self.operations.iter().filter(|op| op.4.as_str() <= today) {
            while index < self.calibrations.len() && self.calibrations[index].0 < *day {
                let cal = &self.calibrations[index];
                if quantity == 0 && cal.1 > 0 {
                    since = Some(cal.0.as_str());
                }
                apply(&mut quantity, &mut since, cal);
                index += 1;
            }
            if kind == "BUY" {
                if quantity == 0 {
                    since = Some(day.as_str());
                }
                quantity += *q as i128;
            } else {
                quantity -= *q as i128;
                if quantity == 0 {
                    since = None;
                }
            }
        }
        while index < self.calibrations.len() && self.calibrations[index].0.as_str() <= today {
            let cal = &self.calibrations[index];
            if quantity == 0 && cal.1 > 0 {
                since = Some(cal.0.as_str());
            }
            apply(&mut quantity, &mut since, cal);
            index += 1;
        }
        if quantity <= 0 {
            return None;
        }
        Some((assets::days_from_civil(today)? - assets::days_from_civil(since?)?).max(0))
    }

    fn range(&self, from: &str, to: &str, prices: &BTreeMap<String, i64>) -> Value {
        if self.truncated {
            return unavailable("record_limit", Some(to));
        }
        if self
            .calibrations
            .iter()
            .any(|(date, _)| date.as_str() >= from && date.as_str() <= to)
        {
            return unavailable("calibration_boundary", Some(to));
        }
        if from > to {
            return unavailable("missing_period_end", Some(to));
        }
        let Some(days) = assets::days_from_civil(from) else {
            return unavailable("invalid_date", Some(to));
        };
        let before = assets::civil_from_days(days - 1);
        let price_at = |date: &str| prices.range(..=date.to_string()).next_back();
        let Some((_, end)) = price_at(to) else {
            return unavailable("missing_price", Some(to));
        };
        let quantity = self.quantity(&before);
        let end_quantity = self.quantity(to);
        if i64::try_from(quantity).is_err()
            || i64::try_from(end_quantity).is_err()
            || quantity < 0
            || end_quantity < 0
        {
            return unavailable("overflow", Some(to));
        }
        let baseline = price_at(&before);
        if quantity > 0 && baseline.is_none() {
            return unavailable("missing_baseline", Some(to));
        }
        let initial = quantity * baseline.map(|(_, p)| *p as i128).unwrap_or(0) / 1_000_000;
        let mut income = end_quantity * *end as i128 / 1_000_000 - initial;
        let mut basis = initial;
        if let Some((q, _, date)) = self
            .opening
            .as_ref()
            .filter(|(_, _, d)| d.as_str() >= from && d.as_str() <= to)
        {
            let Some((_, price)) = price_at(date) else {
                return unavailable("missing_opening_price", Some(to));
            };
            let flow = *q as i128 * *price as i128 / 1_000_000;
            income -= flow;
            basis += flow;
        }
        for (kind, q, amount, fee, date, source) in &self.operations {
            if date.as_str() < from || date.as_str() > to {
                continue;
            }
            let flow = if source == "transfer" {
                let Some((_, price)) = price_at(date) else {
                    return unavailable("missing_transfer_price", Some(to));
                };
                *q as i128 * *price as i128 / 1_000_000
            } else if kind == "BUY" {
                *amount as i128 + *fee as i128
            } else {
                *amount as i128 - *fee as i128
            };
            if kind == "BUY" {
                income -= flow;
                basis += flow;
            } else {
                income += flow;
            }
        }
        result(
            income,
            Some(basis),
            to,
            baseline.map(|(date, _)| date.as_str()),
        )
    }

    pub(super) fn has_records(&self) -> bool {
        self.opening.is_some() || !self.operations.is_empty() || !self.calibrations.is_empty()
    }

    pub(super) fn history(
        &self,
        from: &str,
        to: &str,
        prices: &BTreeMap<String, i64>,
    ) -> Vec<Value> {
        let mut dates = std::collections::BTreeSet::new();
        dates.extend(
            prices
                .keys()
                .filter(|d| d.as_str() >= from && d.as_str() <= to)
                .cloned(),
        );
        dates.extend(
            self.operations
                .iter()
                .map(|op| &op.4)
                .filter(|d| d.as_str() >= from && d.as_str() <= to)
                .cloned(),
        );
        dates.extend(
            self.calibrations
                .iter()
                .map(|cal| &cal.0)
                .filter(|d| d.as_str() >= from && d.as_str() <= to)
                .cloned(),
        );
        if let Some((_, _, date)) = &self.opening {
            if date.as_str() >= from && date.as_str() <= to {
                dates.insert(date.clone());
            }
        }
        dates.insert(to.to_string());
        let mut rows = Vec::new();
        for date in dates {
            let Some(day) = assets::days_from_civil(&date) else {
                continue;
            };
            let before = assets::civil_from_days(day - 1);
            if self.quantity(&before) == 0
                && self.quantity(&date) == 0
                && !self.operations.iter().any(|op| op.4 == date)
            {
                continue;
            }
            let mut row = if prices.contains_key(&date) {
                self.range(&date, &date, prices)
            } else {
                unavailable("missing_daily_price", Some(&date))
            };
            row["date"] = json!(date);
            rows.push(row);
        }
        rows
    }

    pub(super) fn project(
        &self,
        today: &str,
        prices: &BTreeMap<String, i64>,
        current_value: Option<i64>,
        cost: Option<i64>,
        realized: Option<i64>,
    ) -> Value {
        let latest = prices.last_key_value().map(|(d, _)| d.as_str());
        let latest_or_today = latest.unwrap_or(today);
        let holding = match (current_value, cost) {
            (Some(v), Some(c)) => result(
                v as i128 - c as i128,
                Some(c as i128),
                latest_or_today,
                None,
            ),
            _ => unavailable(
                if cost.is_none() {
                    "unknown_cost"
                } else {
                    "missing_price"
                },
                latest,
            ),
        };
        let principal = self
            .opening
            .as_ref()
            .map(|(_, cost, _)| cost.map(|c| c as i128))
            .unwrap_or(Some(0))
            .map(|opening| {
                opening
                    + self
                        .operations
                        .iter()
                        .filter(|(kind, _, _, _, d, _)| kind == "BUY" && d.as_str() <= today)
                        .map(|(_, _, amount, fee, _, _)| *amount as i128 + *fee as i128)
                        .sum::<i128>()
            });
        let cumulative = match (current_value, cost, realized, principal) {
            (Some(v), Some(c), Some(r), Some(p)) if !self.truncated => result(
                v as i128 - c as i128 + r as i128,
                Some(p),
                latest_or_today,
                None,
            ),
            _ => unavailable(
                if self.truncated {
                    "record_limit"
                } else if current_value.is_none() {
                    "missing_price"
                } else {
                    "unknown_cost"
                },
                latest,
            ),
        };
        let dated_range = |date: Option<&str>, from: Option<&str>| match (current_value, date) {
            (None, _) => unavailable("unverified_valuation", date),
            (_, Some(d)) => self.range(from.unwrap_or(d), d, prices),
            _ => unavailable("missing_baseline", None),
        };
        let days = assets::days_from_civil(today).unwrap_or(0);
        let yesterday_date = assets::civil_from_days(days - 1);
        let daily = |date: &str| {
            if prices.contains_key(date) {
                dated_range(Some(date), None)
            } else {
                unavailable("missing_daily_price", Some(date))
            }
        };
        let day = daily(today);
        let yesterday = daily(&yesterday_date);
        let week = assets::civil_from_days(days - (((days + 4).rem_euclid(7) + 6) % 7));
        let period = |from: &str| dated_range(latest, Some(from));
        json!({"day":day,"yesterday":yesterday,"week":period(&week),
            "month":period(&format!("{}-01",&today[..7])),
            "year":period(&format!("{}-01-01",&today[..4])),
            "holding":holding,"cumulative":cumulative})
    }
}

#[cfg(test)]
#[path = "positions_income_tests.rs"]
mod tests;
