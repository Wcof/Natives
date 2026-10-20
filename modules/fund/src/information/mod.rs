//! 投资资讯能力（PanWatch M01～M05 迁移，方案 04）。
//!
//! 生产路径：模块 UI → Fund API → 本模块调度（锁外网络）→ 适配器 → Store → 查询投影。
//! 模块未打开时不创建采集任务；刷新由用户显式触发；取消经 runtime 取消令牌传导。

pub mod adapters;
pub(crate) mod cache;
mod finance;
pub mod normalize;
mod scope;
pub use scope::{
    collect_instruments, instrument_ref_for_code, instrument_ref_for_fund,
    instrument_ref_for_keyword, public_stock_code,
};
pub(crate) use scope::{fund_names, fund_names_for_codes};
pub mod store;
pub(crate) mod tasks;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::storage::Store;

/// 覆盖矩阵（T01 实测结论，方案 04 §2；UI 按 unsupported 展示未覆盖品类）。
/// 股票新闻/公告经 `np-anotice-stock`（股票）；基金/ETF 公告经 F10 JJGG；
/// 港美股、线下资产自动资讯 unsupported（允许手工资料）。
pub const COVERAGE_NOTE: &str =
    "公共财经要闻、A股股票新闻/公告与基金/ETF公告已支持；港美股/线下资产专属资讯暂不支持";

/// 每次刷新单来源条数上限（方案 04 §7：每次采集最多 200 条）。
const MAX_ITEMS_PER_SOURCE: usize = 200;
/// 列表默认缓存（方案 04 §7：15 分钟）。
const LIST_TTL_MS: u64 = 15 * 60 * 1000;

