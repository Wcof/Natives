//! 机会报价观察域测试（R2 R0 / A37、A38）：方向政策边界、日期序列、状态桶、迁移幂等与重放契约。

use super::*;
use crate::storage::Store;

fn fund(store: &Store, code: &str) -> i64 {
    store
        .with_write(|c| -> Result<i64, rusqlite::Error> {
            if let Some(id) = c
                .query_row("SELECT id FROM funds WHERE code=?1", [code], |r| r.get(0))
                .optional()?
            {
                return Ok(id);
            }
            c.execute(
                "INSERT INTO funds(code,name,updated_at) VALUES(?1,'隔离基金','0')",
                [code],
            )?;
            Ok(c.last_insert_rowid())
        })
        .unwrap()
}

fn nav(store: &Store, code: &str, date: &str, raw: i64) {
    let id = fund(store, code);
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(?1,?2,?3,'isolated','0')",
                params![id, date, raw],
            )
        })
        .unwrap();
}

/// 来源更正：更新已存在净值行的原始值（同键覆盖，不新增记录）。
fn update_nav(store: &Store, code: &str, date: &str, raw: i64) {
    let id = fund(store, code);
    store
        .with_write(|c| {
            c.execute(
                "UPDATE fund_nav SET unit_nav_raw=?1 WHERE fund_id=?2 AND nav_date=?3",
                params![raw, id, date],
            )
        })
        .unwrap();
}

fn day(offset: i64) -> String {
    crate::assets::civil_from_days(
        crate::assets::days_from_civil(&crate::finance::today_cn()).unwrap() + offset,
    )
}

/// 绝对 civil day（1970-01-01 起算）转日期，用于以生成日为基准构造序列。
fn abs_day(days: i64) -> String {
    crate::assets::civil_from_days(days)
}

/// 从 today 往回找至少 weeks_back*7+10 天前的周五作为信号生成日，保证其后有
/// 足够工作日且全部早于今天。1970-01-01 为周四：(days+4)%7，5 = 周五。
fn friday_before_today(weeks_back: i64) -> String {
    let today = crate::assets::days_from_civil(&crate::finance::today_cn()).unwrap();
    let mut offset = today;
    while (offset + 4).rem_euclid(7) != 5 || today - offset < weeks_back * 7 + 10 {
        offset -= 1;
    }
    crate::assets::civil_from_days(offset)
}

/// start_days 之后第 count 个工作日（跳过周六周日；有效报价序列即交易日历）。
fn weekday_after(start_days: i64, count: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = start_days + 1;
    while out.len() < count {
        let weekday = (cursor + 4).rem_euclid(7);
        if weekday != 0 && weekday != 6 {
            out.push(crate::assets::civil_from_days(cursor));
        }
        cursor += 1;
    }
    out
}

fn saved_batch(store: &Store, sid: &str, generated_day: &str, items: Value) {
    let projection = json!({
        "snapshotId": sid,
        "dataRevision": "7",
        "algorithmVersion": "panwatch-scan-adapted-v1",
        "inputCount": 1,
        "limited": false,
        "poolCoverage": "recorded-holdings-and-watchlist",
        "generatedAt": (crate::assets::days_from_civil(generated_day).unwrap() * 86400 - 8 * 3600).to_string(),
        "generatedDay": generated_day,
        "items": items,
    });
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO opportunity_snapshots(snapshot_id,facts_key,data_revision,generated_at,projection_json) VALUES(?1,?2,'7','0',?3)",
                params![sid, format!("facts:{sid}"), projection.to_string()],
            )
        })
        .unwrap();
}

