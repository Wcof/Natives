//! API Key Pool —— P0-A Spike（ADR-0020 P0 Gate #5）
//!
//! 验证 API Key 多 Key 池的公平轮转 / 优先级 / 冷却 / 失败切换与可观测语义。
//! 纯内存、无 SQLite 依赖，运行态 cooldown/health 为 ephemeral（ADR-0020：
//! "Engine 的运行态 cooldown/health 可是 ephemeral"）。生产接入时由 Host
//! ProxyEngine 选择持久化后端，本模块保持纯逻辑可独立验证。
//!
//! 语义：
//! - `select` 优先挑选最高优先级且未冷却/未禁用的 Key；
//! - 同优先级内做 round-robin 公平轮转；
//! - 失败计数达到阈值进入冷却；冷却到期自动恢复；
//! - 禁用 Key 永不被选中；
//! - 首选失败后可在同一次选择内 failover 到次选（不重放首 delta 由上层保证）。

use std::time::{Duration, Instant};

/// Key 的可用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    /// 可被选择。
    Active,
    /// 被管理员禁用，永不被选择。
    Disabled,
    /// 进入冷却，直到 cooldown_until。
    CoolingDown,
}

/// 池中单个 Key 的运行态视图。
#[derive(Debug, Clone)]
pub struct PoolKey {
    pub id: String,
    pub priority: u32,
    pub status: KeyStatus,
    /// 冷却截止时刻；仅 status == CoolingDown 时有意义。
    pub cooldown_until: Option<Instant>,
    /// 连续失败次数（冷却/恢复的输入）。
    pub consecutive_failures: u32,
    /// 被选中次数（可观测）。
    pub selected_count: u64,
    /// 失败总数（可观测）。
    pub failure_count: u64,
    /// 最近一次选中时间。
    pub last_selected_at: Option<Instant>,
}

impl PoolKey {
    fn available(&self, now: Instant) -> bool {
        match self.status {
            KeyStatus::Disabled => false,
            KeyStatus::Active => true,
            KeyStatus::CoolingDown => match self.cooldown_until {
                Some(until) => now >= until,
                None => true,
            },
        }
    }
}

/// API Key Pool 选择策略（P0-A Spike）。
#[derive(Debug)]
pub struct KeyPool {
    keys: Vec<PoolKey>,
    /// 同优先级 round-robin 游标。
    rr_cursor: usize,
    /// 进入冷却所需的连续失败次数。
    pub failure_threshold: u32,
    /// 冷却时长。
    pub cooldown: Duration,
    // ---- 可观测计数器 ----
    pub total_selections: u64,
    pub total_failovers: u64,
    pub total_rejections: u64,
}

/// 一次选择的结果。
#[derive(Debug)]
pub enum Selection {
    /// 命中的 Key（含是否经过 failover）。
    Key { id: String, failover: bool },
    /// 池中没有任何可用 Key。
    Empty,
}

