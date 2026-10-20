//! planning 模块测试（P2，方案 05 §4/§6 验收例题）。

use super::*;
use crate::assets::{self, ManualAssetInput};
use crate::ledger::{self, TransactionInput};
use crate::storage::Store;

fn configure_reserve_zero(store: &Store, names: &[&str]) {
    store
        .with_write(|conn| -> Result<(), crate::finance::FinanceError> {
            for name in names {
                let id: i64 =
                    conn.query_row("SELECT id FROM accounts WHERE name=?1", [name], |r| {
                        r.get(0)
                    })?;
                crate::finance::save_reserve(conn, id, Some("0.00"), "")?;
            }
            Ok(())
        })
        .unwrap();
}

fn ensure_accounts(store: &Store, names: &[&str]) {
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            for name in names {
                ledger::ensure_account(conn, name).map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                        Some(format!("{e:?}")),
                    )
                })?;
            }
            Ok(())
        })
        .unwrap();
}

fn buy(store: &Store, account: &str, code: &str, name: &str, qty: &str, price: &str, date: &str) {
    let input = TransactionInput::manual(
        account.to_string(),
        code.to_string(),
        name.to_string(),
        "BUY".to_string(),
        "confirmed".to_string(),
        Fixed::parse(qty, 4).unwrap(),
        Fixed::parse(price, 4).unwrap(),
        Fixed::zero(2),
        date.to_string(),
        format!("req-{account}-{code}-{date}-{qty}-{price}"),
    );
    ledger::record_transaction(store, &input).unwrap();
}

fn set_nav(store: &Store, code: &str, date: &str, nav_raw: i64) {
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO fund_nav(fund_id, nav_date, unit_nav_raw, source, fetched_at)
                 SELECT id, ?2, ?3, 'eastmoney-mobile', '1' FROM funds WHERE code=?1",
                params![code, date, nav_raw],
            )?;
            Ok(())
        })
        .unwrap();
}

fn manual(
    store: &Store,
    account: &str,
    pt: &str,
    code: Option<&str>,
    name: &str,
    value: &str,
    risk: &str,
    date: &str,
) {
    let input = ManualAssetInput {
        account_name: account.to_string(),
        product_type: pt.to_string(),
        market: code.map(|_| "SSE".to_string()),
        code: code.map(str::to_string),
        name: name.to_string(),
        risk_class: risk.to_string(),
        market_value_text: value.to_string(),
        valued_at: date.to_string(),
        confirm_distinct: false,
    };
    store
        .with_write(|conn| assets::create_manual_asset(conn, &input, "2026-09-29"))
        .unwrap();
}