fn item(code: &str, signal: Option<&str>) -> Value {
    let mut row = json!({
        "dataRevision": "7",
        "securityId": format!("fund:{code}"),
        "name": "隔离基金",
        "code": code,
        "kind": "fund",
        "market": "CN",
        "pool": "watchlist",
        "isHolding": false,
        "strategyVersion": "panwatch-scan-adapted-v1",
        "parameterVersion": 1,
        "score": null,
        "rankScore": null,
        "points": null,
        "riskCodes": [],
        "riskLevel": "unknown",
        "action": "observe",
        "constraints": [],
        "quality": "ready",
        "asOf": day(-30),
        "source": "isolated",
        "period": "published-nav",
        "strategyTags": [],
        "evidence": null,
        "amountRecommendation": null,
    });
    if let Some(signal) = signal {
        row["signal"] = signal.into();
    }
    row["candidateId"] = format!("snapshot-x:fund:{code}").into();
    row
}

fn refresh_batch(store: &Store, sid: &str, request_id: &str) -> Result<Value, FinanceError> {
    refresh(store, &json!({"requestId": request_id, "snapshotId": sid}))
}

fn row_of(store: &Store, sid: &str, cid: &str, horizon: i64) -> Value {
    let raw: String = store
        .with_read(|c| {
            c.query_row(
                "SELECT result_json FROM opportunity_observations WHERE snapshot_id=?1 AND candidate_id=?2 AND horizon=?3",
                params![sid, cid, horizon],
                |r| r.get(0),
            )
            .map_err(FinanceError::from)
        })
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn cid_of(code: &str) -> String {
    format!("snapshot-x:fund:{code}")
}

#[test]
fn donor_direction_policy_boundaries() {
    // 买／加：严格大于 0 命中，零涨幅不算命中。
    assert_eq!(classify("buy", 1), Some(true));
    assert_eq!(classify("buy", 0), Some(false));
    assert_eq!(classify("add", -1), Some(false));
    // 回避：严格小于 0 命中。
    assert_eq!(classify("avoid", -1), Some(true));
    assert_eq!(classify("avoid", 0), Some(false));
    assert_eq!(classify("avoid", 1), Some(false));
    // 持有／观察：绝对涨幅严格小于 2%（±20000 ppm 边界不命中）。
    assert_eq!(classify("watch", 19_999), Some(true));
    assert_eq!(classify("watch", -19_999), Some(true));
    assert_eq!(classify("watch", 20_000), Some(false));
    assert_eq!(classify("watch", -20_000), Some(false));
    assert_eq!(classify("watch", 0), Some(true));
    assert_eq!(classify("hold", 20_000), Some(false));
    // 未知信号与约束后 action 不可评估。
    assert_eq!(classify("review_add", 100), None);
    assert_eq!(classify("", 100), None);
    assert_eq!(direction("buy"), Some("up"));
    assert_eq!(direction("avoid"), Some("down"));
    assert_eq!(direction("watch"), Some("flat"));
    assert_eq!(direction("review_add"), None);
}

#[test]
fn fresh_batch_via_generation_stays_immature_and_counts_all_candidates() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES(1,'110022','隔离','fund',CURRENT_TIMESTAMP)",
                [],
            )
        })
        .unwrap();
    let today = crate::finance::today_cn();
    for i in 1..=60 {
        let date = crate::assets::civil_from_days(
            crate::assets::days_from_civil(&today).unwrap() - 61 + i,
        );
        nav(&store, "110022", &date, 10000 + i * 10);
    }
    let home = crate::opportunity_records::generate_home(&store, "all", None, None).unwrap();
    let saved = home["opportunities"]["recordedSnapshot"]["snapshotId"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!home["opportunities"]["items"][0]["signal"].is_null());
    // candidateId 以留存快照为准：自动生成返回的当前投影 revision 可能已前进。
    let cid = crate::opportunity_records::snapshots(&store, 1).unwrap()["items"][0]["items"][0]
        ["candidateId"]
        .as_str()
        .unwrap()
        .to_string();
    let summary = refresh_batch(&store, &saved, "fresh-1").unwrap();
    assert_eq!(summary["candidates"], 1);
    for horizon in ["3", "5", "10"] {
        assert_eq!(summary["horizons"][horizon]["immature"], 1, "{horizon}");
        let row = row_of(&store, &saved, &cid, horizon.parse().unwrap());
        assert_eq!(row["status"], "immature");
        assert!(row["baseline"].is_null(), "生成日当天报价不作起点");
    }
    // 统计：全部保存候选进入覆盖分母；无成熟样本命中率为 null 而非 0%。
    let stats = crate::opportunity_records::stats(&store).unwrap();
    let coverage = &stats["observations"]["horizons"];
    for horizon in ["3", "5", "10"] {
        assert_eq!(coverage[horizon]["total"], 1);
        assert_eq!(coverage[horizon]["immature"], 1);
        assert_eq!(coverage[horizon]["mature"], 0);
        assert!(coverage[horizon]["directions"]["up"]["hitRatePpm"].is_null());
    }
    assert_eq!(
        stats["observations"]["metric"],
        "direction-hit-not-useful-rate-not-actual-return"
    );
}

