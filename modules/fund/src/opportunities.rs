//! PanWatch market-scan heuristic adapted to real holdings and closed market/NAV data.
//! Donor: MIT, commit 00b345b05145705afea9ea9d5cecc090c009d83c,
//! src/modules/strategy/entry_candidates.py (thresholds and scoring).
use crate::{assets::AssetsSummary, finance::FinanceError, storage::Store, trends::KlineBar};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Features {
    pub trend: i32,
    pub macd: i32,
    pub volume_ratio: Option<f64>,
    pub change_pct: Option<f64>,
    pub turnover: Option<f64>,
    pub support: Option<f64>,
    pub close: Option<f64>,
}

/// Score is a donor heuristic, never a probability or a risk grade.
pub(crate) fn scan(f: &Features) -> (i32, i32, &'static str, Vec<&'static str>) {
    let mut points = f.trend * 2 + f.macd * 2;
    let mut tags = Vec::new();
    if f.trend > 0 {
        tags.push("trend_follow");
    }
    if f.macd > 0 {
        tags.push("macd_positive");
    }
    if let Some(v) = f.volume_ratio.filter(|v| v.is_finite()) {
        if v >= 1.8 {
            points += 1;
            tags.push("volume_breakout");
        } else if v <= 0.7 {
            points -= 1;
        }
    }
    if let Some(p) = f.change_pct.filter(|p| p.is_finite()) {
        if (1.5..=8.5).contains(&p) {
            points += 1;
            tags.push("momentum");
        } else if p >= 10.5 {
            points -= 2;
        } else if p <= -5.5 {
            points += 1;
            tags.push("rebound");
        }
    }
    if let (Some(s), Some(c)) = (f.support, f.close) {
        if s.is_finite() && c.is_finite() && s > 0.0 && s < c && c <= s * 1.03 {
            points += 1;
            tags.push("pullback");
        }
    }
    let action = if points >= 4 {
        "buy"
    } else if points >= 3 {
        "add"
    } else if points <= -3 {
        "avoid"
    } else {
        "watch"
    };
    let mut score = match action {
        "buy" => 78,
        "add" => 72,
        "avoid" => 15,
        _ => 52,
    };
    score += 8.min(tags.len() as i32 * 2);
    if let Some(p) = f.change_pct.filter(|p| p.is_finite()) {
        if (1.0..=7.0).contains(&p) {
            score += 2;
        } else if p >= 10.0 {
            score -= 3;
        }
    }
    if let Some(t) = f.turnover.filter(|t| t.is_finite()) {
        if t >= 3e9 {
            score += 3;
        } else if t >= 1e9 {
            score += 1;
        }
    }
    score += if f.trend > 0 {
        5
    } else if f.trend < 0 {
        -6
    } else {
        0
    };
    (points, score.clamp(0, 100), action, tags)
}

fn features(bars: &[KlineBar], exchange: bool) -> Option<Features> {
    if bars.len() < 60
        || bars.iter().any(|b| {
            !b.close.is_finite()
                || b.close <= 0.0
                || !crate::assets::is_valid_calendar_date(&b.date)
        })
        || bars.windows(2).any(|w| w[0].date >= w[1].date)
    {
        return None;
    }
    let closes: Vec<_> = bars.iter().map(|b| b.close).collect();
    let sma = |n| crate::trends::indicators::last_sma(&closes, n);
    let (a, b, c) = (sma(5), sma(10), sma(20));
    // Donor EMA starts from the first value; the existing SMA-seeded MACD is a different algorithm.
    let (mut fast, mut slow, mut dea) = (closes[0], closes[0], 0.0);
    for &value in &closes[1..] {
        fast += (value - fast) * 2.0 / 13.0;
        slow += (value - slow) * 2.0 / 27.0;
        dea += ((fast - slow) - dea) * 2.0 / 10.0;
    }
    let close = *closes.last()?;
    let support = bars[bars.len() - 20..]
        .iter()
        .map(|b| b.low)
        .reduce(f64::min)?;
    let volume = bars[bars.len() - 5..].iter().map(|b| b.volume).sum::<f64>() / 5.0;
    Some(Features {
        trend: if a > b && b > c {
            1
        } else if a < b && b < c {
            -1
        } else {
            0
        },
        macd: if fast - slow > dea { 1 } else { -1 },
        volume_ratio: if exchange && volume > 0.0 {
            Some(bars.last()?.volume / volume)
        } else {
            None
        },
        change_pct: Some((close / closes[closes.len() - 2] - 1.0) * 100.0),
        turnover: None,
        support: Some(support),
        close: Some(close),
    })
}

