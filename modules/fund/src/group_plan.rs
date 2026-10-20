//! group_plan 模块：首页自定义资金分组（方案 11 §4，schema v7）。
//!
//! - 分组是整个管理投资资金的一套分类（A 分母），不是每目标一套资金分摊。
//! - `groupId` 稳定且不靠名称；名称去首尾空格 1~40 字，大小写归一后唯一。
//! - 比例沿用 tenths 精度 0~1000，0~50 组，合计 ≤1000（方案 01 P03：未分配比例合法）。
//! - 绑定引用：基金 sourceKind=fund(accountId,fundId)；手工 sourceKind=manual(assetId)。
//!   同一真实持有资产在当前方案中最多归属一次（服务端唯一约束，不靠前端）。
//! - 整个保存为一个事务；expectedRevision 冲突不覆盖；删组/迁移归属不删资产。
//! - 分组名不改变 risk_class、币种或资产类型；来源删除后绑定标记失效，
//!   不借用同名新资产。

use crate::assets::AssetError;
use crate::storage::Store;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub const GROUP_NAME_MAX: usize = 40;
pub const GROUP_DESC_MAX: usize = 200;
pub const GROUPS_MAX: usize = 50;
pub const TENTHS_SUM: i64 = 1000;

#[derive(Debug, PartialEq, Eq)]
pub enum GroupPlanError {
    /// 校验失败（输入非法/超限/合计不为 1000 等）。
    Invalid(String),
    /// expectedRevision 与当前方案 revision 不一致。
    Conflict(String),
    /// 引用的来源（基金持仓/手工资产）不存在或已失效。
    NotFound(String),
    Db(String),
}

impl From<rusqlite::Error> for GroupPlanError {
    fn from(e: rusqlite::Error) -> Self {
        GroupPlanError::Db(e.to_string())
    }
}

impl From<AssetError> for GroupPlanError {
    fn from(e: AssetError) -> Self {
        match e {
            AssetError::Db(m) => GroupPlanError::Db(m),
            other => GroupPlanError::Invalid(other.message()),
        }
    }
}

