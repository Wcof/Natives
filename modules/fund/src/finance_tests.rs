//! finance 领域回归测试：方案 05 §4 验收算例（收益/现金/幂等/目标/日历）。

use super::*;
use crate::assets::ManualAssetInput;
use crate::finance;
use crate::goals::GoalInput;
use crate::storage::Store;

fn setup_store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO accounts(name, created_at, updated_at) VALUES('主账户','0','0')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn add_cash(store: &Store, amount_text: &str) {
    // 现金建账日期用真实今天：当日成交判等（trade_cash current 语义）依赖
    // 「估值日 = 交易日 = 今天」，写死日期会在系统日期前进后触发未对账分支。
    let today = finance::today_cn();
    let input = ManualAssetInput {
        account_name: "主账户".into(),
        product_type: "cash".into(),
        market: None,
        code: None,
        name: "现金".into(),
        risk_class: "cash".into(),
        market_value_text: amount_text.to_string(),
        valued_at: today.clone(),
        confirm_distinct: false,
    };
    crate::assets::create_asset(store, &input, &today).unwrap();
}

#[test]
fn cash_fact_invalidates_stale_manual_asset_edit() {
    let store = setup_store();
    add_cash(&store, "100.00");
    let (id, version): (i64, i64) = store
        .with_read(|c| {
            c.query_row(
                "SELECT id,version FROM manual_assets WHERE product_type='cash'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    event(
        &store,
        "external_in",
        "10.00",
        &today_cn(),
        Some("version-cash"),
    );
    let body=serde_json::json!({"id":id,"expectedVersion":version,"account":"主账户","productType":"cash","name":"现金","riskClass":"cash","marketValue":"100.00","valuedAt":today_cn()}).to_string();
    assert_eq!(
        crate::api::handle_api_request(
            &store,
            None,
            &app_runtime_core::http::HttpRequest::for_internal(
                "POST",
                "/api/assets/update",
                body.into_bytes()
            ),
            "/api/assets/update"
        )
        .unwrap_err()
        .0,
        409
    );
    let actual: (i64, i64) = store
        .with_read(|c| {
            c.query_row(
                "SELECT market_value_minor,version FROM manual_assets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(actual, (11000, version + 1));
}

#[test]
fn backdated_cash_event_keeps_current_valuation_date() {
    let store = setup_store();
    add_cash(&store, "100.00");
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute(
                "UPDATE manual_assets SET valued_at='2026-09-30' WHERE product_type='cash'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    event(
        &store,
        "external_in",
        "10.00",
        "2026-09-15",
        Some("backdated"),
    );
    let (amount, date): (i64, String) = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT market_value_minor, valued_at FROM manual_assets WHERE product_type='cash'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(amount, 11000);
    assert_eq!(date, "2026-09-30");
}

fn event(
    store: &Store,
    event_type: &str,
    amount_text: &str,
    occurred_at: &str,
    key: Option<&str>,
) -> i64 {
    event_to(store, event_type, amount_text, occurred_at, key, None)
}

fn event_to(
    store: &Store,
    event_type: &str,
    amount_text: &str,
    occurred_at: &str,
    key: Option<&str>,
    counterparty: Option<i64>,
) -> i64 {
    let input = CashEventInput {
        account_id: 1,
        event_type: event_type.into(),
        amount_text: amount_text.into(),
        occurred_at: occurred_at.into(),
        idempotency_key: key.map(str::to_string),
        note: String::new(),
        counterparty_account_id: counterparty,
    };
    store
        .with_write(|conn| record_cash_event(conn, &input))
        .unwrap()
}

/// 算例（05 §4.2）：期初 90 万、外部净投入 5 万、期末 100 万 → 净收益 5 万。
/// 期初与期末用估值快照表达；外部投入走 cash_events。
#[test]
fn return_formula_matches_acceptance_example() {
    let store = setup_store();
    add_cash(&store, "900000.00");
    let summary = crate::assets::summary(&store, "2026-09-01").unwrap();
    // 期初快照（2026-08-31）。
    finance::save_value_snapshot(&store, &summary, 900000_00, "期初建账", "2026-08-31").unwrap();
    // 期间外部转入 5 万（09-05）。
    event(
        &store,
        "external_in",
        "50000.00",
        "2026-09-05",
        Some("ext-1"),
    );
    // 期末资产涨到 100 万：现金补记估值（估值更新直接改现金资产为本例的简化路径）。
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            // 估值更新（02 §3.2）：市值与估值日期同事务更新，保持非过期口径。
            conn.execute(
                "UPDATE manual_assets SET market_value_minor = 100000000, valued_at='2026-09-30'
                 WHERE product_type='cash'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let end_summary = crate::assets::summary(&store, "2026-09-30").unwrap();
    finance::save_value_snapshot(&store, &end_summary, 100000_00, "期末", "2026-09-30").unwrap();

    let result = store
        .with_read(|conn| finance::returns_for_range(conn, "2026-08-31", "2026-09-30"))
        .unwrap();
    assert!(result.computable, "收益应可计算: {:?}", result.reason);
    assert_eq!(result.net_external_minor, Some(50000_00));
    assert_eq!(
        result.net_return_minor,
        Some(50000_00),
        "100万 − 90万 − 5万 = 5万"
    );
}

/// 内部转账组合口径为 0：同一 idempotency 两条事件（一减一增）不改变净外部投入。
#[test]
fn internal_transfer_neutral_to_net_flow() {
    let store = setup_store();
    add_cash(&store, "100000.00");
    let destination = store
        .with_write(|conn| -> rusqlite::Result<i64> {
            conn.execute(
                "INSERT INTO accounts(name, created_at, updated_at) VALUES('B账户','0','0')",
                [],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .unwrap();
    event(&store, "external_in", "100000.00", "2026-09-01", None);
    // A→B 内部转账：A 减（负额 correction 语义不适用）——首版内部转账记为
    // external 口径之外：A 账户 external_out 与 B 账户 external_in 属于错误记法，
    // 正确做法是 internal_transfer 单事件 + 两条账户侧记转（此处验证净投入公式
    // 只统计 external_*，internal_transfer 不计入）。
    event_to(
        &store,
        "internal_transfer",
        "20000.00",
        "2026-09-02",
        Some("it-1"),
        Some(destination),
    );
    let net = store
        .with_read(|conn| finance::net_external_flow(conn, "2026-09-01", "2026-09-30"))
        .unwrap();
    assert_eq!(net, 100000_00, "内部转账不影响外部净投入");
}

/// 现金算例（05 §4.2）：现金 40 万、预留 10 万、占用 5 万 → 可用 25 万。
#[test]
fn available_cash_matches_acceptance_example() {
    let store = setup_store();
    add_cash(&store, "400000.00");
    store
        .with_write(|conn| finance::save_reserve(conn, 1, Some("100000.00"), "备用金"))
        .unwrap();
    store
        .with_write(|conn| {
            finance::create_reservation(
                conn,
                &ReservationInput {
                    account_id: 1,
                    symbol: "sh600519".into(),
                    risk_class: "equity".into(),
                    amount_text: "50000.00".into(),
                    expected_date: "2026-09-15".into(),
                    note: String::new(),
                },
            )
        })
        .unwrap();
    let risk_class: String = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT risk_class FROM cash_reservations WHERE status='held'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(risk_class, "equity");
    let pos = store.with_read(finance::cash_position).unwrap();
    assert_eq!(pos.cash_minor, Some(400000_00));
    assert_eq!(pos.reserved_minor, Some(100000_00));
    assert_eq!(pos.reserved_by_orders_minor, 50000_00);
    assert_eq!(
        pos.available_minor,
        Some(250000_00),
        "40万 − 10万 − 5万 = 25万"
    );
    assert_eq!(pos.shortfall_minor, None);
}

/// 未配置预留 ≠ 0：reserve_state=unconfigured 时 reserved 为 None，可用不可计算。
#[test]
fn unconfigured_reserve_is_not_zero() {
    let store = setup_store();
    add_cash(&store, "100000.00");
    let pos = store.with_read(finance::cash_position).unwrap();
    assert_eq!(pos.reserve_state, "unconfigured");
    assert_eq!(pos.reserved_minor, None);
    assert_eq!(pos.available_minor, None, "预留未确认不生成可执行金额");
    // 明确保存 0 之后才可用。
    store
        .with_write(|conn| finance::save_reserve(conn, 1, Some("0.00"), ""))
        .unwrap();
    let pos = store.with_read(finance::cash_position).unwrap();
    assert_eq!(pos.reserve_state, "configured");
    assert_eq!(pos.available_minor, Some(100000_00));
}

/// 幂等：同 key 同内容返回同 ID；同 key 不同内容冲突。
#[test]
fn idempotency_key_semantics() {
    let store = setup_store();
    let id1 = event(&store, "external_in", "1000.00", "2026-09-01", Some("k1"));
    let id2 = event(&store, "external_in", "1000.00", "2026-09-01", Some("k1"));
    assert_eq!(id1, id2);
    let result = store.with_write(|conn| {
        record_cash_event(
            conn,
            &CashEventInput {
                account_id: 1,
                event_type: "external_in".into(),
                amount_text: "2000.00".into(),
                occurred_at: "2026-09-01".into(),
                idempotency_key: Some("k1".into()),
                note: String::new(),
                counterparty_account_id: None,
            },
        )
    });
    assert!(matches!(result, Err(FinanceError::Conflict(_))));
}

/// 转出超余额报错；占用 convert 只允许一次；释放后再 convert 冲突。
#[test]
fn balance_and_reservation_state_checks() {
    let store = setup_store();
    add_cash(&store, "1000.00");
    store
        .with_write(|c| finance::save_reserve(c, 1, Some("0"), ""))
        .unwrap();
    let result = store.with_write(|conn| {
        record_cash_event(
            conn,
            &CashEventInput {
                account_id: 1,
                event_type: "external_out".into(),
                amount_text: "2000.00".into(),
                occurred_at: "2026-09-01".into(),
                idempotency_key: None,
                note: String::new(),
                counterparty_account_id: None,
            },
        )
    });
    assert!(
        matches!(result, Err(FinanceError::Invalid(_))),
        "转出 2000 > 现金 1000 必须报错"
    );

    crate::assets::create_asset(
        &store,
        &ManualAssetInput {
            account_name: "主账户".into(),
            product_type: "stock".into(),
            market: Some("SSE".into()),
            code: Some("600519".into()),
            name: "茅台".into(),
            risk_class: "equity".into(),
            market_value_text: "0".into(),
            valued_at: "2026-09-30".into(),
            confirm_distinct: false,
        },
        "2026-09-30",
    )
    .unwrap();
    let rid = store
        .with_write(|conn| {
            finance::create_reservation(
                conn,
                &ReservationInput {
                    account_id: 1,
                    symbol: "sh600519".into(),
                    risk_class: "equity".into(),
                    amount_text: "300.00".into(),
                    expected_date: String::new(),
                    note: String::new(),
                },
            )
        })
        .unwrap();
    store
        .with_write(|conn| finance::settle_reservation(conn, rid, "convert"))
        .unwrap();
    let second = store.with_write(|conn| finance::settle_reservation(conn, rid, "convert"));
    assert!(
        matches!(second, Err(FinanceError::Conflict(_))),
        "convert 只允许一次"
    );
    // convert 已扣现金：1000 − 300 = 700。
    let pos = store.with_read(finance::cash_position).unwrap();
    assert_eq!(pos.cash_minor, Some(700_00));
}

/// 目标：创建即当前目标、版本冲突、唯一当前目标（新建/恢复双重约束）、归档/恢复。
#[test]
fn goal_crud_versions_and_exclusivity() {
    let store = setup_store();
    add_cash(&store, "500000.00");
    let today = "2026-09-30";
    let id = store
        .with_write(|conn| {
            crate::goals::create_goal(
                conn,
                &GoalInput {
                    name: "主目标".into(),
                    target_text: "1500000.00".into(),
                    target_date: "2031-09-30".into(),
                    scope_kind: "A".into(),
                    account_ids: vec![],
                    monthly_plan_text: None,
                    note: String::new(),
                },
                today,
            )
        })
        .unwrap();
    // 创建即当前目标（审查项 1）：is_primary=1，用户保存金额后无需再设主。
    let primaries: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM finance_goals WHERE is_primary=1 AND archived=0",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(primaries, 1, "新建目标必须自动成为当前目标");
    // 版本冲突。
    let conflict = store.with_write(|conn| {
        crate::goals::update_goal(
            conn,
            id,
            &GoalInput {
                name: "改名".into(),
                target_text: "1500000.00".into(),
                target_date: "2031-09-30".into(),
                scope_kind: "A".into(),
                account_ids: vec![],
                monthly_plan_text: None,
                note: String::new(),
            },
            99,
            today,
        )
    });
    assert!(matches!(conflict, Err(FinanceError::Conflict(_))));
    // 正常编辑 → version +1。
    let v = store
        .with_write(|conn| {
            crate::goals::update_goal(
                conn,
                id,
                &GoalInput {
                    name: "主目标v2".into(),
                    target_text: "1600000.00".into(),
                    target_date: "2031-09-30".into(),
                    scope_kind: "A".into(),
                    account_ids: vec![],
                    monthly_plan_text: Some("10000.00".into()),
                    note: String::new(),
                },
                1,
                today,
            )
        })
        .unwrap();
    assert_eq!(v, 2);
    // 唯一当前目标：存在活跃目标时新建拒绝（重复提交/双窗口同一约束）。
    let duplicate = store.with_write(|conn| {
        crate::goals::create_goal(
            conn,
            &GoalInput {
                name: "专项A".into(),
                target_text: "100000.00".into(),
                target_date: "2027-01-01".into(),
                scope_kind: "specific".into(),
                account_ids: vec![1],
                monthly_plan_text: None,
                note: String::new(),
            },
            today,
        )
    });
    assert!(matches!(duplicate, Err(FinanceError::Conflict(_))));
    // 归档当前目标后才能新建下一个目标；历史数据保留。
    store
        .with_write(|conn| crate::goals::archive_goal(conn, id))
        .unwrap();
    let g2 = store
        .with_write(|conn| {
            crate::goals::create_goal(
                conn,
                &GoalInput {
                    name: "专项A".into(),
                    target_text: "100000.00".into(),
                    target_date: "2027-01-01".into(),
                    scope_kind: "specific".into(),
                    account_ids: vec![1],
                    monthly_plan_text: None,
                    note: String::new(),
                },
                today,
            )
        })
        .unwrap();
    // 历史恢复同样受唯一当前目标约束。
    let restore_conflict = store.with_write(|conn| crate::goals::restore_goal(conn, id));
    assert!(matches!(restore_conflict, Err(FinanceError::Conflict(_))));
    // 归档 + 恢复；归档目标不可设主。
    store
        .with_write(|conn| crate::goals::archive_goal(conn, g2))
        .unwrap();
    let set_main = store.with_write(|conn| crate::goals::set_primary_goal(conn, g2));
    assert!(matches!(set_main, Err(FinanceError::Invalid(_))));
    store
        .with_write(|conn| crate::goals::restore_goal(conn, g2))
        .unwrap();
    // 恢复后即为当前目标（is_primary=1），且唯一。
    let primaries: i64 = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM finance_goals WHERE is_primary=1 AND archived=0",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(primaries, 1, "当前目标必须唯一");
}

/// 投入日历：月末枚举含闰年与截止日为月末；今天之后的月末才计入。
#[test]
fn month_end_calendar() {
    // 2026-09-30 → 2031-09-30：每月末（含 2028-02-29 闰月），共 60 个月末。
    let months = crate::goals::month_ends_until("2026-09-30", "2031-09-30");
    assert_eq!(months.len(), 60, "方案 02 §8 算例：剩余 60 次");
    assert_eq!(months[0], "2026-10-31");
    assert!(months.contains(&"2028-02-29".to_string()), "闰年二月末");
    assert_eq!(months.last().unwrap(), "2031-09-30", "截止日为月末时包含");
    // 短月与今天之后的第一个月末。
    let months2 = crate::goals::month_ends_until("2026-01-15", "2026-03-31");
    assert_eq!(
        months2,
        vec![
            "2026-01-31".to_string(),
            "2026-02-28".to_string(),
            "2026-03-31".to_string()
        ]
    );
    // 截止日在月中：最后一个月末若晚于截止日不计。
    let months3 = crate::goals::month_ends_until("2026-09-30", "2026-10-15");
    assert!(
        months3.is_empty(),
        "10 月末(10-31) > 截止日(10-15)，无剩余可执行月末"
    );
}

/// 缺基线收益不可计算（不伪造）。
#[test]
fn returns_without_baseline_is_incomplete() {
    let store = setup_store();
    add_cash(&store, "100000.00");
    let result = store
        .with_read(|conn| finance::returns_for_range(conn, "2026-09-01", "2026-09-30"))
        .unwrap();
    assert!(!result.computable);
    assert_eq!(result.reason, Some("missing_baseline".into()));
}

#[test]
fn completion_regression_unknown_account_and_missing_end() {
    let store = setup_store();
    add_cash(&store, "100.00");
    let cash = store.with_read(finance::cash_position).unwrap();
    assert_eq!(cash.accounts[0].available_minor, None);
    store.with_write(|c| -> rusqlite::Result<()> {
        c.execute("INSERT INTO value_snapshots(taken_at,total_minor,cash_minor,noncash_minor,quality,created_at) VALUES('2026-09-01',10000,10000,0,'ready','0')", [])?;
        Ok(())
    }).unwrap();
    let r = store
        .with_read(|c| finance::returns_for_range(c, "2026-09-01", "2026-09-30"))
        .unwrap();
    assert!(!r.computable, "期初快照不能同时冒充缺失期末");
    assert_eq!(r.reason.as_deref(), Some("missing_end"));
}

#[test]
fn completion_regression_specific_goal_creation_and_restore() {
    let store = setup_store();
    let input = GoalInput {
        name: "专项".into(),
        target_text: "1000".into(),
        target_date: "2031-09-30".into(),
        scope_kind: "specific".into(),
        account_ids: vec![1],
        monthly_plan_text: None,
        note: String::new(),
    };
    let id = store
        .with_write(|c| crate::goals::create_goal(c, &input, "2026-09-30"))
        .unwrap();
    assert!(store
        .with_write(|c| crate::goals::create_goal(c, &input, "2026-09-30"))
        .is_err());
    store
        .with_write(|c| crate::goals::archive_goal(c, id))
        .unwrap();
    store
        .with_write(|c| crate::goals::create_goal(c, &input, "2026-09-30"))
        .unwrap();
    assert!(store
        .with_write(|c| crate::goals::restore_goal(c, id))
        .is_err());
}

#[test]
fn completion_regression_income_trade_cash_and_atomicity() {
    let store = setup_store();
    add_cash(&store, "1000");
    store
        .with_write(|c| finance::save_reserve(c, 1, Some("0"), ""))
        .unwrap();
    event(&store, "dividend_cash", "10", "2026-09-30", Some("income"));
    event(&store, "fee", "2", "2026-09-30", Some("fee"));
    assert_eq!(
        store.with_read(finance::cash_position).unwrap().cash_minor,
        Some(100800)
    );
    let mut tx = crate::ledger::TransactionInput::manual(
        "主账户".into(),
        "000001".into(),
        "基金".into(),
        "BUY".into(),
        "confirmed".into(),
        crate::fixed::Fixed::parse("10", 4).unwrap(),
        crate::fixed::Fixed::parse("10", 4).unwrap(),
        crate::fixed::Fixed::parse("5", 2).unwrap(),
        finance::today_cn(),
        "tx-cash".into(),
    );
    crate::ledger::record_transaction(&store, &tx).unwrap();
    assert_eq!(
        store.with_read(finance::cash_position).unwrap().cash_minor,
        Some(90300)
    );
    crate::ledger::record_transaction(&store, &tx).unwrap();
    assert_eq!(
        store.with_read(finance::cash_position).unwrap().cash_minor,
        Some(90300)
    );
    tx.account_name = "别的账户".into();
    assert!(matches!(
        crate::ledger::record_transaction(&store, &tx),
        Err(crate::ledger::LedgerError::Conflict(_))
    ));
    tx.account_name = "主账户".into();
    tx.tx_type = "SELL".into();
    tx.request_id = Some("sell-cash".into());
    let (id, _) = crate::ledger::record_transaction(&store, &tx).unwrap();
    assert_eq!(
        store.with_read(finance::cash_position).unwrap().cash_minor,
        Some(90300),
        "未到账卖出不能增加可用现金"
    );
    store
        .with_write(|c| crate::trade_cash::settle(c, id, "post", &finance::today_cn(), "到账凭证"))
        .unwrap();
    assert_eq!(
        store.with_read(finance::cash_position).unwrap().cash_minor,
        Some(99800)
    );
    assert!(store
        .with_write(|c| crate::trade_cash::settle(c, id, "post", &finance::today_cn(), "重复"))
        .is_err());
    let before = store
        .with_read(|c| crate::storage::Store::data_revision(c))
        .unwrap();
    tx.tx_type = "BUY".into();
    tx.request_id = Some("too-big".into());
    tx.quantity = crate::fixed::Fixed::parse("100000", 4).unwrap();
    assert!(crate::ledger::record_transaction(&store, &tx).is_err());
    assert_eq!(
        store
            .with_read(|c| crate::storage::Store::data_revision(c))
            .unwrap(),
        before,
        "失败交易完整回滚版本与现金"
    );
}
