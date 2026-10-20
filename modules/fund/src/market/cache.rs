//! 行情上游短 TTL 缓存：只缓存真实上游响应，绝不伪造数据。
//!
//! 目的有二：1) 自选表/跑马灯/盘口 3~6s 一轮的高频轮询不必逐次打到上游，
//! 避免上游限流与排队拖慢详情页；2) 重复切换同一股票/周期时整页 K 线
//! 秒开（日 K 需串行拉 4 页上游，首次 ~0.8s）。TTL 内的数据仍是上游
//! 实测值：行情类 3~5s、K 线 30s、板块 60s，均短于人工可感知的过期阈。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

type Entry = (Instant, Duration, Result<String, (u16, String)>);

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static FLIGHTS: LazyLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 失败结果短缓存：上游出错时防止雪崩式重试，1s 后自动放行重试。
const ERROR_TTL: Duration = Duration::from_secs(1);
/// 条目上限：超出即整体清扫过期项，防止键空间随标的切换无限增长。
const MAX_ENTRIES: usize = 256;

pub(crate) fn read_success(key: &str) -> Option<String> {
    let cache = CACHE.lock().ok()?;
    let (at, ttl, value) = cache.get(key)?;
    if at.elapsed() >= *ttl {
        return None;
    }
    value.as_ref().ok().cloned()
}

/// 带 TTL 的请求级缓存：命中返回缓存副本，未命中执行 `fetch` 并落缓存。
pub(crate) fn cached(
    key: impl Into<String>,
    ttl: Duration,
    fetch: impl FnOnce() -> Result<String, (u16, String)>,
) -> Result<String, (u16, String)> {
    let key = key.into();
    let gate = {
        let mut flights = FLIGHTS
            .lock()
            .map_err(|_| (500, "cache lock unavailable".into()))?;
        flights.retain(|_, gate| gate.strong_count() > 0);
        match flights.get(&key).and_then(Weak::upgrade) {
            Some(gate) => gate,
            None => {
                if flights.len() >= MAX_ENTRIES {
                    return Err((503, "public query capacity reached".into()));
                }
                let gate = Arc::new(Mutex::new(()));
                flights.insert(key.clone(), Arc::downgrade(&gate));
                gate
            }
        }
    };
    // Only matching requests wait together; no global cache lock covers network I/O.
    let _flight = if crate::network::remaining().is_none() {
        gate.lock()
            .map_err(|_| (500, "public query interrupted".into()))?
    } else {
        loop {
            match gate.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err((500, "public query interrupted".into()))
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if crate::network::remaining().is_some_and(|remaining| remaining.is_zero()) {
                        return Err((504, "public query deadline exceeded".into()));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    };
    if let Ok(cache) = CACHE.lock() {
        if let Some((at, entry_ttl, value)) = cache.get(&key) {
            if at.elapsed() < *entry_ttl {
                return value.clone();
            }
        }
    }

    let value = fetch();
    let entry_ttl = if value.is_ok() { ttl } else { ERROR_TTL };
    put(key, entry_ttl, value.clone());
    value
}

pub(crate) fn put(key: String, ttl: Duration, value: Result<String, (u16, String)>) {
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() >= MAX_ENTRIES {
            cache.retain(|_, (at, entry_ttl, _)| at.elapsed() < *entry_ttl);
            if cache.len() >= MAX_ENTRIES && !cache.contains_key(&key) {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, (at, _, _))| *at)
                    .map(|(key, _)| key.clone())
                {
                    cache.remove(&oldest);
                }
            }
        }
        cache.insert(key, (Instant::now(), ttl, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn waiting_for_shared_fetch_obeys_callers_total_deadline() {
        let key = "single-flight-budget";
        let (started, ready) = std::sync::mpsc::channel();
        let (release, finish) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            cached(key, Duration::from_secs(30), || {
                started.send(()).unwrap();
                finish.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok("response".into())
            })
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let _budget = crate::network::limit_duration(Duration::from_millis(5));
        let result = cached(key, Duration::from_secs(30), || panic!("duplicate fetch"));
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), "response");
        assert_eq!(result.unwrap_err().0, 504);
    }

    #[test]
    fn simultaneous_cache_misses_share_success_and_failure() {
        for failed in [false, true] {
            let key = format!("single-flight-test-{failed}");
            let calls = AtomicUsize::new(0);
            let ready = Arc::new(Barrier::new(5));
            std::thread::scope(|scope| {
                let handles = (0..5)
                    .map(|_| {
                        let ready = ready.clone();
                        let key = &key;
                        let calls = &calls;
                        scope.spawn(move || {
                            ready.wait();
                            cached(key.clone(), Duration::from_secs(30), || {
                                calls.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(30));
                                if failed {
                                    Err((502, "upstream unavailable".into()))
                                } else {
                                    Ok("one shared response".into())
                                }
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                for handle in handles {
                    assert_eq!(handle.join().unwrap().is_err(), failed);
                }
            });
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
}