#[test]
fn mature_windows_directions_zero_change_and_fixity() {
    let store = Store::open_in_memory().unwrap();
    let generated = friday_before_today(4);
    let generated_days = crate::assets::days_from_civil(&generated).unwrap();
    let all_quotes = weekday_after(generated_days, 12);
    let quotes = &all_quotes[..8];
    // 买：报价上涨；回避：下跌；观察：零涨幅（平稳命中，涨跌都不命中）。
    for (code, rows) in [
        (
            "110022",
            vec![10500i64, 10600, 10700, 10800, 10900, 11000, 11100, 11200],
        ),
        (
            "110023",
            vec![9500, 9400, 9300, 9200, 9100, 9000, 8900, 8800],
        ),
        (
            "110024",
            vec![10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000],
        ),
    ] {
        fund(&store, code);
        nav(&store, code, &generated, 10000);
        for (index, date) in quotes.iter().enumerate() {
            nav(&store, code, date, rows[index]);
        }
    }
    saved_batch(
        &store,
        "snapshot-x",
        &generated,
        json!([
            item("110022", Some("buy")),
            item("110023", Some("avoid")),
            item("110024", Some("watch")),
        ]),
    );
    let first = refresh_batch(&store, "snapshot-x", "window-1").unwrap();
    for horizon in ["3", "5"] {
        assert_eq!(first["horizons"][horizon]["mature"], 3, "{horizon}");
    }
    assert_eq!(first["horizons"]["10"]["immature"], 3);
    // 生成日为周五：起点是之后首个工作日（跳过周末），3 个有效报价止于第三个。
    let buy = row_of(&store, "snapshot-x", &cid_of("110022"), 3);
    assert_eq!(buy["status"], "mature");
    assert_eq!(buy["baseline"]["date"], quotes[0]);
    assert_eq!(buy["end"]["date"], quotes[2]);
    assert_eq!(buy["baseline"]["priceRaw"], 10500);
    assert_eq!(buy["end"]["priceRaw"], 10700);
    assert_eq!(buy["hit"], true, "buy +5% 命中");
    assert_eq!(buy["returnPpm"], (10700 - 10500) * 1_000_000 / 10500);
    let avoid = row_of(&store, "snapshot-x", &cid_of("110023"), 3);
    assert_eq!(avoid["hit"], true, "avoid 下跌命中");
    assert_eq!(avoid["baseline"]["priceRaw"], 9500);
    let watch = row_of(&store, "snapshot-x", &cid_of("110024"), 3);
    assert_eq!(watch["returnPpm"], 0);
    assert_eq!(watch["hit"], true, "零涨幅对 watch 平稳命中");
    // 10 日区间未成熟：起点已知，窗口未凑齐。
    let buy10 = row_of(&store, "snapshot-x", &cid_of("110022"), 10);
    assert_eq!(buy10["status"], "immature");
    assert_eq!(buy10["quoteCount"], 8);
    assert_eq!(buy10["baseline"]["date"], quotes[0]);
    // 统计方向分开：up／down／flat 各 1 成熟 1 命中；未成熟不计输赢。
    let stats = crate::opportunity_records::stats(&store).unwrap();
    let coverage = &stats["observations"]["horizons"];
    assert_eq!(coverage["5"]["total"], 3);
    assert_eq!(coverage["5"]["mature"], 3);
    assert_eq!(coverage["5"]["directions"]["up"]["total"], 1);
    assert_eq!(coverage["5"]["directions"]["up"]["hits"], 1);
    assert_eq!(coverage["5"]["directions"]["down"]["total"], 1);
    assert_eq!(coverage["5"]["directions"]["down"]["hits"], 1);
    assert_eq!(coverage["5"]["directions"]["flat"]["total"], 1);
    assert_eq!(coverage["5"]["directions"]["flat"]["hits"], 1);
    assert_eq!(coverage["10"]["immature"], 3);
    assert_eq!(coverage["10"]["directions"]["up"]["total"], 0);
    assert!(coverage["10"]["directions"]["up"]["hitRatePpm"].is_null());
    // 成熟结果固定：补齐 10 日窗口后再刷新，已成熟的 3/5 日观察不被改写。
    let first_row = row_of(&store, "snapshot-x", &cid_of("110022"), 3);
    for (code, rows) in [
        ("110022", vec![11300i64, 11400, 11500, 11600]),
        ("110023", vec![8700, 8600, 8500, 8400]),
        ("110024", vec![10000, 10000, 10000, 10000]),
    ] {
        for (index, date) in all_quotes[8..].iter().enumerate() {
            nav(&store, code, date, rows[index]);
        }
    }
    let second = refresh_batch(&store, "snapshot-x", "window-2").unwrap();
    assert_eq!(second["horizons"]["10"]["mature"], 3);
    assert_eq!(
        row_of(&store, "snapshot-x", &cid_of("110022"), 3),
        first_row,
        "成熟结果首次保存后固定"
    );
    assert_eq!(
        row_of(&store, "snapshot-x", &cid_of("110024"), 5)["observedAt"],
        first_row["observedAt"]
    );
}