impl KeyPool {
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            keys: Vec::new(),
            rr_cursor: 0,
            failure_threshold: failure_threshold.max(1),
            cooldown,
            total_selections: 0,
            total_failovers: 0,
            total_rejections: 0,
        }
    }

    pub fn add_key(&mut self, id: impl Into<String>, priority: u32) {
        self.keys.push(PoolKey {
            id: id.into(),
            priority,
            status: KeyStatus::Active,
            cooldown_until: None,
            consecutive_failures: 0,
            selected_count: 0,
            failure_count: 0,
            last_selected_at: None,
        });
    }

    pub fn set_disabled(&mut self, id: &str, disabled: bool) {
        if let Some(key) = self.keys.iter_mut().find(|k| k.id == id) {
            if disabled {
                key.status = KeyStatus::Disabled;
            } else if key.status == KeyStatus::Disabled {
                key.status = KeyStatus::Active;
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&PoolKey> {
        self.keys.iter().find(|k| k.id == id)
    }

    /// 选择下一个可用 Key：最高优先级优先，同优先级 round-robin。
    pub fn select(&mut self) -> Selection {
        let now = Instant::now();
        let mut best_priority = None;
        for key in &self.keys {
            if key.available(now) {
                match best_priority {
                    None => best_priority = Some(key.priority),
                    Some(p) if key.priority < p => best_priority = Some(key.priority),
                    _ => {}
                }
            }
        }
        let Some(priority) = best_priority else {
            self.total_rejections += 1;
            return Selection::Empty;
        };

        // 从游标起找第一个该优先级且可用的 Key（环形扫描，保证公平）。
        let n = self.keys.len();
        for offset in 0..n {
            let idx = (self.rr_cursor + offset) % n;
            let key = &self.keys[idx];
            if key.priority == priority && key.available(now) {
                self.rr_cursor = (idx + 1) % n;
                let key = &mut self.keys[idx];
                key.selected_count += 1;
                key.last_selected_at = Some(now);
                if key.status == KeyStatus::CoolingDown {
                    key.status = KeyStatus::Active;
                    key.consecutive_failures = 0;
                }
                self.total_selections += 1;
                return Selection::Key {
                    id: key.id.clone(),
                    failover: false,
                };
            }
        }
        self.total_rejections += 1;
        Selection::Empty
    }

    /// 首选失败：记录失败并（必要时）切换下一个可用 Key（failover）。
    /// 返回 None 表示池内已无其它可用 Key，调用方应返回结构化 Error。
    pub fn record_failure_and_failover(&mut self, failed_id: &str) -> Option<String> {
        let now = Instant::now();
        if let Some(key) = self.keys.iter_mut().find(|k| k.id == failed_id) {
            key.failure_count += 1;
            key.consecutive_failures += 1;
            if key.consecutive_failures >= self.failure_threshold {
                key.status = KeyStatus::CoolingDown;
                key.cooldown_until = Some(now + self.cooldown);
            }
        }
        self.total_failovers += 1;

        // failover 选择：跳过失败 Key，取其它可用 Key（优先级最优、同优先级 RR）。
        let mut best_priority = None;
        for key in &self.keys {
            if key.id != failed_id && key.available(now) {
                match best_priority {
                    None => best_priority = Some(key.priority),
                    Some(p) if key.priority < p => best_priority = Some(key.priority),
                    _ => {}
                }
            }
        }
        let Some(priority) = best_priority else {
            return None;
        };
        let n = self.keys.len();
        for offset in 0..n {
            let idx = (self.rr_cursor + offset) % n;
            let key = &self.keys[idx];
            if key.id != failed_id && key.priority == priority && key.available(now) {
                self.rr_cursor = (idx + 1) % n;
                let key = &mut self.keys[idx];
                key.selected_count += 1;
                key.last_selected_at = Some(now);
                if key.status == KeyStatus::CoolingDown {
                    key.status = KeyStatus::Active;
                    key.consecutive_failures = 0;
                }
                self.total_selections += 1;
                return Some(key.id.clone());
            }
        }
        None
    }

    /// 一次成功调用后重置连续失败计数。
    pub fn record_success(&mut self, id: &str) {
        if let Some(key) = self.keys.iter_mut().find(|k| k.id == id) {
            key.consecutive_failures = 0;
            if key.status == KeyStatus::CoolingDown {
                key.status = KeyStatus::Active;
                key.cooldown_until = None;
            }
        }
    }

    /// 可观测快照（供状态/健康展示，不暴露 Secret）。
    pub fn snapshot(&self) -> Vec<PoolKeySnapshot> {
        let now = Instant::now();
        self.keys
            .iter()
            .map(|k| PoolKeySnapshot {
                id: k.id.clone(),
                priority: k.priority,
                status: k.status,
                cooling_down: k.status == KeyStatus::CoolingDown
                    && k.cooldown_until.map(|u| now < u).unwrap_or(false),
                consecutive_failures: k.consecutive_failures,
                selected_count: k.selected_count,
                failure_count: k.failure_count,
            })
            .collect()
    }

    /// 清理已完全无用的 Key（测试辅助，不面向生产）。
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.keys.clear();
        self.rr_cursor = 0;
    }
}

/// 可观测快照条目。
#[derive(Debug, Clone)]
pub struct PoolKeySnapshot {
    pub id: String,
    pub priority: u32,
    pub status: KeyStatus,
    pub cooling_down: bool,
    pub consecutive_failures: u32,
    pub selected_count: u64,
    pub failure_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> KeyPool {
        KeyPool::new(3, Duration::from_millis(50))
    }

    #[test]
    fn select_prefers_higher_priority() {
        let mut p = pool();
        p.add_key("low", 10);
        p.add_key("high", 1);
        assert!(matches!(p.select(), Selection::Key { id, .. } if id == "high"));
    }

    #[test]
    fn same_priority_round_robins_fairly() {
        let mut p = pool();
        p.add_key("a", 1);
        p.add_key("b", 1);
        let mut picks = Vec::new();
        for _ in 0..6 {
            if let Selection::Key { id, .. } = p.select() {
                picks.push(id);
            }
        }
        // a,b,a,b,a,b
        assert_eq!(picks, vec!["a", "b", "a", "b", "a", "b"]);
    }

    #[test]
    fn disabled_key_never_selected() {
        let mut p = pool();
        p.add_key("a", 1);
        p.add_key("b", 1);
        p.set_disabled("a", true);
        for _ in 0..4 {
            assert!(matches!(p.select(), Selection::Key { id, .. } if id == "b"));
        }
    }

    #[test]
    fn empty_pool_rejects() {
        let mut p = pool();
        assert!(matches!(p.select(), Selection::Empty));
        assert_eq!(p.total_rejections, 1);
    }

    #[test]
    fn failures_enter_cooldown_and_recover_after_duration() {
        let mut p = pool();
        p.add_key("a", 1);
        p.add_key("b", 1);
        // 三次失败触发冷却
        for _ in 0..3 {
            assert!(p.record_failure_and_failover("a").is_some());
        }
        assert_eq!(p.get("a").unwrap().status, KeyStatus::CoolingDown);
        // 冷却期间只剩 b
        for _ in 0..3 {
            assert!(matches!(p.select(), Selection::Key { id, .. } if id == "b"));
        }
        // 冷却到期自动恢复
        std::thread::sleep(Duration::from_millis(60));
        assert!(p.snapshot().iter().any(|k| k.id == "a" && !k.cooling_down));
        assert!(matches!(p.select(), Selection::Key { id, .. } if id == "a"));
    }

    #[test]
    fn failover_switches_to_next_available() {
        let mut p = pool();
        p.add_key("a", 1);
        p.add_key("b", 2);
        let next = p.record_failure_and_failover("a");
        assert_eq!(next.as_deref(), Some("b"));
        assert_eq!(p.total_failovers, 1);
    }

    #[test]
    fn success_resets_failure_counter() {
        let mut p = pool();
        p.add_key("a", 1);
        p.record_failure_and_failover("a");
        p.record_failure_and_failover("a");
        p.record_success("a");
        assert_eq!(p.get("a").unwrap().consecutive_failures, 0);
        assert_eq!(p.get("a").unwrap().status, KeyStatus::Active);
    }

    #[test]
    fn observability_counts_are_monotonic() {
        let mut p = pool();
        p.add_key("a", 1);
        p.add_key("b", 1);
        p.select();
        p.select();
        p.record_failure_and_failover("a");
        assert_eq!(p.total_selections, 3);
        assert_eq!(p.total_failovers, 1);
        let snap = p.snapshot();
        assert_eq!(snap.iter().map(|k| k.selected_count).sum::<u64>(), 3);
        assert!(snap.iter().any(|k| k.id == "a" && k.failure_count == 1));
    }

    #[test]
    fn concurrent_selection_never_duplicates_a_single_key() {
        use std::sync::{Arc, Mutex};
        let pool = Arc::new(Mutex::new(pool()));
        pool.lock().unwrap().add_key("a", 1);
        pool.lock().unwrap().add_key("b", 1);
        let mut handles = Vec::new();
        for _ in 0..8 {
            let pool = Arc::clone(&pool);
            handles.push(std::thread::spawn(move || {
                let mut p = pool.lock().unwrap();
                let _ = p.select();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let p = pool.lock().unwrap();
        // 8 次选择落在 2 个 Key 上，总数守恒（无重复领取同一 id 的竞争错误）
        assert_eq!(p.total_selections, 8);
        let sum: u64 = p.snapshot().iter().map(|k| k.selected_count).sum();
        assert_eq!(sum, 8);
    }

    #[test]
    fn cooldown_only_cooling_keys_not_failed_once() {
        let mut p = KeyPool::new(5, Duration::from_secs(3600));
        p.add_key("a", 1);
        p.add_key("b", 1);
        // 只有一次失败：不触发冷却
        p.record_failure_and_failover("a");
        assert_eq!(p.get("a").unwrap().status, KeyStatus::Active);
        assert_eq!(p.get("a").unwrap().consecutive_failures, 1);
    }
}
