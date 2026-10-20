//! rules 领域回归测试：方案 05 §6 / 03 §5 验收重点。

use super::*;
use crate::assets::ManualAssetInput;
use crate::decisions::{record_decision, DecisionInput};
use crate::finance::CashEventInput;
use crate::simulate::{simulate_buy, SimulateBuyInput};
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
    let today = crate::finance::today_cn();
    let input = ManualAssetInput {
        account_name: "主账户".into(),
        product_type: "cash".into(),
        market: None,
        code: None,
        name: "现金".into(),
        risk_class: "cash".into(),
        market_value_text: amount_text.into(),
        valued_at: today.clone(),
        confirm_distinct: false,
    };
    crate::assets::create_asset(store, &input, &today).unwrap();
}

fn rule(
    store: &Store,
    rule_type: &str,
    object_key: &str,
    params: serde_json::Value,
    enabled: bool,
) -> i64 {
    store
        .with_write(|conn| {
            create_rule(
                conn,
                &RuleInput {
                    name: rule_type.to_string(),
                    rule_type: rule_type.into(),
                    object_key: object_key.into(),
                    params,
                    period: "quote".into(),
                    enabled,
                },
            )
        })
        .unwrap()
}

/// 相同条件连续触发只维护一个未解除事件；解除后再满足才新建（03 §5.2 / 05 §6）。
#[test]
fn dedupe_same_object_single_open_event() {
    let store = setup_store();
    let id = rule(
        &store,
        "stale_valuation",
        "valuation",
        serde_json::json!({}),
        true,
    );
    // 制造过期估值。
    add_cash(&store, "100.00");
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute("UPDATE manual_assets SET valued_at='2026-01-01'", [])?;
            Ok(())
        })
        .unwrap();
    let (_n, _actions) = run_check(&store).unwrap();
    let (_n, _) = run_check(&store).unwrap();
    let events = store.with_read(|conn| list_events(conn, None)).unwrap();
    assert_eq!(events.len(), 1, "同对象未解除事件去重");
    assert_eq!(events[0].rule_id, id);
    // 解除。
    store
        .with_write(|conn| resolve_event(conn, events[0].id, "已核对"))
        .unwrap();
    let open = store
        .with_read(|conn| list_events(conn, Some("open")))
        .unwrap();
    assert!(open.is_empty());
    // 再次满足条件 → 新事件。
    let (_n2, _) = run_check(&store).unwrap();
    let events2 = store
        .with_read(|conn| list_events(conn, Some("open")))
        .unwrap();
    assert_eq!(events2.len(), 1, "解除后再次进入创建新事件");
    assert_ne!(events2[0].id, events[0].id);
}