#[test]
fn flat_threshold_two_percent_boundary_is_not_a_hit() {
    let store = Store::open_in_memory().unwrap();
    let generated = friday_before_today(4);
    let quotes = weekday_after(crate::assets::days_from_civil(&generated).unwrap(), 5);
    // 恰好 +2%（20000 ppm）与 -2%：平稳阈值严格小于，边界不命中。
    for (code, end) in [("110031", 10200i64), ("110032", 9800)] {
        fund(&store, code);
        nav(&store, code, &generated, 10000);
        for (index, date) in quotes.iter().enumerate() {
            nav(&store, code, date, if index == 4 { end } else { 10000 });
        }
    }
    saved_batch(
        &store,
        "snapshot-boundary",
        &generated,
        json!([item("110031", Some("watch")), item("110032", Some("watch"))]),
    );
    refresh_batch(&store, "snapshot-boundary", "boundary-1").unwrap();
    let fixed_up = row_of(&store, "snapshot-boundary", &cid_of("110031"), 5);
    let fixed_down = row_of(&store, "snapshot-boundary", &cid_of("110032"), 5);
    assert_eq!(fixed_up["status"], "mature");
    assert_eq!(fixed_up["returnPpm"], 20000);
    assert_eq!(fixed_up["hit"], false, "恰好 +2% 不算平稳命中");
    assert_eq!(fixed_down["returnPpm"], -20000);
    assert_eq!(fixed_down["hit"], false, "恰好 -2% 不算平稳命中");
    // 生成后的净值来源更正不改写已固定成熟结果。
    update_nav(&store, "110031", &quotes[4], 10199);
    refresh_batch(&store, "snapshot-boundary", "boundary-2").unwrap();
    assert_eq!(
        row_of(&store, "snapshot-boundary", &cid_of("110031"), 5),
        fixed_up
    );
    // 低于阈值命中：+19900 ppm（独立隔离批次）。
    let store2 = Store::open_in_memory().unwrap();
    let generated2 = friday_before_today(4);
    let quotes2 = weekday_after(crate::assets::days_from_civil(&generated2).unwrap(), 5);
    fund(&store2, "110033");
    nav(&store2, "110033", &generated2, 10000);
    for (index, date) in quotes2.iter().enumerate() {
        nav(
            &store2,
            "110033",
            date,
            if index == 4 { 10199 } else { 10000 },
        );
    }
    saved_batch(
        &store2,
        "snapshot-boundary2",
        &generated2,
        json!([item("110033", Some("watch"))]),
    );
    refresh_batch(&store2, "snapshot-boundary2", "boundary-3").unwrap();
    let row = row_of(&store2, "snapshot-boundary2", &cid_of("110033"), 5);
    assert_eq!(row["returnPpm"], 19900);
    assert_eq!(row["hit"], true);
}