fn default_targets() -> Vec<AllocationTargetRow> {
    TARGET_CLASSES
        .iter()
        .map(|c| AllocationTargetRow {
            risk_class: c.to_string(),
            target_tenths: 200,
            lower_tenths: 100,
            upper_tenths: 300,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 目标
// ---------------------------------------------------------------------------

#[test]
fn goal_target_must_be_positive_and_date_not_before_today() {
    let store = Store::open_in_memory().unwrap();
    // 零/负目标拒绝。
    let err = save_goal_tx(&store, "0.00", "2026-12-31", false, "2026-09-29").unwrap_err();
    assert_eq!(err.code(), "PLAN_INVALID");
    // 过去日期拒绝。
    let err = save_goal_tx(&store, "200.00", "2026-09-28", false, "2026-09-29").unwrap_err();
    assert_eq!(err.code(), "PLAN_INVALID");
    save_goal_tx(&store, "200.00", "2026-12-31", false, "2026-09-29").unwrap();
    let goal = store
        .with_read(|conn| get_goal(conn, "2026-09-29"))
        .unwrap()
        .unwrap();
    assert_eq!(goal.target_minor, 20000);
    assert!(!goal.expired);
}

#[test]
fn expired_goal_edit_amount_keeps_date() {
    let store = Store::open_in_memory().unwrap();
    save_goal_tx(&store, "200.00", "2026-09-01", false, "2026-08-01").unwrap();
    // 查询日 2026-09-29：已到期。
    let goal = store
        .with_read(|conn| get_goal(conn, "2026-09-29"))
        .unwrap()
        .unwrap();
    assert!(goal.expired);
    // 已到期只编辑金额、日期未变：保存成功且保留原日期与到期状态（方案 05 §6）。
    save_goal_tx(&store, "300.00", "", true, "2026-09-29").unwrap();
    let goal = store
        .with_read(|conn| get_goal(conn, "2026-09-29"))
        .unwrap()
        .unwrap();
    assert_eq!(goal.target_minor, 30000);
    assert_eq!(goal.target_date, "2026-09-01");
    assert!(goal.expired);
}

#[test]
fn goal_clear_removes_record_but_keeps_allocation() {
    let store = Store::open_in_memory().unwrap();
    save_goal_tx(&store, "200.00", "2026-12-31", false, "2026-09-29").unwrap();
    save_allocation_tx(&store, &default_targets()).unwrap();
    clear_goal_tx(&store).unwrap();
    let goal = store
        .with_read(|conn| get_goal(conn, "2026-09-29"))
        .unwrap();
    assert!(goal.is_none());
    let allocation = store.with_read(get_allocation).unwrap();
    assert_eq!(allocation.len(), 5);
}

#[test]
fn progress_examples_from_acceptance_table() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    // 例题：目标 ¥200，组合 ¥150（基金现值 ¥100 + 人工现金 ¥50），完整且新鲜 → 75.0%。
    buy(
        &store,
        "A",
        "110022",
        "消费行业",
        "100.0000",
        "1.0000",
        "2026-09-01",
    );
    set_nav(&store, "110022", "2026-09-28", 10000); // 1.0000
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))
        })
        .unwrap();
    store
        .with_write(|conn| assets::set_fund_metadata(conn, fund_id, "CNY", "equity"))
        .unwrap();
    manual(
        &store,
        "A",
        "cash",
        None,
        "现金",
        "50.00",
        "cash",
        "2026-09-28",
    );
    save_goal_tx(&store, "200.00", "2026-12-31", false, "2026-09-29").unwrap();
    let (_, progress) = goal_with_progress(&store, "2026-09-29").unwrap().unwrap();
    assert!(progress.computable);
    assert_eq!(progress.ratio_tenths, Some(750));
    // 已到期目标也仍为 75.0%，仅日期状态变化（方案 05 §6）。
    save_goal_tx(&store, "200.00", "", true, "2026-09-29").unwrap();
    store
        .with_write(|conn| {
            conn.execute(
                "UPDATE finance_goals SET target_date='2026-09-01'
                 WHERE id=(SELECT id FROM finance_goals WHERE archived=0
                           ORDER BY is_primary DESC, id LIMIT 1)",
                [],
            )
        })
        .unwrap();
    let (goal, progress) = goal_with_progress(&store, "2026-09-29").unwrap().unwrap();
    assert!(goal.expired);
    assert_eq!(progress.ratio_tenths, Some(750));
}

#[test]
fn progress_not_computable_when_empty_or_incomplete_or_stale() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    save_goal_tx(&store, "200.00", "2026-12-31", false, "2026-09-29").unwrap();
    // 空库：assets_empty。
    let (_, progress) = goal_with_progress(&store, "2026-09-29").unwrap().unwrap();
    assert!(!progress.computable);
    assert!(progress.reason_codes.contains(&"assets_empty".to_string()));
    // 有记录但缺净值：incomplete → 不可计算（不以部分值算完成度）。
    buy(
        &store,
        "A",
        "110022",
        "消费行业",
        "100.0000",
        "1.0000",
        "2026-09-01",
    );
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))
        })
        .unwrap();
    store
        .with_write(|conn| assets::set_fund_metadata(conn, fund_id, "CNY", "equity"))
        .unwrap();
    let (_, progress) = goal_with_progress(&store, "2026-09-29").unwrap().unwrap();
    assert!(!progress.computable);
    assert!(progress.reason_codes.contains(&"incomplete".to_string()));
    // 净值较旧（超过 7 日）：stale → 不可计算。
    set_nav(&store, "110022", "2026-09-01", 10000);
    let (_, progress) = goal_with_progress(&store, "2026-09-29").unwrap().unwrap();
    assert!(!progress.computable);
    assert!(progress.reason_codes.contains(&"stale".to_string()));
}

// ---------------------------------------------------------------------------
// 配置策略
// ---------------------------------------------------------------------------

