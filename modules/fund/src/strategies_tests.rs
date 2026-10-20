//! 策略目录域测试（R2 · A33/A36）：目录同步幂等、更新校验与拒绝、乐观锁、
//! 权重进入生产投影排序、停用排除、恢复默认与 v21→v22 迁移。

use super::*;
use crate::opportunities::projection;
use crate::storage::Store;

fn update_ok(
    store: &Store,
    code: &str,
    fields: Value,
    revision: i64,
) -> Result<Value, FinanceError> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let request_id = format!(
        "strat-{}-{}",
        crate::finance::now_secs(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let mut body = json!({"requestId": request_id, "code": code, "expectedRevision": revision});
    for (k, v) in fields.as_object().expect("fields object") {
        body[k.clone()] = v.clone();
    }
    update(store, &body)
}

#[test]
fn catalog_syncs_defaults_idempotently_and_lists_effective_weights() {
    let store = Store::open_in_memory().unwrap();
    let listed = list(&store).unwrap();
    assert_eq!(
        listed["items"].as_array().unwrap().len(),
        DEFAULT_STRATEGIES.len()
    );
    let codes: Vec<&str> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["code"].as_str().unwrap())
        .collect();
    for (code, ..) in DEFAULT_STRATEGIES {
        assert!(codes.contains(code), "default {code} synced");
    }
    // 幂等：重复 list 不新增行、revision 不变。
    let again = list(&store).unwrap();
    assert_eq!(again, listed);
    // 生效权重表与清单一致（默认权重、未覆盖）。
    let weights = store.with_snapshot(effective_weights).unwrap();
    assert_eq!(weights["revision"], listed["revision"]);
    let trend = weights["weights"]["trend_follow"].clone();
    assert_eq!(trend["weightPpm"], 1_150_000);
    assert_eq!(trend["overridden"], false);
    assert_eq!(trend["enabled"], true);
}

#[test]
fn update_rejects_invalid_values_instead_of_clamping_and_conflicts_on_stale_revision() {
    let store = Store::open_in_memory().unwrap();
    let listed = list(&store).unwrap();
    let revision = listed["revision"].as_i64().unwrap();
    // 非法权重：超上界、低于下界、非整数——显式拒绝，不是 clamp。
    for bad in [0, 99_999, 3_000_001, 12_345_678] {
        assert!(
            matches!(
                update_ok(&store, "trend_follow", json!({"weightPpm": bad}), revision),
                Err(FinanceError::Invalid(_))
            ),
            "weight {bad} must be rejected"
        );
    }
    assert!(matches!(
        update_ok(&store, "trend_follow", json!({"weightPpm": 1.5}), revision),
        Err(FinanceError::Invalid(_))
    ));
    assert!(matches!(
        update_ok(&store, "trend_follow", json!({"enabled": "yes"}), revision),
        Err(FinanceError::Invalid(_))
    ));
    assert!(matches!(
        update_ok(
            &store,
            "no_such_strategy",
            json!({"enabled": true}),
            revision
        ),
        Err(FinanceError::NotFound(_))
    ));
    // 无变更 / restore 混合变更同样拒绝。
    assert!(matches!(
        update_ok(&store, "trend_follow", json!({}), revision),
        Err(FinanceError::Invalid(_))
    ));
    assert!(matches!(
        update_ok(
            &store,
            "trend_follow",
            json!({"restore": true, "weightPpm": 1_500_000}),
            revision
        ),
        Err(FinanceError::Invalid(_))
    ));
    // 目录未被上述失败改动：revision 不变、无 ops 记录。
    assert_eq!(list(&store).unwrap()["revision"], revision);
    // 陈旧乐观锁 → 409。
    let _ = update_ok(
        &store,
        "trend_follow",
        json!({"weightPpm": 1_500_000}),
        revision,
    )
    .unwrap();
    assert!(matches!(
        update_ok(&store, "rebound", json!({"weightPpm": 1_200_000}), revision),
        Err(FinanceError::Conflict(_))
    ));
}