#[test]
fn missing_price_window_lost_old_signal_and_future_quotes() {
    let store = Store::open_in_memory().unwrap();
    let generated = friday_before_today(4);
    let generated_days = crate::assets::days_from_civil(&generated).unwrap();
    let quotes = weekday_after(generated_days, 8);
    // 缺价：生成日后无报价且留存序列早已停更（>7 天，疑似停牌或退市）。
    fund(&store, "110041");
    nav(&store, "110041", &day(-60), 10000);
    nav(&store, "110041", &day(-59), 10100);
    // 窗口丢失：留存 NAV 序列起点晚于生成日（早期记录已被淘汰）。
    fund(&store, "110042");
    for i in 1..=30 {
        nav(&store, "110042", &abs_day(generated_days + i), 10000 + i);
    }
    // 停牌：起点之后只有 2 个报价，窗口凑不满（未成熟，起点已知）。
    fund(&store, "110043");
    nav(&store, "110043", &generated, 10000);
    nav(&store, "110043", &quotes[0], 10050);
    nav(&store, "110043", &quotes[1], 10100);
    // 未来报价不进入：起点后 3 个报价 + 一条未来日期记录。
    fund(&store, "110044");
    nav(&store, "110044", &generated, 10000);
    for (index, date) in quotes.iter().take(3).enumerate() {
        nav(&store, "110044", date, 10000 + (index as i64 + 1) * 100);
    }
    nav(&store, "110044", &day(5), 999999);
    // 旧记录没有原始 signal：不按当前分数或 action 反推。
    fund(&store, "110045");
    nav(&store, "110045", &generated, 10000);
    for (index, date) in quotes.iter().take(5).enumerate() {
        nav(&store, "110045", date, 10000 + (index as i64 + 1) * 100);
    }
    let mut legacy = item("110045", None);
    legacy["action"] = "review_add".into();
    legacy["score"] = 90.into();
    saved_batch(
        &store,
        "snapshot-x",
        &generated,
        json!([
            item("110041", Some("buy")),
            item("110042", Some("buy")),
            item("110043", Some("buy")),
            item("110044", Some("buy")),
            legacy,
        ]),
    );
    let summary = refresh_batch(&store, "snapshot-x", "gaps-1").unwrap();
    assert_eq!(summary["horizons"]["3"]["missingPrice"], 1);
    assert_eq!(summary["horizons"]["3"]["windowLost"], 1);
    assert_eq!(summary["horizons"]["3"]["immature"], 1);
    assert_eq!(summary["horizons"]["3"]["mature"], 1);
    assert_eq!(summary["horizons"]["3"]["signalUnknown"], 1);
    let missing = row_of(&store, "snapshot-x", &cid_of("110041"), 3);
    assert_eq!(missing["status"], "missing_price");
    assert_eq!(missing["baseline"], json!(null));
    assert_eq!(missing["seriesLast"], day(-59));
    let lost = row_of(&store, "snapshot-x", &cid_of("110042"), 3);
    assert_eq!(lost["status"], "window_lost");
    assert!(lost["baseline"].is_null(), "窗口丢失不给起点");
    assert!(lost["seriesFirst"].as_str().unwrap() > generated.as_str());
    let suspended = row_of(&store, "snapshot-x", &cid_of("110043"), 3);
    assert_eq!(suspended["status"], "immature");
    assert_eq!(suspended["quoteCount"], 2);
    assert_eq!(suspended["baseline"]["date"], quotes[0]);
    let future = row_of(&store, "snapshot-x", &cid_of("110044"), 3);
    assert_eq!(future["status"], "mature");
    assert_eq!(future["end"]["date"], quotes[2], "未来报价不进入窗口");
    assert_eq!(future["quoteCount"], 3);
    let legacy_row = row_of(&store, "snapshot-x", &cid_of("110045"), 3);
    assert_eq!(legacy_row["status"], "signal_unknown");
    assert!(legacy_row["baseline"].is_null());
    assert!(legacy_row["returnPpm"].is_null());
    assert!(legacy_row["hit"].is_null());
    // 统计口径：signal 未知优先于行状态，不进入方向命中分母。
    let stats = crate::opportunity_records::stats(&store).unwrap();
    let coverage = &stats["observations"]["horizons"]["3"];
    assert_eq!(coverage["total"], 5);
    assert_eq!(coverage["signalUnknown"], 1);
    assert_eq!(coverage["missingPrice"], 1);
    assert_eq!(coverage["windowLost"], 1);
    assert_eq!(coverage["immature"], 1);
    assert_eq!(coverage["mature"], 1);
    assert_eq!(coverage["directions"]["up"]["total"], 1);
    assert_eq!(coverage["directions"]["up"]["hits"], 1);
}