#[test]
fn allocation_save_validation_matrix() {
    let store = Store::open_in_memory().unwrap();
    // 非 100% 总和拒绝。
    let mut rows = default_targets();
    rows[0].target_tenths = 250; // 总和 101.0%
    let err = save_allocation_tx(&store, &rows).unwrap_err();
    assert_eq!(err.code(), "PLAN_INVALID");
    // 上下限颠倒拒绝。
    let mut rows = default_targets();
    rows[0].lower_tenths = 300;
    rows[0].upper_tenths = 100;
    let err = save_allocation_tx(&store, &rows).unwrap_err();
    assert_eq!(err.code(), "PLAN_INVALID");
    // unknown 类别拒绝。
    let mut rows = default_targets();
    rows[0].risk_class = "unknown".into();
    let err = save_allocation_tx(&store, &rows).unwrap_err();
    assert_eq!(err.code(), "PLAN_INVALID");
    // 缺类别拒绝。
    let rows = default_targets()[..4].to_vec();
    let err = save_allocation_tx(&store, &rows).unwrap_err();
    assert_eq!(err.code(), "PLAN_INVALID");
    // 0/100 边界合法（总和恰为 100.0%）：保存成功。
    let mut rows = default_targets();
    for row in rows.iter_mut() {
        if row.risk_class == "equity" {
            row.target_tenths = 1000;
            row.lower_tenths = 0;
            row.upper_tenths = 1000;
        } else {
            row.target_tenths = 0;
            row.lower_tenths = 0;
            row.upper_tenths = 0;
        }
    }
    save_allocation_tx(&store, &rows).unwrap();
    assert_eq!(store.with_read(get_allocation).unwrap().len(), 5);
}

#[test]
fn allocation_save_is_atomic_on_failure() {
    let store = Store::open_in_memory().unwrap();
    let good = default_targets();
    save_allocation_tx(&store, &good).unwrap();
    // 非法输入失败后旧策略保持完整（单事务整体替换）。
    let mut bad = default_targets();
    bad[2].target_tenths = 500; // 总和 131.0%
    assert!(save_allocation_tx(&store, &bad).is_err());
    let rows = store.with_read(get_allocation).unwrap();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|r| r.target_tenths == 200));
}

// ---------------------------------------------------------------------------
// 配置检查
// ---------------------------------------------------------------------------

#[test]
fn check_example_equity_above_range_with_delta() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    // 例题：权益 ¥100、黄金 ¥50；目标权益 50%（40%~60%）、黄金 50%。
    buy(
        &store,
        "A",
        "110022",
        "消费行业",
        "100.0000",
        "1.0000",
        "2026-09-01",
    );
    set_nav(&store, "110022", "2026-09-28", 10000);
    let fund_id: i64 = store
        .with_read(|conn| {
            conn.query_row("SELECT id FROM funds WHERE code='110022'", [], |r| r.get(0))
        })
        .unwrap();
    store
        .with_write(|conn| assets::set_fund_metadata(conn, fund_id, "CNY", "equity"))
        .unwrap();
    manual(
        &store,
        "A",
        "gold",
        None,
        "金条",
        "50.00",
        "gold",
        "2026-09-28",
    );
    let mut rows = default_targets();
    // equity: 50% (40~60)；gold: 50% (40~60)；其余 0%（0~0）。
    for row in rows.iter_mut() {
        match row.risk_class.as_str() {
            "equity" | "gold" => {
                row.target_tenths = 500;
                row.lower_tenths = 400;
                row.upper_tenths = 600;
            }
            _ => {
                row.target_tenths = 0;
                row.lower_tenths = 0;
                row.upper_tenths = 0;
            }
        }
    }
    save_allocation_tx(&store, &rows).unwrap();
    configure_reserve_zero(&store, &["A"]);
    let check = allocation_check(&store, "2026-09-29").unwrap();
    assert!(check.computable);
    assert_eq!(check.total_minor, Some(15000));
    let equity = check
        .classes
        .iter()
        .find(|c| c.risk_class == "equity")
        .unwrap();
    // 实际 100/150 = 66.7%（0.1% 定点四舍五入）。
    assert_eq!(equity.actual_tenths, Some(667));
    assert_eq!(equity.status, Some("above"));
    // 偏差 ≈ +16.7 个百分点。
    assert_eq!(equity.delta_tenths, Some(167));
    assert_eq!(check.out_of_range.len(), 2);
    assert!(check
        .out_of_range
        .iter()
        .any(|o| o.risk_class == "gold" && o.direction == "below"));
}

