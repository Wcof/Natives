//! collector_test: 验证 30+ 工具本地扫描、Token/费用计算与增量去重。

use std::fs::{create_dir_all, File};
use std::io::Write;
use tempfile::tempdir;
use tokenusage_module::collector::pricing::{calculate_cost_micros, get_model_price};
use tokenusage_module::collector::scanner::scan_all_sources;
use tokenusage_module::storage::Store;

#[test]
fn test_pricing_calculation() {
    let price = get_model_price("claude-3-7-sonnet-20250219");
    assert_eq!(price.input_per_million, 3_000_000);
    assert_eq!(price.output_per_million, 15_000_000);
    assert_eq!(price.cache_read_per_million, 300_000);

    // 10,000 input tokens (with 2,000 cache read), 1,000 output tokens
    // uncached input = 8,000 -> 8,000 * 3 = 24,000 micros ($0.024)
    // cache read = 2,000 -> 2,000 * 0.3 = 600 micros ($0.0006)
    // output = 1,000 -> 1,000 * 15 = 15,000 micros ($0.015)
    // total = 39,600 micros ($0.0396)
    let cost = calculate_cost_micros("claude-3-7-sonnet", 10_000, 1_000, 2_000, 0, 0);
    assert_eq!(cost, 39_600);

    // DeepSeek R1 reasoning tokens
    let r1_price = get_model_price("deepseek-reasoner");
    assert_eq!(r1_price.input_per_million, 550_000);
    assert_eq!(r1_price.output_per_million, 2_190_000);
    let r1_cost = calculate_cost_micros("deepseek-reasoner", 1_000_000, 0, 0, 0, 1_000_000);
    assert_eq!(r1_cost, 550_000 + 2_190_000);
}

#[test]
fn test_scanner_and_deduplication() {
    let temp = tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let home_dir = temp.path().join("home");
    create_dir_all(&data_dir).unwrap();

    let store = Store::open(&data_dir).unwrap();

    // 构造模拟 Claude Code 项目与会话
    let claude_dir = home_dir.join(".claude/projects/my-project");
    create_dir_all(&claude_dir).unwrap();
    let claude_file = claude_dir.join("chat.jsonl");
    let mut f = File::create(&claude_file).unwrap();
    writeln!(
        f,
        r#"{{"session_id":"sess-1","turn_id":"t1","model":"claude-3-7-sonnet","input_tokens":5000,"output_tokens":1000,"cache_read_tokens":1000,"timestamp":"2026-09-20T10:00:00Z","title":"Refactor code","project_path":"/repo"}}"#
    ).unwrap();
    writeln!(
        f,
        r#"{{"session_id":"sess-1","turn_id":"t2","model":"claude-3-7-sonnet","input_tokens":8000,"output_tokens":2000,"cache_read_tokens":2000,"timestamp":"2026-09-20T10:05:00Z","title":"Refactor code","project_path":"/repo"}}"#
    ).unwrap();

    // 构造模拟 Codex 会话
    let codex_dir = home_dir.join(".codex/sessions");
    create_dir_all(&codex_dir).unwrap();
    let codex_file = codex_dir.join("session_codex.jsonl");
    let mut f2 = File::create(&codex_file).unwrap();
    writeln!(
        f2,
        r#"{{"session_id":"codex-s1","turn_id":"c1","model":"gpt-4o","input_tokens":4000,"output_tokens":800,"timestamp":"2026-09-20T11:00:00Z","title":"Codex fix"}}"#
    ).unwrap();

    // 第一次扫描
    let summary1 = scan_all_sources(&store, Some(&home_dir)).unwrap();
    assert_eq!(summary1.new_records, 3);
    assert!(summary1.scanned_sources >= 2);

    // 验证数据库状态
    store.with_read(|conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 3);

        let session_count: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(session_count, 2);

        let (total_tokens, cost_micros): (i64, i64) = conn.query_row(
            "SELECT total_input_tokens + total_output_tokens, total_cost_micros FROM sessions WHERE session_id = 'sess-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(total_tokens, (5000 + 1000) + (8000 + 2000));
        assert!(cost_micros > 0);

        // 验证 daily_aggregates
        let daily_tokens: i64 = conn.query_row(
            "SELECT SUM(total_tokens) FROM daily_aggregates WHERE date = '2026-09-20'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(daily_tokens, (5000 + 1000) + (8000 + 2000) + (4000 + 800));

        Ok(())
    }).unwrap();

    // 第二次扫描（增量去重测试）：相同文件不产生重复记录
    let summary2 = scan_all_sources(&store, Some(&home_dir)).unwrap();
    assert_eq!(summary2.new_records, 0);

    store
        .with_read(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 3, "Record count must remain 3 after re-scan");
            Ok(())
        })
        .unwrap();
}