// PanWatch _build_plan adapted to closed data: references only, never orders or cash amounts.
fn conditional_plan(
    f: Option<&Features>,
    bars: &[KlineBar],
    exchange: bool,
    signal: &str,
    constraints: &[&str],
) -> (Value, Value) {
    let valid = |v: f64| v.is_finite() && v > 0.0;
    let Some((price, support)) = f.and_then(|f| {
        Some((
            f.close.filter(|v| valid(*v))?,
            f.support.filter(|v| valid(*v))?,
        ))
    }) else {
        return (
            json!({"state":"unavailable","entryRange":null,"target":null,"constraints":constraints}),
            json!({"below":null,"reason":"snapshot_changed"}),
        );
    };
    let resistance = bars
        .iter()
        .rev()
        .take(20)
        .map(|b| b.high)
        .filter(|v| valid(*v))
        .reduce(f64::max);
    let blocked = !constraints.is_empty();
    let opening = matches!(signal, "buy" | "add");
    let defensive = signal == "avoid";
    let below = if defensive {
        None
    } else {
        Some(support * if opening && exchange { 0.985 } else { 0.98 })
    };
    let target = if !exchange || blocked || defensive {
        None
    } else if opening {
        Some(
            resistance
                .filter(|r| *r > price)
                .map_or(price * 1.06, |r| r * 0.99),
        )
    } else {
        resistance.map(|r| r * 0.99)
    };
    let state = if blocked {
        "blocked"
    } else if exchange {
        "review"
    } else {
        "nav_review"
    };
    (
        json!({"state":state,"referencePrice":price,"support":support,"resistance":resistance,"entryRange":if exchange && opening && !blocked {Some([price*0.99,price*1.01])} else {None},"target":target,"constraints":constraints,"confirmation":if exchange {"manual_review"}else{"published_nav_confirmation"}}),
        json!({"below":below,"reason":if defensive {"risk_control"}else{"below_support"},"scope":"original_snapshot","refreshOn":["data_revision","holdings","cash_reconciliation","strategy_revision"]}),
    )
}