#[test]
fn check_boundary_rounding_not_masked() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    // 实际 59.96%、下限 60.0%：必须判低于区间（不能四舍五入成 60.0% 变合格）。
    // 组合 ¥100.00：现金 59.96、黄金 40.04；现金下限 60.0%。
    manual(
        &store,
        "A",
        "cash",
        None,
        "现金",
        "59.96",
        "cash",
        "2026-09-28",
    );
    manual(
        &store,
        "A",
        "gold",
        None,
        "金条",
        "40.04",
        "gold",
        "2026-09-28",
    );
    let mut rows = default_targets();
    for row in rows.iter_mut() {
        match row.risk_class.as_str() {
            "cash" => {
                row.target_tenths = 600;
                row.lower_tenths = 600;
                row.upper_tenths = 1000;
            }
            "gold" => {
                row.target_tenths = 400;
                row.lower_tenths = 0;
                row.upper_tenths = 400;
            }
            _ => {
                row.target_tenths = 0;
                row.lower_tenths = 0;
                row.upper_tenths = 0;
            }
        }
    }
    save_allocation_tx(&store, &rows).unwrap();
    configure_reserve_zero(&store, &["A"]);
    let check = allocation_check(&store, "2026-09-29").unwrap();
    assert!(check.computable);
    let cash = check
        .classes
        .iter()
        .find(|c| c.risk_class == "cash")
        .unwrap();
    assert_eq!(cash.actual_tenths, Some(600), "展示值四舍五入为 60.0%");
    assert_eq!(cash.status, Some("below"), "真实值 59.96% 低于 60.0% 下限");
    let gold = check
        .classes
        .iter()
        .find(|c| c.risk_class == "gold")
        .unwrap();
    // 黄金 40.04% 上限 40.0%：高于区间（端点在区间内，40.04 > 40.0）。
    assert_eq!(gold.status, Some("above"));
}

#[test]
fn check_endpoints_count_as_within() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    // 恰好落在边界：40.0% 对下限 40.0% → within。
    manual(
        &store,
        "A",
        "cash",
        None,
        "现金",
        "60.00",
        "cash",
        "2026-09-28",
    );
    manual(
        &store,
        "A",
        "gold",
        None,
        "金条",
        "40.00",
        "gold",
        "2026-09-28",
    );
    let mut rows = default_targets();
    for row in rows.iter_mut() {
        match row.risk_class.as_str() {
            "cash" => {
                row.target_tenths = 600;
                row.lower_tenths = 400;
                row.upper_tenths = 600;
            }
            "gold" => {
                row.target_tenths = 400;
                row.lower_tenths = 0;
                row.upper_tenths = 400;
            }
            _ => {
                row.target_tenths = 0;
                row.lower_tenths = 0;
                row.upper_tenths = 0;
            }
        }
    }
    save_allocation_tx(&store, &rows).unwrap();
    configure_reserve_zero(&store, &["A"]);
    let check = allocation_check(&store, "2026-09-29").unwrap();
    let cash = check
        .classes
        .iter()
        .find(|c| c.risk_class == "cash")
        .unwrap();
    assert_eq!(cash.status, Some("within"));
    assert!(check.out_of_range.is_empty());
}

#[test]
fn check_not_computable_reasons() {
    let store = Store::open_in_memory().unwrap();
    ensure_accounts(&store, &["A"]);
    save_allocation_tx(&store, &default_targets()).unwrap();
    configure_reserve_zero(&store, &["A"]);
    // 空组合：empty_portfolio，不是「正常」。
    let check = allocation_check(&store, "2026-09-29").unwrap();
    assert!(!check.computable);
    assert!(check.reason_codes.contains(&"empty_portfolio".to_string()));
    // 真实零值组合（¥0 现金）：zero_total，不是「正常」。
    manual(
        &store,
        "A",
        "cash",
        None,
        "现金",
        "0.00",
        "cash",
        "2026-09-28",
    );
    let check = allocation_check(&store, "2026-09-29").unwrap();
    assert!(!check.computable);
    assert!(check.reason_codes.contains(&"zero_total".to_string()));
    // 分类 unknown：unknown_class。
    manual(
        &store,
        "A",
        "gold",
        None,
        "金条",
        "50.00",
        "unknown",
        "2026-09-28",
    );
    let check = allocation_check(&store, "2026-09-29").unwrap();
    assert!(!check.computable);
    assert!(check.reason_codes.contains(&"unknown_class".to_string()));
    assert_eq!(check.unknown_class_count, 1);
}