/// 刷新结果（方案 04 §6：成功有结果/成功无结果/部分成功/禁用分开）。
#[derive(Debug, Default, serde::Serialize)]
pub struct RefreshOutcome {
    pub sources: Vec<SourceOutcome>,
    pub written: usize,
    pub cancelled: bool,
    pub total_identities: usize,
    pub covered_identities: usize,
    pub next_identity_offset: Option<usize>,
    pub window_days: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct SourceOutcome {
    pub source_id: String,
    pub status: String, // ok_with_items | ok_empty | partial | disabled | error
    pub count: usize,
    pub error_code: Option<String>,
    pub latency_ms: u128,
}

/// 任务注册表：每会话一个刷新任务（方案 04 §7；取消对当前 owner 生效）。
pub struct TaskRegistry {
    generation: AtomicU64,
    cancelled: AtomicU64,
    commit_gate: std::sync::Mutex<()>,
    runtime_cancel: Option<std::sync::Arc<app_runtime_core::CancellationToken>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            cancelled: AtomicU64::new(0),
            commit_gate: std::sync::Mutex::new(()),
            runtime_cancel: None,
        }
    }

    pub fn with_cancellation(
        cancel: Option<std::sync::Arc<app_runtime_core::CancellationToken>>,
    ) -> Self {
        Self {
            runtime_cancel: cancel,
            ..Self::new()
        }
    }

    /// 开始新任务：返回代际；此前任务的 cancelled 位置位。
    pub fn start(&self) -> u64 {
        let _guard = self.commit_gate.lock().unwrap_or_else(|e| e.into_inner());
        let prev = self.generation.fetch_add(1, Ordering::SeqCst);
        self.cancelled.store(prev, Ordering::SeqCst);
        prev + 1
    }

    pub fn cancel_current(&self) {
        let _guard = self.commit_gate.lock().unwrap_or_else(|e| e.into_inner());
        self.cancelled
            .store(self.generation.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// 取消指定代际（mode/T03 复核更正）：仅当它仍是活动代际时置位。
    /// 过期代际为 no-op，不会覆盖更新代际的取消状态——取消请求因此绑定
    /// 其验证过的任务代际，消除「验证后新任务被接纳、旧取消误杀新任务」
    /// 的竞态。返回是否实际置位。
    pub fn cancel_generation(&self, generation: u64) -> bool {
        let _guard = self.commit_gate.lock().unwrap_or_else(|e| e.into_inner());
        if generation > 0 && self.generation.load(Ordering::SeqCst) == generation {
            self.cancelled.store(generation, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn commit<T, E>(
        &self,
        generation: u64,
        write: impl FnOnce() -> Result<T, E>,
    ) -> Result<Option<T>, E> {
        let _guard = self.commit_gate.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_cancelled(generation) {
            return Ok(None);
        }
        write().map(Some)
    }

    pub fn is_cancelled(&self, generation: u64) -> bool {
        self.runtime_cancel
            .as_ref()
            .is_some_and(|c| c.is_cancelled())
            || generation > 0
                && (self.generation.load(Ordering::SeqCst) != generation
                    || self.cancelled.load(Ordering::SeqCst) == generation)
    }
}

/// 判断一组来源中哪些启用（M05 `_source_policy` 语义：全部禁用 → disabled；无记录 → 默认启用）。
pub fn enabled_sources(conn: &rusqlite::Connection, ids: &[&str]) -> rusqlite::Result<Vec<String>> {
    let states = store::source_states(conn)?;
    Ok(states
        .into_iter()
        .filter(|s| ids.contains(&s.source_id.as_str()) && s.enabled)
        .map(|s| s.source_id)
        .collect())
}

/// 关联判定（方案 04 §3）：公告/基金公告 codes 来自上游明确身份 → direct；
/// 新闻按名称/关键词搜索 → candidate（不得冒充 direct）。
fn relations_for(item: &adapters::RawItem) -> Vec<(String, &'static str, String)> {
    match item.source_id {
        // 新闻按名称/代码关键词搜索：candidate 关系同时挂 keyword 身份与
        // 六位数字代码的 security 身份（审计五-1：自选证券 scope 需按 code 命中；
        // 名称关键词仍是 candidate，不冒充 direct）。
        adapters::SRC_STOCK_NEWS => item
            .codes
            .iter()
            .flat_map(|c| {
                let mut refs = vec![(
                    instrument_ref_for_keyword(c),
                    "candidate",
                    "新闻按名称/关键词搜索命中".to_string(),
                )];
                if c.len() == 6 && c.chars().all(|d| d.is_ascii_digit()) {
                    refs.push((
                        instrument_ref_for_code(c),
                        "candidate",
                        "新闻内容命中证券代码（关键词级，非公告身份）".to_string(),
                    ));
                }
                refs
            })
            .collect(),
        adapters::SRC_STOCK_ANN => item
            .codes
            .iter()
            .map(|c| {
                (
                    instrument_ref_for_code(c),
                    "direct",
                    "公告上游明确证券身份".to_string(),
                )
            })
            .collect(),
        adapters::SRC_FUND_ANN => item
            .codes
            .iter()
            .map(|c| {
                (
                    instrument_ref_for_fund(c),
                    "direct",
                    "基金公告 F10 明确基金身份".to_string(),
                )
            })
            .collect(),
        _ => vec![],
    }
}

pub fn refresh(
    registry: &TaskRegistry,
    store: &Store,
    watchlist_codes: &[String],
    now: &str,
) -> RefreshOutcome {
    let generation = registry.start();
    refresh_generation(
        registry,
        store,
        watchlist_codes,
        now,
        generation,
        true,
        true,
        7,
        0,
    )
}

pub fn refresh_generation(
    registry: &TaskRegistry,
    store: &Store,
    watchlist_codes: &[String],
    now: &str,
    generation: u64,
    include_holdings: bool,
    include_market: bool,
    days: u32,
    identity_offset: usize,
) -> RefreshOutcome {
    let deadline = Instant::now() + std::time::Duration::from_secs(20);
    let mut outcome = RefreshOutcome::default();
    outcome.window_days = days;
    let mut written_total = 0usize;

    let (mut fund_codes, holding_stocks) = if include_holdings {
        match store.with_read(collect_instruments) {
            Ok(v) => v,
            Err(_) => {
                outcome.sources.push(SourceOutcome {
                    source_id: "scope".into(),
                    status: "error".into(),
                    count: 0,
                    error_code: Some("store_error".into()),
                    latency_ms: 0,
                });
                return outcome;
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };
    let holding_refs: std::collections::HashSet<String> = fund_codes
        .iter()
        .map(|c| instrument_ref_for_fund(c))
        .chain(holding_stocks.iter().map(|c| instrument_ref_for_code(c)))
        .collect();
    let enabled = match store.with_read(|conn| enabled_sources(conn, store::SOURCE_IDS)) {
        Ok(v) => v,
        Err(_) => {
            outcome.sources.push(SourceOutcome {
                source_id: "sources".into(),
                status: "error".into(),
                count: 0,
                error_code: Some("store_error".into()),
                latency_ms: 0,
            });
            return outcome;
        }
    };
    if enabled.is_empty() {
        outcome
            .sources
            .extend(store::SOURCE_IDS.iter().map(|s| SourceOutcome {
                source_id: (*s).into(),
                status: "disabled".into(),
                count: 0,
                error_code: None,
                latency_ms: 0,
            }));
        return outcome;
    }

    // 来源按真实身份分流（审计五-1；T01 覆盖矩阵）：股票公告只接收股票/ETF 代码，
    // 基金持仓代码不套用股票公告接口；基金公告走 F10；新闻两者都作关键词。
    let mut plan: Vec<(&'static str, Vec<String>)> = Vec::new();
    if include_market && identity_offset == 0 {
        plan.push((adapters::SRC_FINANCE, vec![]));
    }
    let mut stock_codes: Vec<String> = watchlist_codes
        .iter()
        .filter_map(|c| public_stock_code(c))
        .collect();
    stock_codes.extend(holding_stocks);
    stock_codes.sort();
    stock_codes.dedup();
    outcome.total_identities = fund_codes.len() + stock_codes.len();
    (fund_codes, stock_codes, outcome.next_identity_offset) =
        scope::identity_batch(&fund_codes, &stock_codes, identity_offset);
    outcome.covered_identities = fund_codes.len() + stock_codes.len();
    if fund_codes.len() + stock_codes.len() <= 50 {
        // 方案 04 §7：单次 ≤50 个身份。
        if !stock_codes.is_empty() {
            plan.push((adapters::SRC_STOCK_ANN, stock_codes.clone()));
        }
        for code in &fund_codes {
            plan.push((adapters::SRC_FUND_ANN, vec![code.clone()]));
        }
        for code in &stock_codes {
            plan.push((adapters::SRC_STOCK_NEWS, vec![code.clone()]));
        }
        let names = if include_holdings {
            match store.with_read(|c| fund_names_for_codes(c, &fund_codes)) {
                Ok(names) => names,
                Err(_) => {
                    outcome.sources.push(SourceOutcome {
                        source_id: "scope".into(),
                        status: "error".into(),
                        count: 0,
                        error_code: Some("store_error".into()),
                        latency_ms: 0,
                    });
                    return outcome;
                }
            }
        } else {
            vec![]
        };
        for keyword in names {
            plan.push((adapters::SRC_STOCK_NEWS, vec![keyword]));
        }
    }

    let priority = match crate::settings::get(store) {
        Ok(v) => v["sourcePriority"]
            .as_str()
            .unwrap_or(adapters::SRC_STOCK_ANN)
            .to_string(),
        Err(_) => {
            outcome.sources.push(SourceOutcome {
                source_id: "settings".into(),
                status: "error".into(),
                count: 0,
                error_code: Some("configuration_invalid".into()),
                latency_ms: 0,
            });
            return outcome;
        }
    };
    plan.sort_by_key(|(source, _)| {
        if *source == adapters::SRC_FINANCE {
            0
        } else if *source == priority {
            1
        } else {
            2
        }
    });
    for (source_id, targets) in &plan {
        if registry.is_cancelled(generation) || Instant::now() >= deadline {
            outcome.cancelled = registry.is_cancelled(generation);
            outcome.sources.push(SourceOutcome {
                source_id: (*source_id).into(),
                status: "partial".into(),
                count: 0,
                error_code: Some(
                    if outcome.cancelled {
                        "cancelled"
                    } else {
                        "task_timeout"
                    }
                    .into(),
                ),
                latency_ms: 0,
            });
            break;
        }
        if !enabled.iter().any(|s| s == source_id) {
            outcome.sources.push(SourceOutcome {
                source_id: (*source_id).to_string(),
                status: "disabled".into(),
                count: 0,
                error_code: None,
                latency_ms: 0,
            });
            continue;
        }
        if written_total >= MAX_ITEMS_PER_SOURCE {
            outcome.sources.push(SourceOutcome {
                source_id: (*source_id).into(),
                status: "partial".into(),
                count: 0,
                error_code: Some("item_limit_reached".into()),
                latency_ms: 0,
            });
            break;
        }
        let started = Instant::now();
        let mut partial_error = None;
        let result = match *source_id {
            adapters::SRC_FINANCE => finance::fetch(deadline),
            adapters::SRC_STOCK_ANN => adapters::fetch_stock_announcements_until(targets, deadline),
            adapters::SRC_FUND_ANN => {
                let mut all = Vec::new();
                let mut last_err = None;
                let mut ok_count = 0usize;
                for code in targets {
                    if registry.is_cancelled(generation) {
                        outcome.cancelled = true;
                        break;
                    }
                    match adapters::fetch_fund_announcements_until(code, deadline) {
                        Ok(items) => {
                            ok_count += 1;
                            all.extend(items);
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                match (ok_count, last_err) {
                    (0, Some(e)) => Err(e),
                    (0, None) => Ok(Vec::new()),
                    (n, Some(e)) if n > 0 => {
                        // partial 语义：部分成功在来源状态记录错误，结果保留。
                        partial_error = Some(e.code().to_string());
                        Ok(all)
                    }
                    _ => Ok(all),
                }
            }
            adapters::SRC_STOCK_NEWS => {
                let mut all = Vec::new();
                let mut errors = 0usize;
                for keyword in targets {
                    if registry.is_keyword_cancelled(generation) {
                        outcome.cancelled = true;
                        break;
                    }
                    match adapters::fetch_stock_news_until(keyword, deadline) {
                        Ok(items) => all.extend(items),
                        Err(_) => errors += 1,
                    }
                }
                if all.is_empty() && errors > 0 {
                    Err(adapters::FetchError::Network(format!(
                        "{errors} 个关键词失败"
                    )))
                } else if errors > 0 {
                    partial_error = Some("network_error".to_string());
                    Ok(all)
                } else {
                    Ok(all)
                }
            }
            _ => Ok(vec![]),
        };
        let latency = started.elapsed().as_millis();
        if registry.is_cancelled(generation) {
            outcome.cancelled = true;
            break;
        }
        let truncated: Vec<adapters::RawItem> = match result {
            Ok(items) => items
                .into_iter()
                .filter(|item| {
                    item.published_at.as_ref().is_none_or(|at| {
                        normalize::window_start(now, days).is_some_and(|start| at >= &start)
                    })
                })
                .take(MAX_ITEMS_PER_SOURCE.saturating_sub(written_total))
                .collect(),
            Err(e) => {
                outcome.sources.push(SourceOutcome {
                    source_id: (*source_id).to_string(),
                    status: "error".into(),
                    count: 0,
                    error_code: Some(e.code().to_string()),
                    latency_ms: latency,
                });
                let _ = registry.commit(generation, || {
                    store.with_write(|conn| {
                        if enabled_sources(conn, &[source_id])?.is_empty() {
                            return Ok(());
                        }
                        store::record_source_result(
                            conn,
                            source_id,
                            false,
                            0,
                            latency as i64,
                            Some(e.code()),
                            now,
                        )
                    })
                });
                continue;
            }
        };
        let count = truncated.len();
        let status = if partial_error.is_some() {
            "partial"
        } else if count == 0 {
            "ok_empty"
        } else {
            "ok_with_items"
        };
        // 审计五-1：存储失败必须反馈（partial + store_error），不静默报成功。
        let write_result = registry.commit(generation, || {
            store.with_write::<(bool, usize), rusqlite::Error>(|conn| {
                if enabled_sources(conn, &[source_id])?.is_empty() {
                    return Ok((false, 0));
                }
                store::record_source_result(
                    conn,
                    source_id,
                    true,
                    count as i64,
                    latency as i64,
                    partial_error.as_deref(),
                    now,
                )?;
                let n = store::upsert_items(conn, &truncated, &relations_for, now)?;
                store::prune(conn, 10_000)?;
                // R02（方案 05 §8）：直接关联持仓的重要公告 → 复核提醒。
                // 幂等：(asset_ref, source_ref) 唯一，重采/双窗口/重开不重复；
                // 内容更正（contentVersion 升级）且原提醒已解除时才重新提醒。
                for item in &truncated {
                    if item.importance < 2 {
                        continue;
                    }
                    for (reference, relation, _) in relations_for(item) {
                        if relation != "direct" || !holding_refs.contains(&reference) {
                            continue;
                        }
                        let source_ref = format!("{}:{}", item.source_id, item.external_id);
                        let version = content_version_of(conn, item.source_id, &item.external_id);
                        upsert_review_alert(
                            conn,
                            &reference,
                            &source_ref,
                            version,
                            &item.title,
                            item.event_types.first().copied().unwrap_or("notice"),
                            now,
                        )?;
                    }
                }
                Ok((true, n))
            })
        });
        if let Err(_e) = write_result {
            outcome.sources.push(SourceOutcome {
                source_id: (*source_id).to_string(),
                status: "partial".into(),
                count: 0,
                error_code: Some("store_error".into()),
                latency_ms: latency,
            });
            continue;
        }
        if matches!(write_result, Ok(None)) {
            outcome.cancelled = true;
            break;
        }
        if matches!(write_result, Ok(Some((false, _)))) {
            outcome.sources.push(SourceOutcome {
                source_id: (*source_id).into(),
                status: "disabled".into(),
                count: 0,
                error_code: None,
                latency_ms: latency,
            });
            continue;
        }
        written_total += count;
        outcome.sources.push(SourceOutcome {
            source_id: (*source_id).to_string(),
            status: status.into(),
            count,
            error_code: partial_error,
            latency_ms: latency,
        });
    }
    outcome.written = written_total;
    outcome
}

impl TaskRegistry {
    fn is_keyword_cancelled(&self, generation: u64) -> bool {
        self.is_cancelled(generation)
    }
}

/// 当前条目的内容版本（upsert 之后读取，用于提醒幂等键）。
fn content_version_of(conn: &rusqlite::Connection, source_id: &str, external_id: &str) -> i64 {
    conn.query_row(
        "SELECT content_version FROM information_items WHERE source_id=?1 AND external_id=?2",
        rusqlite::params![source_id, external_id],
        |r| r.get(0),
    )
    .unwrap_or(1)
}

/// 复核提醒幂等 upsert（可独立测试）：未解除期间重采不重复；
/// 已解除的提醒仅在内容实质变化（contentVersion 升级）时重新打开。
pub fn upsert_review_alert(
    conn: &rusqlite::Connection,
    asset_ref: &str,
    source_ref: &str,
    content_version: i64,
    title: &str,
    event_type: &str,
    now: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO information_alerts(asset_ref, source_ref, content_version, title, event_type, first_triggered_at, updated_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?6) \
         ON CONFLICT(asset_ref, source_ref) DO UPDATE SET \
         content_version=?3, title=?4, event_type=?5, \
         status=CASE WHEN information_alerts.status='resolved' AND information_alerts.content_version < ?3 \
                     THEN 'open' ELSE information_alerts.status END, \
         notify_result=CASE WHEN information_alerts.status='resolved' AND information_alerts.content_version < ?3 THEN 'pending' ELSE information_alerts.notify_result END, \
         updated_at=?6",
        rusqlite::params![asset_ref, source_ref, content_version, title, event_type, now],
    )?;
    Ok(())
}

/// 列表缓存键与 TTL（方案 04 §7：15 分钟；键含 scope、参数与版本）。
pub fn list_cache_key(scope: &str, refs: &[String], page: usize) -> String {
    format!("information:{scope}:{}:{page}", refs.join(","))
}

/// 进程内列表缓存（方案 04 §7：15 分钟 TTL；键含 scope、refs 与页码）。
/// 读取时同时校验 Store 的 data_revision——任何写事务（刷新、flags、archive）
/// 都会推进 revision，缓存随之自然失效，不需要手动清理。
pub struct ListCache {
    entries: std::sync::Mutex<HashMap<String, (std::time::Instant, String, Vec<u8>)>>,
}

impl ListCache {
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str, revision: &str) -> Option<Vec<u8>> {
        let guard = self.entries.lock().ok()?;
        let (at, rev, data) = guard.get(key)?;
        if at.elapsed().as_millis() as u64 > LIST_TTL_MS || rev != revision {
            return None;
        }
        Some(data.clone())
    }

    pub fn set(&self, key: &str, revision: &str, data: Vec<u8>) {
        let mut guard = match self.entries.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        // 容量上限：超限整体清空（进程内短缓存，重建成本低）。
        if guard.len() >= 128 {
            guard.clear();
        }
        guard.insert(
            key.to_string(),
            (std::time::Instant::now(), revision.to_string(), data),
        );
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