#[test]
fn generation_day_quote_never_becomes_baseline_and_retry_completes_window() {
    let store = Store::open_in_memory().unwrap();
    let generated = friday_before_today(3);
    let generated_days = crate::assets::days_from_civil(&generated).unwrap();
    let all_quotes = weekday_after(generated_days, 10);
    let quotes = &all_quotes[..3];
    fund(&store, "110051");
    nav(&store, "110051", &generated, 10000);
    for (index, date) in quotes.iter().enumerate() {
        nav(&store, "110051", date, 10100 + index as i64 * 100);
    }
    saved_batch(
        &store,
        "snapshot-x",
        &generated,
        json!([item("110051", Some("add"))]),
    );
    let first = refresh_batch(&store, "snapshot-x", "grow-1").unwrap();
    assert_eq!(first["horizons"]["3"]["mature"], 1);
    let row3 = row_of(&store, "snapshot-x", &cid_of("110051"), 3);
    assert_eq!(
        row3["baseline"]["date"], quotes[0],
        "生成日当天报价不作起点"
    );
    assert_eq!(row3["baseline"]["priceRaw"], 10100);
    // 未成熟 → 成熟：补齐 5/10 日后重试，起点保持不变。
    for (index, date) in all_quotes[3..].iter().enumerate() {
        nav(&store, "110051", date, 10400 + index as i64 * 50);
    }
    let second = refresh_batch(&store, "snapshot-x", "grow-2").unwrap();
    assert_eq!(second["horizons"]["5"]["mature"], 1);
    assert_eq!(second["horizons"]["10"]["mature"], 1);
    let row5 = row_of(&store, "snapshot-x", &cid_of("110051"), 5);
    assert_eq!(row5["baseline"]["date"], quotes[0]);
    assert_eq!(row5["end"]["date"], all_quotes[4]);
    assert_eq!(
        row_of(&store, "snapshot-x", &cid_of("110051"), 3),
        row3,
        "重试不改写已固定成熟结果"
    );
}

