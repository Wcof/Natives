// 目标进度精度测试：ppm（0.0001%）粒度保持亚 0.1% 进度可见（动态小数显示）。
use super::*;
use crate::assets::AssetsSummary;

fn goal(target_minor: i64) -> FinanceGoalRow {
    FinanceGoalRow {
        id: 1,
        name: "隔离目标".into(),
        target_minor,
        target_date: "2099-12-31".into(),
        scope_kind: "A".into(),
        asset_bindings: "[]".into(),
        is_primary: true,
        archived: false,
        monthly_plan_minor: None,
        version: 1,
        note: String::new(),
    }
}

#[test]
fn progress_keeps_sub_tenth_percent_precision_in_ppm() {
    let conn = Connection::open_in_memory().unwrap();
    // 10 元 / 100 万元 = 0.001%：tenths 粒度四舍五入塌缩为 0，ppm 保留 10。
    let summary = AssetsSummary {
        has_entries: true,
        quality: "ready",
        complete_value_minor: Some(1_000),
        ..Default::default()
    };
    let p = goal_scope_value(&conn, &goal(100_000_000), &summary, None);
    assert_eq!(p.ratio_tenths, Some(0));
    assert_eq!(p.ratio_ppm, Some(10));

    // 常规进度：42.35% → tenths 424（0.1% 四舍五入），ppm 423_500。
    let summary = AssetsSummary {
        has_entries: true,
        quality: "ready",
        complete_value_minor: Some(42_350_000),
        ..Default::default()
    };
    let p = goal_scope_value(&conn, &goal(100_000_000), &summary, None);
    assert_eq!(p.ratio_tenths, Some(424));
    assert_eq!(p.ratio_ppm, Some(423_500));

    // 真实零：两粒度一致为 0。
    let summary = AssetsSummary {
        has_entries: true,
        quality: "ready",
        complete_value_minor: Some(0),
        ..Default::default()
    };
    let p = goal_scope_value(&conn, &goal(100_000_000), &summary, None);
    assert_eq!(p.ratio_tenths, Some(0));
    assert_eq!(p.ratio_ppm, Some(0));
    assert!(p.computable);
}