#[test]
fn weights_and_enabled_changes_actually_move_production_candidates() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_write(|c| {
            c.execute(
                "INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,created_at) VALUES(1,'110022','隔离','fund',CURRENT_TIMESTAMP)",
                [],
            )?;
            c.execute("INSERT INTO funds(id,code,name,updated_at) VALUES(1,'110022','隔离基金','0')", [])
        })
        .unwrap();
    let today = crate::finance::today_cn();
    // 缓跌夹具：末段单日 -6% 触发 rebound 标签，基础分低（avoid 11），权重差不被封顶掩盖。
    let mut raw = 20000i64;
    let mut rows = Vec::new();
    for i in 1..=60 {
        raw = if i <= 54 {
            20000 - i * 50
        } else {
            raw * 94 / 100
        };
        rows.push(raw);
    }
    for (i, value) in rows.iter().enumerate() {
        let date = crate::assets::civil_from_days(
            crate::assets::days_from_civil(&today).unwrap() - 61 + i as i64 + 1,
        );
        store
            .with_write(|c| {
                c.execute(
                    "INSERT INTO fund_nav(fund_id,nav_date,unit_nav_raw,source,fetched_at) VALUES(1,?1,?2,'isolated','0')",
                    rusqlite::params![date, value],
                )
            })
            .unwrap();
    }
    let project = |store: &Store| {
        store
            .with_read(|c| {
                projection(
                    c,
                    &crate::assets::AssetsSummary::default(),
                    &today,
                    Some(1000),
                )
            })
            .unwrap()
    };
    let before = project(&store);
    let fund = before["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "fund")
        .unwrap()
        .clone();
    let base_score = fund["score"].as_i64().unwrap();
    let tags = fund["strategyTags"].as_array().unwrap();
    assert_eq!(
        tags,
        &json!(["rebound"]).as_array().unwrap().clone(),
        "declining fixture must match rebound only, got {tags:?}"
    );
    assert_eq!(fund["signal"], "avoid");
    // 默认 rebound 0.95x：rank = (base × 950_000) / 1_000_000。
    assert_eq!(
        fund["rankScore"].as_i64().unwrap(),
        base_score * 950_000 / 1_000_000,
        "default rebound weight 0.95x must shape the production rank"
    );
    let before_id = before["snapshotId"].clone();
    // 权重 2.0x → rank 变化且快照身份变化（参数进入生产计算，旧快照保留旧版本）。
    let listed = list(&store).unwrap();
    let revision = listed["revision"].as_i64().unwrap();
    let _ = update_ok(&store, "rebound", json!({"weightPpm": 2_000_000}), revision).unwrap();
    let after = project(&store);
    let fund_after = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "fund")
        .unwrap();
    assert_eq!(
        fund_after["rankScore"].as_i64().unwrap(),
        base_score * 2,
        "2.0x weight must move the production rank"
    );
    assert_ne!(
        after["snapshotId"], before_id,
        "config revision changes candidate identity"
    );
    let matched = fund_after["strategyWeights"].as_array().unwrap();
    assert!(matched
        .iter()
        .any(|m| m["code"] == "rebound" && m["weightPpm"] == 2_000_000));
    // 停用 rebound：匹配排除，rank 回到 1.0x 基础分。
    let listed = list(&store).unwrap();
    let _ = update_ok(
        &store,
        "rebound",
        json!({"enabled": false}),
        listed["revision"].as_i64().unwrap(),
    )
    .unwrap();
    let disabled = project(&store);
    let fund_disabled = disabled["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "fund")
        .unwrap();
    assert_eq!(fund_disabled["rankScore"].as_i64().unwrap(), base_score);
    assert!(fund_disabled["strategyWeights"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["code"] != "rebound"));
    // 恢复默认并重新启用：覆盖被删除，rank 回到默认 0.95x。
    let listed = list(&store).unwrap();
    let _ = update_ok(
        &store,
        "rebound",
        json!({"restore": true, "enabled": true}),
        listed["revision"].as_i64().unwrap(),
    )
    .unwrap_or_else(|e| panic!("restore+enable rejected: {}", e.message()));
    let restored = project(&store);
    let fund_restored = restored["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "fund")
        .unwrap();
    assert_eq!(
        fund_restored["rankScore"].as_i64().unwrap(),
        base_score * 950_000 / 1_000_000
    );
    let weights = store.with_snapshot(effective_weights).unwrap();
    assert_eq!(weights["weights"]["rebound"]["overridden"], false);
    // 重放幂等：同载荷返回同结果、不再次推进 revision。
    let listed = list(&store).unwrap();
    let revision = listed["revision"].as_i64().unwrap();
    let replay_body = json!({"requestId": "strat-replay-1", "code": "trend_follow", "weightPpm": 1_400_000, "expectedRevision": revision});
    let first = update(&store, &replay_body).unwrap();
    let replay = update(&store, &replay_body).unwrap();
    assert_eq!(first, replay);
    assert_eq!(list(&store).unwrap()["revision"], first["revision"]);
}

#[test]
fn weighted_rank_is_deterministic_and_bounded() {
    let weights = json!({"weights": {
        "trend_follow": {"weightPpm": 3_000_000, "enabled": true},
        "volume_breakout": {"weightPpm": 1_180_000, "enabled": true},
        "rebound": {"weightPpm": 950_000, "enabled": false}
    }});
    // 取启用集合的最高权重；停用不参与；基础分 100 封顶 100。
    let (rank, matched) = weighted_rank(
        &weights,
        &["trend_follow", "volume_breakout", "rebound"],
        100,
    );
    assert_eq!(rank, 100);
    assert_eq!(matched.len(), 2);
    let (rank, _) = weighted_rank(&weights, &["rebound"], 90);
    assert_eq!(rank, 90, "disabled-only match falls back to 1.0x");
    let (rank, _) = weighted_rank(&weights, &["unknown_tag"], 72);
    assert_eq!(rank, 72);
}

#[test]
fn v21_to_v22_migration_preserves_rows_and_matches_new_schema() {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("fund.db")).unwrap();
    for step in crate::migration::all_migrations()
        .iter()
        .filter(|m| m.version <= 21)
    {
        conn.execute_batch(step.sql).unwrap();
        conn.pragma_update(None, "user_version", step.version)
            .unwrap();
    }
    conn.execute(
        "INSERT INTO opportunity_snapshots(snapshot_id,facts_key,data_revision,generated_at,projection_json) VALUES('old','old-facts','9','0','{\"snapshotId\":\"old\",\"items\":[]}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO opportunity_observations(snapshot_id,candidate_id,horizon,result_json) VALUES('old','old',3,'{\"status\":\"immature\"}')",
        [],
    )
    .unwrap();
    assert_eq!(
        crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    assert_eq!(
        crate::migration::run_migrations(dir.path(), &conn, "test").unwrap(),
        crate::storage::SCHEMA_VERSION,
        "v22 迁移幂等"
    );
    let kept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM opportunity_observations WHERE snapshot_id='old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "v21 观察数据保留");
    let strategy_tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'strategy%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        strategy_tables,
        vec![
            "strategy_catalog",
            "strategy_config",
            "strategy_weight_overrides"
        ]
    );
    let fresh = Store::open_in_memory().unwrap();
    for table in strategy_tables {
        let columns = |c: &rusqlite::Connection| -> Vec<String> {
            c.prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            columns(&conn),
            fresh
                .with_read(|c| Ok::<_, FinanceError>(columns(c)))
                .unwrap(),
            "{table} 新旧库结构一致"
        );
    }
}

#[test]
fn rank_weighting_matches_donor_compute_rank_score_restricted_to_weight_term() {
    // 供体 _compute_rank_score 的质量/来源/状态/风险项在本投影无对应候选字段，
    // 显式退化为 rank=clamp(base×weight,0,100)；样例由供体公式生成（见 fixture 头）。
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/opportunity-rank-parity.json")).unwrap();
    assert!(fixture["cases"].as_array().unwrap().len() >= 80);
    for case in fixture["cases"].as_array().unwrap() {
        let base = case["base"].as_i64().unwrap() as i32;
        let weight_ppm = case["weightPpm"].as_i64().unwrap();
        let weights = json!({"weights": {
            "parity": {"weightPpm": weight_ppm, "enabled": true}
        }});
        let (rank, matched) = weighted_rank(&weights, &["parity"], base);
        assert_eq!(
            i64::from(rank),
            case["rank"].as_i64().unwrap(),
            "case {case}"
        );
        assert_eq!(
            matched,
            vec![json!({"code": "parity", "weightPpm": weight_ppm})]
        );
    }
}