#[test]
fn replay_conflict_not_found_and_rollback() {
    let store = Store::open_in_memory().unwrap();
    let generated = friday_before_today(4);
    let quotes = weekday_after(crate::assets::days_from_civil(&generated).unwrap(), 3);
    fund(&store, "110061");
    nav(&store, "110061", &generated, 10000);
    for (index, date) in quotes.iter().enumerate() {
        nav(&store, "110061", date, 10100 + index as i64 * 100);
    }
    saved_batch(
        &store,
        "snapshot-x",
        &generated,
        json!([item("110061", Some("buy"))]),
    );
    let first = refresh_batch(&store, "snapshot-x", "same-1").unwrap();
    let replayed = refresh_batch(&store, "snapshot-x", "same-1").unwrap();
    assert_eq!(first, replayed, "同载荷请求重放返回同结果");
    assert!(matches!(
        refresh(
            &store,
            &json!({"requestId": "same-1", "snapshotId": "snapshot-other"})
        ),
        Err(FinanceError::Conflict(_))
    ));
    // 未知快照：显式失败，不写 ops、不写观察。
    assert!(matches!(
        refresh_batch(&store, "snapshot-unknown", "missing-1"),
        Err(FinanceError::NotFound(_))
    ));
    // 结构损坏的投影：完整事务回滚，无半批观察、无 remember。
    saved_batch(
        &store,
        "snapshot-broken",
        &generated,
        json!([{"securityId": "fund:110062"}]),
    );
    assert!(refresh_batch(&store, "snapshot-broken", "broken-1").is_err());
    let (ops, broken): (i64, i64) = store
        .with_read(|c| {
            Ok::<_, FinanceError>((
                c.query_row("SELECT COUNT(*) FROM opportunity_ops", [], |r| r.get(0))
                    .unwrap(),
                c.query_row(
                    "SELECT COUNT(*) FROM opportunity_observations WHERE snapshot_id='snapshot-broken'",
                    [],
                    |r| r.get(0),
                )
                .unwrap(),
            ))
        })
        .unwrap();
    assert_eq!(ops, 1, "仅成功路径 remember 一次");
    assert_eq!(broken, 0, "写失败回滚");
}

#[test]
fn coverage_is_null_without_samples_and_attach_serves_history() {
    let store = Store::open_in_memory().unwrap();
    let stats = crate::opportunity_records::stats(&store).unwrap();
    for horizon in ["3", "5", "10"] {
        assert_eq!(stats["observations"]["horizons"][horizon]["total"], 0);
        assert!(
            stats["observations"]["horizons"][horizon]["directions"]["up"]["hitRatePpm"].is_null()
        );
    }
    // 历史批次附带观察结果；缺失区间为 null。
    let generated = friday_before_today(4);
    let quotes = weekday_after(crate::assets::days_from_civil(&generated).unwrap(), 3);
    fund(&store, "110071");
    nav(&store, "110071", &generated, 10000);
    for (index, date) in quotes.iter().enumerate() {
        nav(&store, "110071", date, 10100 + index as i64 * 100);
    }
    saved_batch(
        &store,
        "snapshot-attach",
        &generated,
        json!([item("110071", Some("buy"))]),
    );
    refresh_batch(&store, "snapshot-attach", "attach-1").unwrap();
    let history = crate::opportunity_records::snapshots(&store, 1).unwrap();
    let row = &history["items"][0]["items"][0];
    assert_eq!(row["observation"]["horizons"]["3"]["status"], "mature");
    assert_eq!(row["observation"]["horizons"]["5"]["status"], "immature");
    assert_eq!(row["observation"]["horizons"]["10"]["status"], "immature");
    // 未执行观察的批次各区间均为 null。
    fund(&store, "110072");
    saved_batch(
        &store,
        "snapshot-unobserved",
        &generated,
        json!([item("110072", Some("watch"))]),
    );
    let history = crate::opportunity_records::snapshots(&store, 1).unwrap();
    let fresh = &history["items"][0]["items"][0];
    assert_eq!(fresh["observation"]["horizons"]["3"], json!(null));
    assert_eq!(fresh["observation"]["horizons"]["10"], json!(null));
}