pub(crate) fn nav_bars(
    conn: &Connection,
    code: &str,
    today: &str,
) -> Result<(String, Vec<KlineBar>), FinanceError> {
    let mut stmt=conn.prepare("SELECT n.nav_date,n.unit_nav_raw,n.source FROM fund_nav n JOIN funds f ON f.id=n.fund_id WHERE f.code=?1 AND n.nav_date<=?2 AND n.unit_nav_raw>0 ORDER BY n.nav_date DESC LIMIT 250")?;
    let mut rows = stmt
        .query_map(params![code, today], |r| {
            let date: String = r.get(0)?;
            let close = r.get::<_, i64>(1)? as f64 / 10000.0;
            Ok((
                KlineBar {
                    date,
                    close,
                    open: close,
                    high: close,
                    low: close,
                    volume: 0.0,
                    amount: 0.0,
                    pct: 0.0,
                    turnover: 0.0,
                },
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let source = rows
        .first()
        .map(|r| r.1.clone())
        .unwrap_or_else(|| "fund_nav".into());
    rows.reverse();
    Ok((source, rows.into_iter().map(|r| r.0).collect()))
}

fn identity(kind: &str, market: &str, code: &str) -> String {
    if kind == "fund" {
        format!("fund:{code}")
    } else {
        format!("security:{market}:{code}")
    }
}

pub(crate) fn projection(
    conn: &Connection,
    summary: &AssetsSummary,
    today: &str,
    available: Option<i64>,
) -> Result<Value, FinanceError> {
    let revision = Store::data_revision(conn)?;
    let strategy_cfg = crate::strategies::effective_weights(conn)?;
    let strategy_revision = strategy_cfg["revision"].as_i64().unwrap_or(1);
    let snapshot = format!("opportunity:{revision}:{today}:1:{strategy_revision}");
    // Sorted, bounded inputs; no network, job or second holdings authority lives here.
    let mut pool = BTreeMap::<String, (String, String, String, String, bool, bool)>::new();
    for row in &summary.funds {
        let Ok((kind, market, code)) = crate::ledger::instrument_parts(&row.fund_code) else {
            continue;
        };
        pool.insert(
            identity(kind, market, code),
            (
                row.fund_name.clone(),
                kind.into(),
                market.into(),
                code.into(),
                true,
                false,
            ),
        );
    }
    for row in &summary.manual {
        if !["stock", "etf"].contains(&row.product_type.as_str()) {
            continue;
        }
        let parts: Vec<_> = row.instrument_key.split(':').collect();
        if let ["security", market, code] = parts.as_slice() {
            pool.entry(identity(&row.product_type, market, code))
                .or_insert((
                    row.name.clone(),
                    row.product_type.clone(),
                    (*market).into(),
                    (*code).into(),
                    true,
                    false,
                ));
        }
    }
    let mut stmt = conn
        .prepare("SELECT symbol,name,asset_type FROM watchlist_items ORDER BY symbol LIMIT 501")?;
    let watch = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (symbol, name, kind) in watch {
        let normalized = crate::market::normalize_symbol(&symbol);
        let (market, code) = if kind == "fund" {
            ("CN", symbol.as_str())
        } else if let Some(c) = normalized.strip_prefix("sh") {
            ("SSE", c)
        } else if let Some(c) = normalized.strip_prefix("sz") {
            ("SZSE", c)
        } else if let Some(c) = normalized.strip_prefix("bj") {
            ("BSE", c)
        } else if let Some(c) = normalized.strip_prefix("hk") {
            ("HK", c)
        } else if let Some(c) = normalized.strip_prefix("us") {
            ("US", c)
        } else {
            continue;
        };
        if code.is_empty()
            || code.len() > 20
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            continue;
        }
        let kind = if kind == "etf" {
            "etf"
        } else if kind == "fund" {
            "fund"
        } else {
            "stock"
        };
        pool.entry(identity(kind, market, code))
            .and_modify(|row| row.5 = true)
            .or_insert((name, kind.into(), market.into(), code.into(), false, true));
    }
    // 市场发现有界池（v23）：只读留存表 ≤30 只，pool="market"；冷启动为空，
    // 绝不联网，不冒称全市场扫描。
    for e in crate::opportunity_discovery::projection_entries(conn)? {
        let id = crate::opportunity_discovery::identity(&e);
        pool.entry(id)
            .or_insert((e.name, e.kind.into(), e.market.into(), e.code, false, false));
    }
    let input_count = pool.len();
    let mut items = Vec::new();
    for (id, (name, kind, market, code, holding, watch)) in pool.into_iter().take(100) {
        let exchange = kind != "fund";
        let (source, bars) = if exchange {
            crate::market::cached_closed_bars(&market, &code, today)
                .map(|(v, b)| {
                    (
                        v["source"]
                            .as_str()
                            .unwrap_or("web.ifzq.gtimg.cn")
                            .to_string(),
                        b,
                    )
                })
                .unwrap_or_default()
        } else {
            nav_bars(conn, &code, today)?
        };
        let feat = features(&bars, exchange);
        let date = bars.last().map(|b| b.date.as_str());
        let stale = date
            .and_then(crate::assets::days_from_civil)
            .is_none_or(|d| crate::assets::days_from_civil(today).unwrap_or(d) - d > 7);
        let mut risks = vec!["liquidity_unverified"];
        if !exchange {
            risks.push("fund_exposure_unknown");
        }
        if stale {
            risks.push("stale_data");
        }
        let (points, score, signal, tags) = feat
            .as_ref()
            .map(scan)
            .map(|(p, s, a, t)| (Some(p), Some(s), a, t))
            .unwrap_or((None, None, "watch", vec![]));
        let (rank, strategy_weights) =
            crate::strategies::weighted_rank(&strategy_cfg, &tags, score.unwrap_or(0));
        let mut constraints = Vec::new();
        if feat.is_none() {
            constraints.push("insufficient_history");
        }
        if stale {
            constraints.push("stale_data");
        }
        if holding
            && (summary.funds.iter().any(|r| {
                crate::ledger::instrument_parts(&r.fund_code)
                    .is_ok_and(|(k, m, c)| identity(k, m, c) == id)
                    && (r.value_minor.is_none() || r.stale)
            }) || summary
                .manual
                .iter()
                .any(|r| r.instrument_key == id && (r.stale || r.overlap_review)))
        {
            constraints.push("valuation_unverified");
        }
        if available.is_none_or(|a| a <= 0) {
            constraints.push("cash_unverified");
        }
        let (conditions, invalidation) =
            conditional_plan(feat.as_ref(), &bars, exchange, signal, &constraints);
        // Candidate actions invite review; they do not claim an executable amount/order.
        let action = if !constraints.is_empty() {
            "observe"
        } else {
            match (signal, holding) {
                ("buy" | "add", true) => "review_add",
                ("buy" | "add", false) => "build_candidate",
                ("avoid", true) => "review_reduce",
                ("avoid", false) => "avoid",
                (_, true) => "hold",
                _ => "observe",
            }
        };
        items.push(json!({"dataRevision":revision,"securityId":id,"name":name,"code":code,"kind":kind,"market":market,"pool":if holding&&watch {"mixed"} else if holding {"holdings"} else if watch {"watchlist"} else {"market"},"isHolding":holding,"strategyVersion":"panwatch-scan-adapted-v1","parameterVersion":1,"signal":if feat.is_some(){Some(signal)}else{None},"score":score,"rankScore":rank,"strategyWeights":strategy_weights,"points":points,"riskCodes":risks,"riskLevel":"unknown","action":action,"constraints":constraints,"quality":if feat.is_none(){"insufficient_data"}else if stale{"stale"}else{"ready"},"asOf":date,"source":source,"period":if exchange {"closed-day"} else {"published-nav"},"strategyTags":tags,"evidence":feat.map(|f|json!({"maArrangement":f.trend,"macdState":f.macd,"volumeRatio":f.volume_ratio,"lastChangePct":f.change_pct,"support":f.support,"close":f.close})),"amountRecommendation":null,"planVersion":"panwatch-plan-adapted-v1","conditions":conditions,"invalidation":invalidation}));
    }
    items.sort_by(|a, b| {
        b["rankScore"]
            .as_i64()
            .cmp(&a["rankScore"].as_i64())
            .then_with(|| a["securityId"].as_str().cmp(&b["securityId"].as_str()))
    });
    let digest = app_runtime_core::sha256_hex(
        serde_json::to_string(&items)
            .map_err(|_| FinanceError::Invalid("机会序列化失败".into()))?
            .as_bytes(),
    );
    let snapshot = format!("{snapshot}:{digest}");
    for item in &mut items {
        item["snapshotId"] = snapshot.clone().into();
        item["candidateId"] =
            format!("{snapshot}:{}", item["securityId"].as_str().unwrap_or("")).into();
    }
    Ok(
        json!({"snapshotId":snapshot,"dataRevision":revision,"algorithmVersion":"panwatch-scan-adapted-v1","strategyRevision":strategy_revision,"inputCount":input_count,"limited":input_count>100,"poolCoverage":"recorded-holdings-watchlist-and-bounded-market","items":items}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_plan_matches_donor_prices_and_respects_data_cash_and_nav() {
        let f = Features {
            close: Some(100.0),
            support: Some(90.0),
            ..Default::default()
        };
        let bars = vec![KlineBar {
            date: "2026-01-01".into(),
            open: 100.0,
            high: 120.0,
            low: 90.0,
            close: 100.0,
            volume: 10.0,
            amount: 0.0,
            pct: 0.0,
            turnover: 0.0,
        }];
        for signal in ["buy", "add", "watch", "hold", "avoid"] {
            let (p, i) = conditional_plan(Some(&f), &bars, true, signal, &[]);
            let fixture: Value =
                serde_json::from_str(include_str!("../tests/opportunity-plan-parity.json"))
                    .unwrap();
            let donor = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["action"] == signal)
                .unwrap();
            assert_eq!(p["entryRange"], donor["entryRange"]);
            assert_eq!(p["target"], donor["target"]);
            assert_eq!(i["below"], donor["below"]);
            assert_eq!(p["state"], "review");
            if matches!(signal, "buy" | "add") {
                assert_eq!(p["entryRange"], json!([99.0, 101.0]));
                assert!((p["target"].as_f64().unwrap() - 118.8).abs() < 1e-9);
                assert!((i["below"].as_f64().unwrap() - 88.65).abs() < 1e-9);
            } else if signal == "avoid" {
                assert!(i["below"].is_null());
                assert!(p["target"].is_null());
            } else {
                assert_eq!(i["below"], 88.2);
            }
        }
        let (p, _) = conditional_plan(Some(&f), &bars, true, "buy", &["cash_unverified"]);
        assert_eq!(p["state"], "blocked");
        assert!(p["entryRange"].is_null());
        assert!(p["target"].is_null());
        let (p, i) = conditional_plan(Some(&f), &bars, false, "buy", &[]);
        assert_eq!(p["state"], "nav_review");
        assert!(p["entryRange"].is_null());
        assert!(p["target"].is_null());
        assert_eq!(i["below"], 88.2);
        for price in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let bad = Features {
                close: Some(price),
                ..f
            };
            assert_eq!(
                conditional_plan(Some(&bad), &bars, true, "buy", &[]).0["state"],
                "unavailable"
            );
        }
        assert_eq!(
            conditional_plan(None, &[], true, "buy", &[]).0["state"],
            "unavailable"
        );
    }
    #[test]
    fn donor_scan_threshold_and_score_parity() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/opportunity-parity.json")).unwrap();
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 756);
        for row in fixture["cases"].as_array().unwrap() {
            let f = Features {
                trend: row["trend"].as_i64().unwrap() as i32,
                macd: row["macd"].as_i64().unwrap() as i32,
                volume_ratio: row["volume"].as_f64(),
                change_pct: row["pct"].as_f64(),
                turnover: row["turnover"].as_f64(),
                support: row["support"].as_f64(),
                close: row["close"].as_f64(),
            };
            let (points, score, action, tags) = scan(&f);
            assert_eq!(
                json!({"points":points,"score":score,"action":action,"tags":tags}),
                json!({"points":row["points"],"score":row["score"].as_f64().unwrap() as i32,"action":row["action"],"tags":row["tags"]}),
                "input {row}"
            );
        }
    }
    #[test]
    fn production_home_projection_deduplicates_accounts_and_never_adds_unheld_or_invents_missing_scores(
    ) {
        let store = Store::open_in_memory().unwrap();
        let today = crate::finance::today_cn();
        store.with_write(|c|->rusqlite::Result<()>{
            c.execute("INSERT INTO watchlists(id,name,created_at) VALUES('test','隔离','0')",[])?;
            for (symbol,kind) in [("sh600519","stock"),("hk00700","stock"),("usAAPL","stock"),("110022","fund")] {
                c.execute("INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES('test',?1,'隔离',?2,'0')",params![symbol,kind])?;
            }
            c.execute("INSERT INTO funds(code,name,updated_at) VALUES('110022','隔离基金','0')",[])?;
            for day in 1..=60 {let date=crate::assets::civil_from_days(crate::assets::days_from_civil(&today).unwrap()-61+day);c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,?2,'isolated','0')",params![date,10000+day*100])?;}
            Ok(())
        }).unwrap();
        let home = crate::home::build_snapshot(&store, "all").unwrap();
        let items = home["opportunities"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        let fund = items.iter().find(|i| i["kind"] == "fund").unwrap();
        assert_eq!(fund["quality"], "ready");
        assert_eq!(fund["action"], "observe");
        assert!(fund["evidence"]["volumeRatio"].is_null());
        assert!(fund["constraints"]
            .as_array()
            .unwrap()
            .contains(&json!("cash_unverified")));
        for row in items.iter().filter(|i| i["kind"] != "fund") {
            assert!(row["score"].is_null());
            assert_eq!(row["quality"], "insufficient_data");
            assert_eq!(row["action"], "observe");
        }
        let out = store
            .with_read(|c| projection(c, &AssetsSummary::default(), &today, Some(1000)))
            .unwrap();
        let fund = out["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == "fund")
            .unwrap();
        assert_eq!(fund["action"], "build_candidate");
        assert_eq!(fund["riskLevel"], "unknown");
        assert!(!fund["strategyTags"]
            .as_array()
            .unwrap()
            .contains(&json!("volume_breakout")));
        assert_eq!(fund["period"], "published-nav");
        assert_eq!(fund["source"], "isolated");
        assert_eq!(
            out["snapshotId"],
            store
                .with_read(|c| projection(c, &AssetsSummary::default(), &today, Some(1000)))
                .unwrap()["snapshotId"]
        );
        let old_id = out["snapshotId"].clone();
        store.with_read(|c|->rusqlite::Result<()>{
            // A source correction without a ledger revision still changes candidate facts/identity.
            c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,16000,'same-day-published','0')",[&today])?;
            let future=crate::assets::civil_from_days(crate::assets::days_from_civil(&today).unwrap()+1);
            c.execute("INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,999999,'future-excluded','0')",[future])?;
            Ok(())
        }).unwrap();
        let current = store
            .with_read(|c| projection(c, &AssetsSummary::default(), &today, Some(1000)))
            .unwrap();
        assert_eq!(current["dataRevision"], out["dataRevision"]);
        assert_ne!(current["snapshotId"], old_id);
        let fund = current["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == "fund")
            .unwrap();
        assert_eq!(fund["asOf"], today);
        assert_eq!(fund["source"], "same-day-published");
        assert_eq!(fund["evidence"]["close"], 1.6); // NAV raw uses four decimal places.
        let mut summary = AssetsSummary::default();
        for account_id in [1, 2] {
            summary.manual.push(crate::assets::ManualAssetRow {
                id: account_id,
                account_id,
                account_name: "隔离".into(),
                product_type: "stock".into(),
                instrument_key: "security:SSE:600519".into(),
                name: "隔离".into(),
                risk_class: "equity".into(),
                market_value_minor: 100,
                valued_at: today.clone(),
                stale: false,
                overlap_review: false,
                legacy_overlap_distinct: false,
            });
        }
        let out = store
            .with_read(|c| projection(c, &summary, &today, Some(1000)))
            .unwrap();
        let stock: Vec<_> = out["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["securityId"] == "security:SSE:600519")
            .collect();
        assert_eq!(stock.len(), 1);
        assert_eq!(stock[0]["isHolding"], true);
        assert_eq!(stock[0]["pool"], "mixed");
        assert_eq!(stock[0]["action"], "observe");
        summary.manual[0].stale = true;
        let stale = store
            .with_read(|c| projection(c, &summary, &today, Some(1000)))
            .unwrap();
        let stock = stale["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["securityId"] == "security:SSE:600519")
            .unwrap();
        assert!(stock["constraints"]
            .as_array()
            .unwrap()
            .contains(&json!("valuation_unverified")));
    }
}
