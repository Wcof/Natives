//! 市场发现有界池测试（R2 · A32/A38 部分）：符号规范化、种子幂等与留存淘汰、
//! 投影消费有界、发现行进入生产候选且池标记 market、投影不联网只读。

use super::*;
use crate::assets::AssetsSummary;
use crate::storage::Store;

fn entry(symbol: &str, name: &str, pct_raw: i64) -> DiscoveryEntry {
    parse_symbol(symbol, name, "隔离板块", pct_raw).expect("valid symbol")
}

fn count(store: &Store) -> i64 {
    store
        .with_read(|c| {
            Ok::<_, FinanceError>(
                c.query_row("SELECT COUNT(*) FROM opportunity_discovery", [], |r| {
                    r.get(0)
                })
                .unwrap(),
            )
        })
        .unwrap()
}

#[test]
fn symbol_parsing_accepts_a_share_shapes_and_rejects_others() {
    let ok = entry("sz000676", "隔离股", 850);
    assert_eq!(ok.market, "SZSE");
    assert_eq!(ok.code, "000676");
    assert_eq!(ok.kind, "stock");
    assert_eq!(entry("sh510300", "隔离ETF", 0).kind, "etf");
    assert!(
        parse_symbol("hk00700", "腾讯", "x", 0).is_none(),
        "港美不在投影行情面"
    );
    assert!(parse_symbol("usAAPL", "Apple", "x", 0).is_none());
    assert!(parse_symbol("sh60051", "短代码", "x", 0).is_none());
    assert!(parse_symbol("sh600519", "", "x", 0).is_none(), "无名不入池");
}

#[test]
fn seed_is_idempotent_evicts_oldest_and_projection_reads_bounded() {
    let store = Store::open_in_memory().unwrap();
    // 幂等：同 symbol 覆盖更新，不新增行。
    let _ = store
        .with_write(|c| seed(c, &[entry("sh600519", "隔离A", 100)]))
        .unwrap();
    let _ = store
        .with_write(|c| seed(c, &[entry("sh600519", "隔离A更新", 200)]))
        .unwrap();
    assert_eq!(count(&store), 1);
    // 留存上限：塞入 RETENTION+5 行后只剩最新 RETENTION 行。
    let batch: Vec<DiscoveryEntry> = (0..RETENTION + 5)
        .map(|i| entry(&format!("sz{i:06}"), &format!("隔离{i}"), i as i64))
        .collect();
    let _ = store.with_write(|c| seed(c, &batch)).unwrap();
    assert_eq!(count(&store), RETENTION);
    // 最旧的 5 行（sh600519 与 sz000000~sz000003）被淘汰。
    let kept = projection_entries_of(&store);
    assert!(kept.len() <= PROJECTION_LIMIT as usize);
    assert!(
        !kept.iter().any(|e| e.symbol == "sh600519"),
        "oldest evicted"
    );
    // 投影按涨幅降序。
    let pcts: Vec<i64> = kept.iter().map(|e| e.change_pct_raw).collect();
    let mut sorted = pcts.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(pcts, sorted, "projection reads change-pct descending");
    // 读取端不写库：revision 不变。
    let before = store
        .with_read(crate::storage::Store::data_revision)
        .unwrap();
    let _ = projection_entries_of(&store);
    assert_eq!(
        store
            .with_read(crate::storage::Store::data_revision)
            .unwrap(),
        before
    );
}

fn projection_entries_of(store: &Store) -> Vec<DiscoveryEntry> {
    store.with_snapshot(projection_entries).unwrap()
}

#[test]
fn discovery_rows_become_market_pool_candidates_in_production_projection() {
    let store = Store::open_in_memory().unwrap();
    let today = crate::finance::today_cn();
    let _ = store
        .with_write(|c| seed(c, &[entry("sh600519", "隔离股", 980)]))
        .unwrap();
    let out = store
        .with_read(|c| {
            crate::opportunities::projection(c, &AssetsSummary::default(), &today, Some(1000))
        })
        .unwrap();
    assert_eq!(
        out["poolCoverage"], "recorded-holdings-watchlist-and-bounded-market",
        "coverage must name the discovery pool explicitly"
    );
    let item = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["securityId"] == "security:SSE:600519")
        .expect("discovered symbol becomes a candidate");
    assert_eq!(item["pool"], "market");
    assert_eq!(item["isHolding"], false);
    assert_eq!(
        item["quality"], "insufficient_data",
        "cold cache: no invented score"
    );
    assert!(item["score"].is_null());
    // 持仓身份优先：同一证券被登记为持仓后不再显示 market 池。
    let mut summary = AssetsSummary::default();
    summary.manual.push(crate::assets::ManualAssetRow {
        id: 1,
        account_id: 1,
        account_name: "隔离".into(),
        product_type: "stock".into(),
        instrument_key: "security:SSE:600519".into(),
        name: "隔离股".into(),
        risk_class: "equity".into(),
        market_value_minor: 100,
        valued_at: today.clone(),
        stale: false,
        overlap_review: false,
        legacy_overlap_distinct: false,
    });
    let out = store
        .with_read(|c| crate::opportunities::projection(c, &summary, &today, Some(1000)))
        .unwrap();
    let item = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["securityId"] == "security:SSE:600519")
        .unwrap();
    assert_eq!(
        item["pool"], "holdings",
        "holding identity wins over discovery"
    );
    // 空表：inputCount 不含发现项、coverage 不变。
    let _ = store
        .with_write(|c| c.execute("DELETE FROM opportunity_discovery", []))
        .unwrap();
    let empty = store
        .with_read(|c| {
            crate::opportunities::projection(c, &AssetsSummary::default(), &today, Some(1000))
        })
        .unwrap();
    assert_eq!(empty["inputCount"], 0);
}

#[test]
fn v22_to_v23_migration_preserves_rows_and_matches_new_schema() {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("fund.db")).unwrap();
    for step in crate::migration::all_migrations()
        .iter()
        .filter(|m| m.version <= 22)
    {
        conn.execute_batch(step.sql).unwrap();
        conn.pragma_update(None, "user_version", step.version)
            .unwrap();
    }
    conn.execute(
        "INSERT INTO strategy_config(id,revision,updated_at) VALUES(1,4,'0') ON CONFLICT(id) DO UPDATE SET revision=4",
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
        "v23 迁移幂等"
    );
    assert_eq!(
        conn.query_row("SELECT revision FROM strategy_config WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        4,
        "v22 数据保留"
    );
    let columns = |c: &rusqlite::Connection| -> Vec<String> {
        c.prepare("PRAGMA table_info(opportunity_discovery)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let fresh = Store::open_in_memory().unwrap();
    assert_eq!(
        columns(&conn),
        fresh
            .with_read(|c| Ok::<_, FinanceError>(columns(c)))
            .unwrap(),
        "新旧库结构一致"
    );
}
