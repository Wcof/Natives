//! 行情上游短 TTL 缓存：只缓存真实上游响应，绝不伪造数据。
//!
//! 目的有二：1) 自选表/跑马灯/盘口 3~6s 一轮的高频轮询不必逐次打到上游，
//! 避免上游限流与排队拖慢详情页；2) 重复切换同一股票/周期时整页 K 线
//! 秒开（日 K 需串行拉 4 页上游，首次 ~0.8s）。TTL 内的数据仍是上游
//! 实测值：行情类 3~5s、K 线 30s、板块 60s，均短于人工可感知的过期阈。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

type Entry = (Instant, Duration, Result<String, (u16, String)>);

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 失败结果短缓存：上游出错时防止雪崩式重试，1s 后自动放行重试。
const ERROR_TTL: Duration = Duration::from_secs(1);
/// 条目上限：超出即整体清扫过期项，防止键空间随标的切换无限增长。
const MAX_ENTRIES: usize = 256;

/// 带 TTL 的请求级缓存：命中返回缓存副本，未命中执行 `fetch` 并落缓存。
pub(crate) fn cached(
    key: impl Into<String>,
    ttl: Duration,
    fetch: impl FnOnce() -> Result<String, (u16, String)>,
) -> Result<String, (u16, String)> {
    let key = key.into();
    if let Ok(cache) = CACHE.lock() {
        if let Some((at, entry_ttl, value)) = cache.get(&key) {
            if at.elapsed() < *entry_ttl {
                return value.clone();
            }
        }
    }

    let value = fetch();
    let entry_ttl = if value.is_ok() { ttl } else { ERROR_TTL };
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() >= MAX_ENTRIES {
            cache.retain(|_, (at, entry_ttl, _)| at.elapsed() < *entry_ttl);
        }
        cache.insert(key, (Instant::now(), entry_ttl, value.clone()));
    }
    value
}
