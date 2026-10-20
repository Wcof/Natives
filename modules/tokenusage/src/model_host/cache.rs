//! A worker-owned bounded query cache; identical requests share one in-flight fetch.
use super::ModelQueryError;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

type Result = std::result::Result<String, ModelQueryError>;
#[derive(Debug, Default)]
struct Query {
    running: bool,
    value: Option<(Instant, Result)>,
}
#[derive(Debug, Default)]
struct Slot {
    state: Mutex<Query>,
    ready: Condvar,
}
#[derive(Debug, Default)]
pub(super) struct QueryCache {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}
impl QueryCache {
    pub fn invalidate(&self) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.clear();
        }
    }
    pub fn get(&self, key: &str, fetch: impl FnOnce() -> Result) -> Result {
        let slot = {
            let mut slots = self
                .slots
                .lock()
                .map_err(|_| ModelQueryError::Unavailable("query cache unavailable".into()))?;
            if !slots.contains_key(key) && slots.len() >= 8 {
                let oldest = slots
                    .iter()
                    .filter_map(|(key, slot)| {
                        slot.state
                            .try_lock()
                            .ok()
                            .filter(|state| !state.running)
                            .map(|state| (key.clone(), state.value.as_ref().map(|v| v.0)))
                    })
                    .min_by_key(|(_, at)| *at)
                    .map(|(key, _)| key);
                if let Some(key) = oldest {
                    slots.remove(&key);
                } else {
                    return Err(ModelQueryError::Unavailable(
                        "query concurrency limit".into(),
                    ));
                }
            }
            slots.entry(key.into()).or_default().clone()
        };
        let mut state = slot
            .state
            .lock()
            .map_err(|_| ModelQueryError::Unavailable("query cache unavailable".into()))?;
        while state.running {
            let (next, timeout) = slot
                .ready
                .wait_timeout(state, Duration::from_secs(10))
                .map_err(|_| ModelQueryError::Unavailable("query unavailable".into()))?;
            state = next;
            if timeout.timed_out() {
                return Err(ModelQueryError::Unavailable("query timeout".into()));
            }
        }
        if let Some((at, value)) = &state.value {
            if at.elapsed() < Duration::from_secs(if value.is_ok() { 5 } else { 1 }) {
                return value.clone();
            }
        }
        state.running = true;
        drop(state);
        let value = fetch();
        if let Ok(mut state) = slot.state.lock() {
            state.running = false;
            state.value = Some((Instant::now(), value.clone()));
            slot.ready.notify_all();
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn twenty_components_share_one_query_and_invalidation_refetches() {
        let cache = QueryCache::default();
        let count = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..20 {
                scope.spawn(|| {
                    assert_eq!(
                        cache
                            .get("same", || {
                                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(15));
                                Ok("real response".into())
                            })
                            .unwrap(),
                        "real response"
                    );
                });
            }
        });
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
        cache.invalidate();
        cache
            .get("same", || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok("updated".into())
            })
            .unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        for index in 0..32 {
            cache
                .get(&index.to_string(), || Ok("value".into()))
                .unwrap();
        }
        assert!(cache.slots.lock().unwrap().len() <= 8);
    }
}