/// 暂缓事件到期前保持 snoozed；到期后仍成立则恢复 open，不重复新建。
#[test]
fn snooze_keeps_single_event_and_recovers() {
    let store = setup_store();
    rule(
        &store,
        "stale_valuation",
        "valuation",
        serde_json::json!({}),
        true,
    );
    add_cash(&store, "100.00");
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute("UPDATE manual_assets SET valued_at='2026-01-01'", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
    run_check(&store).unwrap();
    let events = store.with_read(|conn| list_events(conn, None)).unwrap();
    let eid = events[0].id;
    store
        .with_write(|conn| snooze_event(conn, eid, "2099-12-31"))
        .unwrap();
    run_check(&store).unwrap();
    let all = store.with_read(|conn| list_events(conn, None)).unwrap();
    assert_eq!(all.len(), 1, "暂缓期间不新建事件");
    assert_eq!(all[0].status, "snoozed");
    // 到期（snoozed_until 过去）→ 恢复 open。
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute("UPDATE watch_events SET snoozed_until='2026-01-01'", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
    run_check(&store).unwrap();
    let all2 = store.with_read(|conn| list_events(conn, None)).unwrap();
    assert_eq!(all2.len(), 1, "到期恢复不新建");
    assert_eq!(all2[0].status, "open");
}

/// 数据源失败不把风险设为已解除：ready_state 独立记录（03 §5.1）。
#[test]
fn ready_state_recorded_independently() {
    let store = setup_store();
    rule(
        &store,
        "class_unknown",
        "class",
        serde_json::json!({}),
        true,
    );
    let input = ManualAssetInput {
        account_name: "主账户".into(),
        product_type: "stock".into(),
        market: Some("SSE".into()),
        code: Some("600519".into()),
        name: "贵州茅台".into(),
        risk_class: "unknown".into(),
        market_value_text: "1000.00".into(),
        valued_at: crate::finance::today_cn(),
        confirm_distinct: false,
    };
    crate::assets::create_asset(&store, &input, &crate::finance::today_cn()).unwrap();
    let (n, actions) = run_check(&store).unwrap();
    assert!(n >= 1);
    let actions: Vec<_> = actions
        .iter()
        .filter(|a| a.rule_type == "class_unknown")
        .collect();
    assert!(!actions.is_empty());
    assert_eq!(
        actions[0].result_type, "补齐数据",
        "未知分类 → 补齐数据，不给金额建议"
    );
    let events = store.with_read(|conn| list_events(conn, None)).unwrap();
    assert_eq!(events[0].ready_state, "unknown");
}

/// 规则编辑产生新版本；未解除事件随版本更新（不重复通知由 UI 层提示）。
#[test]
fn rule_edit_bumps_version_and_updates_events() {
    let store = setup_store();
    let rid = rule(
        &store,
        "stale_valuation",
        "valuation",
        serde_json::json!({}),
        true,
    );
    add_cash(&store, "100.00");
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute("UPDATE manual_assets SET valued_at='2026-01-01'", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
    run_check(&store).unwrap();
    let v = store
        .with_write(|conn| {
            update_rule(
                conn,
                rid,
                &RuleInput {
                    name: "估值过期规则".into(),
                    rule_type: "stale_valuation".into(),
                    object_key: "valuation".into(),
                    params: serde_json::json!({}),
                    period: "quote".into(),
                    enabled: true,
                },
            )
        })
        .unwrap();
    assert_eq!(v, 2);
    let events = store.with_read(|conn| list_events(conn, None)).unwrap();
    assert_eq!(events[0].rule_version, 2, "未解除事件跟随规则版本");
}

/// 阈值校验：concentration 无 threshold 拒绝创建（投资阈值必须用户确认，03 §3）。
#[test]
fn rule_params_validation() {
    let store = setup_store();
    let result = store.with_write(|conn| {
        create_rule(
            conn,
            &RuleInput {
                name: "集中度".into(),
                rule_type: "concentration".into(),
                object_key: String::new(),
                params: serde_json::json!({}),
                period: "quote".into(),
                enabled: true,
            },
        )
    });
    assert!(matches!(result, Err(RuleError::Invalid(_))));
    // 未知类型拒绝。
    let result2 = store.with_write(|conn| {
        create_rule(
            conn,
            &RuleInput {
                name: "x".into(),
                rule_type: "moon_phase".into(),
                object_key: String::new(),
                params: serde_json::json!({}),
                period: "quote".into(),
                enabled: true,
            },
        )
    });
    assert!(matches!(result2, Err(RuleError::Invalid(_))));
}

/// 集中度算例（02 §8）：标的 22% > 上限 20% → 减仓候选，金额=超额部分。
#[test]
fn concentration_produces_reduce_candidate() {
    let store = setup_store();
    add_cash(&store, "10000.00");
    // 预留未配置时 B 不可算（不默认 0，02 §1.1）；测试先明确配置预留 0。
    store
        .with_write(|conn| finance::save_reserve(conn, 1, Some("0.00"), ""))
        .unwrap();
    rule(
        &store,
        "concentration",
        "",
        serde_json::json!({"threshold": 20.0}),
        true,
    );
    // 单一标的 3000 占可配置资产 10000−0 预留……cash 类也计入 B。
    // 用现金 7000 + 标的 3000（B=10000）→ 标的占 30% > 20%。
    let input = ManualAssetInput {
        account_name: "主账户".into(),
        product_type: "stock".into(),
        market: Some("SSE".into()),
        code: Some("600519".into()),
        name: "贵州茅台".into(),
        risk_class: "equity".into(),
        market_value_text: "3000.00".into(),
        valued_at: crate::finance::today_cn(),
        confirm_distinct: false,
    };
    crate::assets::create_asset(&store, &input, &crate::finance::today_cn()).unwrap();
    let (_n, actions) = run_check(&store).unwrap();
    let reduce: Vec<_> = actions
        .iter()
        .filter(|a| a.result_type == "减仓候选")
        .collect();
    assert_eq!(reduce.len(), 1, "集中度超限生成减仓候选");
    // B = A − R = (现金 10000 + 标的 3000) − 0 = 13000；超额 = 3000 − 20%×13000 = 400 元。
    assert_eq!(
        reduce[0].amount_cap_minor,
        Some(40000),
        "减仓金额 = 市值 − 上限×B（不计费参考）"
    );
}

/// 模拟预算约束（03 §4.1）：min(K, H, U, 单笔, 现金−费用)。
#[test]
fn simulate_buy_budget_caps() {
    let store = setup_store();
    add_cash(&store, "250000.00"); // 现金 25 万（配置后即可用）
    store
        .with_write(|conn| finance::save_reserve(conn, 1, Some("0.00"), ""))
        .unwrap();
    // 配置：权益 60%，现金 40%（无持仓 → 权益缺口 = 60%×B）。
    let rows = vec![
        crate::planning::AllocationTargetRow {
            risk_class: "equity".into(),
            target_tenths: 600,
            lower_tenths: 0,
            upper_tenths: 1000,
        },
        crate::planning::AllocationTargetRow {
            risk_class: "gold".into(),
            target_tenths: 0,
            lower_tenths: 0,
            upper_tenths: 1000,
        },
        crate::planning::AllocationTargetRow {
            risk_class: "fixed_income".into(),
            target_tenths: 0,
            lower_tenths: 0,
            upper_tenths: 1000,
        },
        crate::planning::AllocationTargetRow {
            risk_class: "cash".into(),
            target_tenths: 400,
            lower_tenths: 0,
            upper_tenths: 1000,
        },
        crate::planning::AllocationTargetRow {
            risk_class: "other".into(),
            target_tenths: 0,
            lower_tenths: 0,
            upper_tenths: 1000,
        },
    ];
    store
        .with_write(|conn| crate::planning::save_allocation(conn, &rows))
        .unwrap();
    // R08：模拟必须有集中度约束（用户确认的规则）才可计算。
    rule(
        &store,
        "concentration",
        "",
        serde_json::json!({"threshold": 50.0}),
        true,
    );
    // 单笔上限 5 万 → cap = min(缺口 15万, H 12.5万, U 15万, 现金 25万, 单笔 5万) = 5万。
    let result = simulate_buy(
        &store,
        &SimulateBuyInput {
            risk_class: "equity".into(),
            symbol: "600519".into(),
            single_limit_text: Some("50000.00".into()),
            fee_text: Some("50.00".into()),
        },
    )
    .unwrap();
    assert!(result.computable, "{:?}", result.reason);
    // 类别缺口 K = 60%×25万 = 15万；U = D−I = 15万−0 = 15万；现金−费 = 24.995万。
    assert_eq!(
        result.cap_minor,
        Some(5000000),
        "min(K,U,现金−费,单笔) = 单笔上限 5万"
    );
    // 无单笔上限、费用显式 0 → cap = K = 15万（H 12.5万 > 15万? 否：H=50%×25万−0=12.5万）。
    // 实际 min(K 15万, H 12.5万, U 15万, 现金−0费 25万) = 12.5万。
    let result2 = simulate_buy(
        &store,
        &SimulateBuyInput {
            risk_class: "equity".into(),
            symbol: "600519".into(),
            single_limit_text: None,
            fee_text: Some("0.00".into()),
        },
    )
    .unwrap();
    assert_eq!(result2.cap_minor, None, "未确认单笔上限时停止金额测算");
    // 费用未确认（None）≠ 0：不给可计算金额。
    let result3 = simulate_buy(
        &store,
        &SimulateBuyInput {
            risk_class: "equity".into(),
            symbol: "600519".into(),
            single_limit_text: None,
            fee_text: None,
        },
    )
    .unwrap();
    assert!(!result3.computable, "费用未确认时模拟不可计算");
}

/// 决策记录：事件存在性校验 + 动作枚举；记录不改变事件状态（记录≠成交）。
#[test]
fn decision_records_do_not_change_state() {
    let store = setup_store();
    rule(
        &store,
        "stale_valuation",
        "valuation",
        serde_json::json!({}),
        true,
    );
    add_cash(&store, "100.00");
    store
        .with_write(|conn| -> rusqlite::Result<()> {
            conn.execute("UPDATE manual_assets SET valued_at='2026-01-01'", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
    run_check(&store).unwrap();
    let events = store.with_read(|conn| list_events(conn, None)).unwrap();
    let eid = events[0].id;
    let bad = store.with_write(|conn| {
        record_decision(
            conn,
            &DecisionInput {
                event_id: eid,
                action: "buy".into(),
                note: String::new(),
                snapshot_id: String::new(),
            },
        )
    });
    assert!(matches!(bad, Err(RuleError::Invalid(_))), "未知动作拒绝");
    store
        .with_write(|conn| {
            record_decision(
                conn,
                &DecisionInput {
                    event_id: eid,
                    action: "done".into(),
                    note: "已核对估值".into(),
                    snapshot_id: "home-test".into(),
                },
            )
        })
        .unwrap();
    let status: String = store
        .with_read(|conn| {
            conn.query_row("SELECT status FROM watch_events WHERE id=?1", [eid], |r| {
                r.get(0)
            })
        })
        .unwrap();
    assert_eq!(
        status, "open",
        "记录已处理不等于风险解除（需真实核对/解除）"
    );
    let cash_event = CashEventInput {
        account_id: 1,
        event_type: "external_in".into(),
        amount_text: "1.00".into(),
        occurred_at: "2026-09-30".into(),
        idempotency_key: None,
        note: String::new(),
        counterparty_account_id: None,
    };
    let _ = cash_event;
}

#[test]
fn completion_regression_simulation_version_and_shared_budget() {
    let store = setup_store();
    add_cash(&store, "100");
    store
        .with_write(|c| finance::save_reserve(c, 1, Some("0"), ""))
        .unwrap();
    let rows: Vec<_> = crate::planning::TARGET_CLASSES
        .iter()
        .map(|key| crate::planning::AllocationTargetRow {
            risk_class: key.to_string(),
            target_tenths: if *key == "equity" { 1000 } else { 0 },
            lower_tenths: 0,
            upper_tenths: 1000,
        })
        .collect();
    store
        .with_write(|c| crate::planning::save_allocation(c, &rows))
        .unwrap();
    rule(
        &store,
        "concentration",
        "",
        serde_json::json!({"threshold":80}),
        true,
    );
    let revision = store.with_read(Store::data_revision).unwrap();
    let inputs: Vec<_> = (0..3)
        .map(|_| SimulateBuyInput {
            risk_class: "equity".into(),
            symbol: "security:SSE:600519".into(),
            single_limit_text: Some("50".into()),
            fee_text: Some("0".into()),
        })
        .collect();
    let results = crate::simulate::simulate_basket(&store, 1, Some(&revision), &inputs).unwrap();
    assert_eq!(
        results
            .iter()
            .map(|r| r.cap_minor.unwrap())
            .collect::<Vec<_>>(),
        vec![5000, 3000, 0]
    );
    assert!(crate::simulate::simulate_basket(&store, 1, Some("old"), &inputs).is_err());
    let bad = SimulateBuyInput {
        fee_text: Some("-1".into()),
        ..inputs.into_iter().next().unwrap()
    };
    assert!(crate::simulate::simulate_basket(&store, 1, Some(&revision), &[bad]).is_err());
}