impl GroupPlanError {
    pub fn code(&self) -> &'static str {
        match self {
            GroupPlanError::Invalid(_) => "GROUP_PLAN_INVALID",
            GroupPlanError::Conflict(_) => "GROUP_PLAN_CONFLICT",
            GroupPlanError::NotFound(_) => "GROUP_PLAN_NOT_FOUND",
            GroupPlanError::Db(_) => "GROUP_PLAN_DB",
        }
    }

    pub fn message(&self) -> String {
        match self {
            GroupPlanError::Invalid(m) => format!("分组配置无效：{m}"),
            GroupPlanError::Conflict(m) => format!("分组配置已被其他窗口修改：{m}"),
            GroupPlanError::NotFound(m) => format!("引用的来源不存在：{m}"),
            GroupPlanError::Db(e) => format!("数据库错误：{e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInput {
    /// 稳定 id；新建为空串时由服务端生成 `g<随机>`。
    pub group_id: String,
    pub name: String,
    /// 用户填写的分类说明（最长 200 字；新建必填）。
    pub description: String,
    /// 0~1000（tenths）。
    pub ratio_tenths: i64,
    pub sort_order: i64,
    /// 绑定引用：fund => (account_id, fund_id)；manual => asset_id。
    pub fund_bindings: Vec<(i64, i64)>,
    pub manual_bindings: Vec<i64>,
    pub offline_bindings: Vec<i64>,
    pub cash_bindings: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlanInput {
    pub groups: Vec<GroupInput>,
    /// 客户端读到的方案 revision；尚无方案时为 0。
    pub expected_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub group_id: String,
    pub name: String,
    pub description: String,
    pub ratio_tenths: i64,
    pub sort_order: i64,
    pub fund_bindings: Vec<(i64, i64)>,
    pub manual_bindings: Vec<i64>,
    pub offline_bindings: Vec<i64>,
    pub cash_bindings: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlan {
    pub revision: i64,
    /// 首次保存成功后为 true；旧 B 偏离规则随之暂停（原因记录在事件流）。
    pub active: bool,
    pub paused_legacy_rules: bool,
    pub groups: Vec<GroupRow>,
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// 大小写归一（简单 lower；名称唯一性约束列）。
fn name_norm(name: &str) -> String {
    name.to_lowercase()
}

/// 校验输入的分组集合：数量、名称、比例合计、组内引用去重、跨组引用唯一。
/// 仅做纯校验，不触库；来源存在性在事务里再查（用户资产可并发变化）。
fn validate_groups(groups: &[GroupInput]) -> Result<(), GroupPlanError> {
    if groups.len() > GROUPS_MAX {
        return Err(GroupPlanError::Invalid(format!(
            "分组数量不能超过 {GROUPS_MAX}"
        )));
    }
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    // 跨组引用唯一：fund => (account,fund)，manual => assetId。
    let mut seen_fund: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut seen_manual: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut seen_extra = std::collections::HashSet::new();
    let mut sum: i64 = 0;
    for g in groups {
        let name = g.name.trim();
        if name.is_empty() || name.chars().count() > GROUP_NAME_MAX {
            return Err(GroupPlanError::Invalid(format!(
                "分组名称需 1~{GROUP_NAME_MAX} 字"
            )));
        }
        if !seen_names.insert(name_norm(name)) {
            return Err(GroupPlanError::Invalid(format!("分组名称重复：{name}")));
        }
        let desc = g.description.trim();
        if g.group_id.is_empty() {
            if desc.is_empty() || desc.chars().count() > GROUP_DESC_MAX {
                return Err(GroupPlanError::Invalid(format!(
                    "新建分组说明需 1~{GROUP_DESC_MAX} 字"
                )));
            }
        } else if desc.chars().count() > GROUP_DESC_MAX {
            return Err(GroupPlanError::Invalid(format!(
                "分组说明不能超过 {GROUP_DESC_MAX} 字"
            )));
        }
        if !(0..=TENTHS_SUM).contains(&g.ratio_tenths) {
            return Err(GroupPlanError::Invalid(format!(
                "分组比例需在 0~100%：{}",
                g.name.trim()
            )));
        }
        sum = sum
            .checked_add(g.ratio_tenths)
            .ok_or_else(|| GroupPlanError::Invalid("比例合计溢出".into()))?;
        // 绑定去重对所有组生效（含服务端生成 id 的新组）。
        for (a, f) in &g.fund_bindings {
            if !seen_fund.insert((*a, *f)) {
                return Err(GroupPlanError::Invalid("同一资产不能归属多个分组".into()));
            }
        }
        for id in &g.manual_bindings {
            if !seen_manual.insert(*id) {
                return Err(GroupPlanError::Invalid("同一资产不能归属多个分组".into()));
            }
        }
        for (kind, ids) in [("offline", &g.offline_bindings), ("cash", &g.cash_bindings)] {
            for id in ids {
                if *id <= 0 || !seen_extra.insert((kind, *id)) {
                    return Err(GroupPlanError::Invalid("同一资产不能归属多个分组".into()));
                }
            }
        }
        if g.group_id.is_empty() {
            continue; // 新建，服务端生成 id
        }
        if !seen_ids.insert(g.group_id.clone()) {
            return Err(GroupPlanError::Invalid(format!(
                "groupId 重复：{}",
                g.group_id
            )));
        }
    }
    // 方案 01 P03：合计 ≤100% 可保存（未分配比例=1000−sum）；超过即时拒绝；
    // 空方案合法（sum=0）。不自动归一化或挤占其他组比例。
    if sum > TENTHS_SUM {
        return Err(GroupPlanError::Invalid(format!(
            "分组比例合计不能超过 100%（当前 {}%）",
            sum as f64 / 10.0
        )));
    }
    Ok(())
}

/// 生成 `g<16hex>` 组 id（绑定稳定性不依赖名称）。单写进程内由
/// 时间戳＋原子计数保证唯一，不引入新依赖。
fn new_group_id() -> Result<String, GroupPlanError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Ok(format!("g{nanos:016x}{seq:04x}"))
}

/// 确认引用的来源仍存在：基金持仓按 (account_id, fund_id) 在 positions 有行；
/// 手工资产按 id 在 manual_assets 有行。失效引用直接拒绝（保存时资产仍在的
/// 才允许绑定；来源后续删除由读取侧标 invalid，不自动删除绑定行）。
fn validate_sources(conn: &Connection, groups: &[GroupInput]) -> Result<(), GroupPlanError> {
    let manual_refs: std::collections::HashSet<i64> = groups
        .iter()
        .flat_map(|g| g.manual_bindings.iter().copied())
        .collect();
    for g in groups {
        for (kind, ids) in [("offline", &g.offline_bindings), ("cash", &g.cash_bindings)] {
            for id in ids {
                let sql = if kind == "offline" {
                    "SELECT EXISTS(SELECT 1 FROM offline_assets WHERE id=?1 AND deleted_at IS NULL)"
                } else {
                    "SELECT EXISTS(SELECT 1 FROM manual_assets WHERE account_id=?1 AND product_type='cash')"
                };
                if !conn.query_row(sql, [id], |r| r.get::<_, bool>(0))? {
                    return Err(GroupPlanError::NotFound(format!("{kind}:{id}")));
                }
                if kind == "cash" {
                    let manual_id: i64 = conn.query_row(
                        "SELECT id FROM manual_assets WHERE account_id=?1 AND product_type='cash'",
                        [id],
                        |r| r.get(0),
                    )?;
                    if manual_refs.contains(&manual_id) {
                        return Err(GroupPlanError::Invalid(
                            "同一现金不能通过两种引用重复归组".into(),
                        ));
                    }
                }
            }
        }
        for (account_id, fund_id) in &g.fund_bindings {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM positions WHERE account_id=?1 AND fund_id=?2 AND quantity_raw>0",
                    params![account_id, fund_id],
                    |r| r.get(0),
                )
                .map_err(GroupPlanError::from)?;
            if exists == 0 {
                return Err(GroupPlanError::NotFound(format!(
                    "基金持仓 account={account_id} fund={fund_id}"
                )));
            }
        }
        for asset_id in &g.manual_bindings {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM manual_assets WHERE id=?1 AND deleted_at IS NULL AND converted_ref IS NULL",
                    params![asset_id],
                    |r| r.get(0),
                )
                .map_err(GroupPlanError::from)?;
            if exists == 0 {
                return Err(GroupPlanError::NotFound(format!("手工资产 id={asset_id}")));
            }
        }
    }
    Ok(())
}

/// 读取当前方案（无方案返回 None）。
pub fn get_plan(conn: &Connection) -> Result<Option<GroupPlan>, GroupPlanError> {
    let exists: i64 = conn
        .query_row("SELECT COUNT(*) FROM group_plan", [], |r| r.get(0))
        .map_err(GroupPlanError::from)?;
    if exists == 0 {
        return Ok(None);
    }
    let (revision, paused): (i64, i64) = conn
        .query_row(
            "SELECT revision, paused_legacy_rules FROM group_plan WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(GroupPlanError::from)?;
    let mut stmt = conn
        .prepare(
            "SELECT group_id, name, ratio_tenths, sort_order, COALESCE(description, '') FROM group_plan_groups
             ORDER BY sort_order, group_id",
        )
        .map_err(GroupPlanError::from)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(GroupPlanError::from)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(GroupPlanError::from)?;
    let mut groups = Vec::with_capacity(rows.len());
    for (group_id, name, ratio_tenths, sort_order, description) in rows {
        let mut fund_bindings = Vec::new();
        let mut manual_bindings = Vec::new();
        let mut bstmt = conn
            .prepare(
                "SELECT source_kind, account_id, fund_id, asset_id FROM group_plan_bindings
                 WHERE group_id=?1",
            )
            .map_err(GroupPlanError::from)?;
        let brows = bstmt
            .query_map(params![group_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(GroupPlanError::from)?;
        for row in brows {
            let (kind, account_id, fund_id, asset_id) = row.map_err(GroupPlanError::from)?;
            match kind.as_str() {
                "fund" => {
                    fund_bindings
                        .push((account_id.unwrap_or_default(), fund_id.unwrap_or_default()));
                }
                "manual" => {
                    manual_bindings.push(asset_id.unwrap_or_default());
                }
                _ => {}
            }
        }
        let mut offline_bindings = Vec::new();
        let mut cash_bindings = Vec::new();
        let mut extra=conn.prepare("SELECT source_kind,source_id FROM group_plan_extra_bindings WHERE group_id=?1 ORDER BY source_id")?;
        for row in extra.query_map([&group_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })? {
            let (kind, id) = row?;
            if kind == "offline" {
                offline_bindings.push(id);
            } else {
                cash_bindings.push(id);
            }
        }
        groups.push(GroupRow {
            group_id,
            name,
            description,
            ratio_tenths,
            sort_order,
            fund_bindings,
            manual_bindings,
            offline_bindings,
            cash_bindings,
        });
    }
    Ok(Some(GroupPlan {
        revision,
        active: true,
        paused_legacy_rules: paused == 1,
        groups,
    }))
}

/// 保存（激活）方案：单事务全量替换 groups+bindings；expectedRevision 不匹配
/// 则返回 Conflict，不覆盖。首次保存后 paused_legacy_rules=1（旧 B 偏离规则
/// 暂停；历史事件保留）。
pub fn save_plan_tx(store: &Store, input: &GroupPlanInput) -> Result<GroupPlan, GroupPlanError> {
    validate_groups(&input.groups)?;
    store.with_write(|conn| -> Result<GroupPlan, GroupPlanError> {
        let current: Option<i64> = conn
            .query_row("SELECT revision FROM group_plan WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(GroupPlanError::from)?;
        match current {
            None if input.expected_revision!=0=>return Err(GroupPlanError::Conflict("方案版本已变化".into())),
            Some(rev) if rev != input.expected_revision => {
                return Err(GroupPlanError::Conflict(format!(
                    "预览版本 {}，当前版本 {rev}", input.expected_revision
                )));
            }
            _ => {}
        }
        validate_sources(conn, &input.groups)?;
        let now = now_iso();
        if current.is_none() {
            conn.execute(
                "INSERT INTO group_plan(id, revision, paused_legacy_rules, created_at, updated_at)
                 VALUES(1, 0, 1, ?1, ?1)",
                params![now],
            )
            .map_err(GroupPlanError::from)?;
        }
        conn.execute("DELETE FROM group_plan_extra_bindings", [])?;
        conn.execute("DELETE FROM group_plan_bindings", [])
            .map_err(GroupPlanError::from)?;
        conn.execute("DELETE FROM group_plan_groups", [])
            .map_err(GroupPlanError::from)?;
        for g in &input.groups {
            let group_id = if g.group_id.is_empty() {
                new_group_id()?
            } else {
                g.group_id.clone()
            };
            conn.execute(
                "INSERT INTO group_plan_groups(group_id, name, name_norm, ratio_tenths, sort_order, description, created_at, updated_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    group_id,
                    g.name.trim(),
                    name_norm(g.name.trim()),
                    g.ratio_tenths,
                    g.sort_order,
                    g.description.trim(),
                    now
                ],
            )
            .map_err(GroupPlanError::from)?;
            for (account_id, fund_id) in &g.fund_bindings {
                conn.execute(
                    "INSERT INTO group_plan_bindings(group_id, source_kind, account_id, fund_id, asset_id, created_at)
                     VALUES(?1,'fund',?2,?3,NULL,?4)",
                    params![group_id, account_id, fund_id, now],
                )
                .map_err(GroupPlanError::from)?;
            }
            for (kind,ids) in [("offline",&g.offline_bindings),("cash",&g.cash_bindings)] {
                for id in ids {conn.execute("INSERT INTO group_plan_extra_bindings(group_id,source_kind,source_id,created_at) VALUES(?1,?2,?3,?4)",params![group_id,kind,id,now])?;}
            }
            for asset_id in &g.manual_bindings {
                conn.execute(
                    "INSERT INTO group_plan_bindings(group_id, source_kind, account_id, fund_id, asset_id, created_at)
                     VALUES(?1,'manual',NULL,NULL,?2,?3)",
                    params![group_id, asset_id, now],
                )
                .map_err(GroupPlanError::from)?;
            }
        }
        // 方案 revision +1（dataRevision 由 with_write 统一递增）。
        conn.execute(
            "UPDATE group_plan SET revision=revision+1, updated_at=?1 WHERE id=1",
            params![now],
        )
        .map_err(GroupPlanError::from)?;
        let revision: i64 = conn
            .query_row("SELECT revision FROM group_plan WHERE id=1", [], |r| r.get(0))
            .map_err(GroupPlanError::from)?;
        let paused: i64 = conn
            .query_row(
                "SELECT paused_legacy_rules FROM group_plan WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(GroupPlanError::from)?;
        // 读回完整方案（含生成的组 id）。
        let groups = get_plan(conn)?.ok_or_else(|| GroupPlanError::Db("方案读回失败".into()))?.groups;
        Ok(GroupPlan {
            revision,
            active: true,
            paused_legacy_rules: paused == 1,
            groups,
        })
    })
}

/// 旧 B 配置 → 新 A 参考预览（不改库）：复制旧类别名与比例作为建议草稿。
/// 按 risk_class 提议归属；unknown/争议资产放未归类，不猜。
pub fn import_preview(
    conn: &Connection,
    summary: &crate::assets::AssetsSummary,
) -> Result<Value, GroupPlanError> {
    let targets = crate::planning::get_allocation(conn).map_err(GroupPlanError::from)?;
    if targets.is_empty() {
        return Ok(json!({"available": false, "reason": "no_legacy_config"}));
    }
    // 旧 B 分母 = A − R（预留）。预留未知时 B 不可计算，仅展示 A。
    let reserved = crate::finance::cash_position(conn)
        .map_err(|e| GroupPlanError::Invalid(e.message()))?
        .reserved_minor;
    let denom_b = match (summary.complete_value_minor, reserved) {
        (Some(a), Some(r)) => Some(a.saturating_sub(r)),
        _ => None,
    };
    let mut items = Vec::new();
    for t in &targets {
        let suggested_a = match (summary.complete_value_minor, denom_b) {
            (Some(a), Some(b)) if b > 0 => Some(a * t.target_tenths / b),
            _ => None,
        };
        items.push(json!({
            "className": t.risk_class,
            "legacyBasisTenths": t.target_tenths,
            "suggestedATenths": t.target_tenths,
            "suggestedAMinor": suggested_a,
        }));
    }
    Ok(json!({
        "available": true,
        "denominatorA": summary.complete_value_minor,
        "denominatorB": denom_b,
        "note": "旧配置按 B=A−R 计算；此处仅作参考草稿，保存后才生效。",
        "classes": items,
    }))
}

// ---------------------------------------------------------------------------
// 同快照分组投影（方案 11 §3.3：A 分母；金额未知与真实零分开）
// ---------------------------------------------------------------------------

pub use crate::group_projection::group_allocation;

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn g(id: &str, name: &str, ratio: i64, sort: i64) -> GroupInput {
        GroupInput {
            group_id: id.into(),
            name: name.into(),
            description: "默认测试分类说明".into(),
            ratio_tenths: ratio,
            sort_order: sort,
            fund_bindings: vec![],
            manual_bindings: vec![],
            offline_bindings: vec![],
            cash_bindings: vec![],
        }
    }

    #[test]
    fn save_requires_sum_1000_and_unique_names() {
        let store = store();
        // 方案 01 P03：合计 70% 可保存（未分配比例合法）。
        save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g("", "权益", 600, 0), g("", "现金", 300, 1)],
                expected_revision: 0,
            },
        )
        .unwrap();
        // 超过 100% 拒绝。
        let err = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g("", "权益", 600, 0), g("", "现金", 500, 1)],
                expected_revision: 1,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "GROUP_PLAN_INVALID");
        // 重名（大小写不同）。
        let err = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g("", "Equity", 500, 0), g("", "equity", 500, 1)],
                expected_revision: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "GROUP_PLAN_INVALID");
    }

    #[test]
    fn save_activates_and_revisions_conflict() {
        let store = store();
        let plan = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g("", "权益", 600, 0), g("", "现金", 400, 1)],
                expected_revision: 0,
            },
        )
        .unwrap();
        assert_eq!(plan.revision, 1);
        assert!(plan.paused_legacy_rules);
        // 旧 revision 提交被拒。
        let err = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g("", "权益", 700, 0), g("", "现金", 300, 1)],
                expected_revision: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "GROUP_PLAN_CONFLICT");
        // 正确 revision 再保存，revision 递增且 id 稳定。
        let plan2 = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![
                    g(&plan.groups[0].group_id, "权益改名", 700, 0),
                    g(&plan.groups[1].group_id, "现金", 300, 1),
                ],
                expected_revision: 1,
            },
        )
        .unwrap();
        assert_eq!(plan2.revision, 2);
        assert_eq!(plan2.groups[0].group_id, plan.groups[0].group_id);
        assert_eq!(plan2.groups[0].name, "权益改名");
    }

    #[test]
    fn binding_uniqueness_is_server_enforced() {
        let store = store();
        store
            .with_write(|c| -> rusqlite::Result<()> {
                c.execute(
                    "INSERT INTO accounts(name,created_at,updated_at) VALUES('A',0,0)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO manual_assets(account_id,product_type,instrument_key,name,market_value_minor,valued_at,created_at,updated_at)
                     VALUES(1,'cash','cash','现金',100, '', 0, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let mut g1 = g("", "一", 500, 0);
        g1.manual_bindings = vec![1];
        let mut g2 = g("", "二", 500, 1);
        g2.manual_bindings = vec![1];
        let err = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g1, g2],
                expected_revision: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "GROUP_PLAN_INVALID");
        // 绑定不存在的来源被拒。
        let err = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![{
                    let mut x = g("", "三", 1000, 0);
                    x.manual_bindings = vec![999];
                    x
                }],
                expected_revision: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "GROUP_PLAN_NOT_FOUND");
    }

    #[test]
    fn allocation_projection_conserves_amounts() {
        // 夹具（方案 11 §9）：权益 50000 + 黄金 20000 + 现金 30000 = A 100000；
        // 分组 60/20/20；黄金移出 → 未归类 20000，A 不变；金额未知与零分开。
        let store = store();
        let today = "2026-10-01";
        store
            .with_write(|c| -> rusqlite::Result<()> {
                c.execute(
                    "INSERT INTO accounts(name,created_at,updated_at) VALUES('甲',0,0)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,created_at,updated_at)
                     VALUES(1,'stock','security:SSE:600519','股票','equity',5000000,?1,0,0)",
                    [today],
                )?;
                c.execute(
                    "INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,created_at,updated_at)
                     VALUES(1,'gold','gold:金','黄金','gold',2000000,?1,0,0)",
                    [today],
                )?;
                c.execute(
                    "INSERT INTO manual_assets(account_id,product_type,instrument_key,name,risk_class,market_value_minor,valued_at,created_at,updated_at)
                     VALUES(1,'cash','cash','现金','cash',3000000,?1,0,0)",
                    [today],
                )?;
                Ok(())
            })
            .unwrap();
        let save = |groups: Vec<GroupInput>| {
            save_plan_tx(
                &store,
                &GroupPlanInput {
                    groups,
                    expected_revision: 0,
                },
            )
        };
        let mut equity = g("", "权益", 600, 0);
        equity.manual_bindings = vec![1];
        let mut gold = g("", "黄金", 200, 1);
        gold.manual_bindings = vec![2];
        let mut cash = g("", "现金", 200, 2);
        cash.manual_bindings = vec![3];
        save(vec![equity, gold, cash]).unwrap();

        let summary = store
            .with_read(|c| crate::assets::assets_summary(c, today))
            .unwrap();
        assert_eq!(summary.complete_value_minor, Some(10_000_000)); // A = 100000 元
        let alloc = store.with_read(|c| group_allocation(c, &summary)).unwrap();
        let groups = alloc["groups"].as_array().unwrap();
        assert_eq!(groups[0]["currentMinor"].as_i64(), Some(5_000_000));
        assert_eq!(groups[0]["currentTenths"].as_i64(), Some(500)); // 50% ≠ 计划 60%
        assert_eq!(groups[0]["gapTenths"].as_i64(), Some(100));
        assert_eq!(groups[0]["planMinor"].as_i64(), Some(6_000_000));
        assert_eq!(groups[1]["currentMinor"].as_i64(), Some(2_000_000));
        assert_eq!(groups[2]["currentMinor"].as_i64(), Some(3_000_000));
        assert_eq!(alloc["unassignedMinor"].as_i64(), Some(0));

        // 黄金移到未归类：A 不变、未归类 20000、黄金组金额 0（真实零）。
        let mut equity2 = g("", "权益", 600, 0);
        equity2.manual_bindings = vec![1];
        let mut gold2 = g("", "黄金", 200, 1);
        gold2.manual_bindings = vec![];
        let mut cash2 = g("", "现金", 200, 2);
        cash2.manual_bindings = vec![3];
        let revision = alloc["revision"].as_i64().unwrap();
        save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![equity2, gold2, cash2],
                expected_revision: revision,
            },
        )
        .unwrap();
        let alloc2 = store.with_read(|c| group_allocation(c, &summary)).unwrap();
        assert_eq!(alloc2["denominatorMinor"].as_i64(), Some(10_000_000));
        assert_eq!(alloc2["unassignedMinor"].as_i64(), Some(2_000_000));
        assert_eq!(
            alloc2["groups"].as_array().unwrap()[1]["currentMinor"].as_i64(),
            Some(0)
        );
        // 真实零：占比 0%（区别于金额未知 null）；计划 200 仍由 ratioTenths 表达。
        assert_eq!(
            alloc2["groups"].as_array().unwrap()[1]["currentTenths"].as_i64(),
            Some(0)
        );
        assert_eq!(
            alloc2["groups"].as_array().unwrap()[1]["ratioTenths"].as_i64(),
            Some(200)
        );

        // 黄金估值删除路径：先重新绑定黄金（第三次保存），再删除来源，
        // 验证引用失效标记 source_gone，不借用同名新资产。
        let revision2 = alloc2["revision"].as_i64().unwrap();
        let mut equity3 = g("", "权益", 600, 0);
        equity3.manual_bindings = vec![1];
        let mut gold3 = g("", "黄金", 200, 1);
        gold3.manual_bindings = vec![2];
        let mut cash3 = g("", "现金", 200, 2);
        cash3.manual_bindings = vec![3];
        save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![equity3, gold3, cash3],
                expected_revision: revision2,
            },
        )
        .unwrap();
        store
            .with_write(|c| -> rusqlite::Result<()> {
                c.execute("DELETE FROM manual_assets WHERE id=2", [])?;
                Ok(())
            })
            .unwrap();
        let summary3 = store
            .with_read(|c| crate::assets::assets_summary(c, today))
            .unwrap();
        let alloc3 = store.with_read(|c| group_allocation(c, &summary3)).unwrap();
        // 引用已失效（来源删除）：标记 source_gone，不借用同名新资产。
        assert_eq!(alloc3["invalidRefs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn delete_group_keeps_assets() {
        let store = store();
        store
            .with_write(|c| -> rusqlite::Result<()> {
                c.execute(
                    "INSERT INTO accounts(name,created_at,updated_at) VALUES('A',0,0)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO manual_assets(account_id,product_type,instrument_key,name,market_value_minor,valued_at,created_at,updated_at)
                     VALUES(1,'cash','cash','现金',100, '', 0, 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let mut g1 = g("", "一", 1000, 0);
        g1.manual_bindings = vec![1];
        save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g1],
                expected_revision: 0,
            },
        )
        .unwrap();
        // 删除组（不再绑定资产）后资产仍在。
        save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g("", "二", 1000, 0)],
                expected_revision: 1,
            },
        )
        .unwrap();
        let count: i64 = store
            .with_read(|c| c.query_row("SELECT COUNT(*) FROM manual_assets", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn description_validation_and_roundtrip() {
        let store = store();
        // 新建缺少说明时被拒绝
        let mut g_empty_desc = g("", "股票型", 500, 0);
        g_empty_desc.description = "   ".into();
        let err = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g_empty_desc],
                expected_revision: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "GROUP_PLAN_INVALID");

        // 新建说明超过 200 字时被拒绝
        let mut g_long_desc = g("", "股票型", 500, 0);
        g_long_desc.description = "测".repeat(201);
        let err2 = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g_long_desc],
                expected_revision: 0,
            },
        )
        .unwrap_err();
        assert_eq!(err2.code(), "GROUP_PLAN_INVALID");

        // 正确保存与往返读取
        let mut g_valid = g("", "成长权益", 500, 0);
        g_valid.description = "高波动成长型股票及行业基金配置".into();
        let plan = save_plan_tx(
            &store,
            &GroupPlanInput {
                groups: vec![g_valid],
                expected_revision: 0,
            },
        )
        .unwrap();
        assert_eq!(plan.groups[0].description, "高波动成长型股票及行业基金配置");

        let loaded = store.with_read(get_plan).unwrap().unwrap();
        assert_eq!(
            loaded.groups[0].description,
            "高波动成长型股票及行业基金配置"
        );
    }
}
