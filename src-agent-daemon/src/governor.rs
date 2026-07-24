use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

pub const SETTINGS_KEY: &str = "engine:provider_rate_limit:v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRateLimitSettings {
    pub enabled: bool,
    pub requests_per_minute: u32,
}

impl Default for EngineRateLimitSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 10,
        }
    }
}

impl EngineRateLimitSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=600).contains(&self.requests_per_minute) {
            return Err("requests_per_minute must be an integer between 1 and 600".into());
        }
        Ok(())
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(60_000 / u64::from(self.requests_per_minute))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRateLimitSnapshot {
    pub settings: EngineRateLimitSettings,
    pub effective_interval_ms: u64,
    pub queued_requests: u32,
    pub cooling_routes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RouteKey {
    provider_id: String,
    key_id: String,
}

#[derive(Default)]
struct RouteState {
    last_sent: Option<Instant>,
    cooling_until: Option<Instant>,
    queue: VecDeque<u64>,
}

struct GovernorState {
    settings: EngineRateLimitSettings,
    routes: HashMap<RouteKey, RouteState>,
    next_ticket: u64,
}

pub struct ProviderRequestGovernor {
    state: Mutex<GovernorState>,
    notify: Notify,
}

impl ProviderRequestGovernor {
    pub fn new(settings: EngineRateLimitSettings) -> Self {
        Self {
            state: Mutex::new(GovernorState {
                settings,
                routes: HashMap::new(),
                next_ticket: 0,
            }),
            notify: Notify::new(),
        }
    }

    pub async fn acquire(
        &self,
        provider_id: &str,
        key_id: &str,
        cancel: CancellationToken,
    ) -> Result<(), String> {
        let key = RouteKey {
            provider_id: provider_id.to_owned(),
            key_id: key_id.to_owned(),
        };
        let ticket = {
            let mut state = self.state.lock().await;
            state.next_ticket = state.next_ticket.wrapping_add(1);
            let ticket = state.next_ticket;
            state
                .routes
                .entry(key.clone())
                .or_default()
                .queue
                .push_back(ticket);
            ticket
        };

        loop {
            let notified = self.notify.notified();
            let wait = {
                let mut state = self.state.lock().await;
                let settings = state.settings.clone();
                let route = state
                    .routes
                    .get_mut(&key)
                    .expect("route inserted with ticket");
                if !settings.enabled {
                    remove_ticket(route, ticket);
                    None
                } else if route.queue.front() == Some(&ticket) {
                    let now = Instant::now();
                    let due = route
                        .last_sent
                        .map(|last| last + settings.interval())
                        .unwrap_or(now);
                    let allowed_at = route.cooling_until.map(|cool| cool.max(due)).unwrap_or(due);
                    if allowed_at <= now {
                        route.queue.pop_front();
                        route.last_sent = Some(now);
                        None
                    } else {
                        Some(allowed_at.saturating_duration_since(now))
                    }
                } else {
                    Some(Duration::ZERO)
                }
            };
            if wait.is_none() {
                self.notify.notify_waiters();
                return Ok(());
            }
            let wait = wait.expect("checked above");
            tokio::select! {
                _ = cancel.cancelled() => {
                    self.remove_ticket(&key, ticket).await;
                    return Err("cancelled".into());
                }
                _ = notified => {}
                _ = tokio::time::sleep(wait), if !wait.is_zero() => {}
            }
        }
    }

    pub async fn record_rate_limit(
        &self,
        provider_id: &str,
        key_id: &str,
        retry_after_ms: Option<u64>,
    ) {
        let key = RouteKey {
            provider_id: provider_id.to_owned(),
            key_id: key_id.to_owned(),
        };
        let cooldown_ms = retry_after_ms.unwrap_or_else(|| 60_000 + rand::random::<u64>() % 2_001);
        let mut state = self.state.lock().await;
        let route = state.routes.entry(key).or_default();
        let candidate = Instant::now() + Duration::from_millis(cooldown_ms);
        route.cooling_until = Some(
            route
                .cooling_until
                .map(|existing| existing.max(candidate))
                .unwrap_or(candidate),
        );
        drop(state);
        self.notify.notify_waiters();
    }

    pub async fn snapshot(&self) -> EngineRateLimitSnapshot {
        let state = self.state.lock().await;
        let now = Instant::now();
        EngineRateLimitSnapshot {
            settings: state.settings.clone(),
            effective_interval_ms: state.settings.interval().as_millis() as u64,
            queued_requests: state
                .routes
                .values()
                .map(|route| route.queue.len() as u32)
                .sum(),
            cooling_routes: state
                .routes
                .values()
                .filter(|route| route.cooling_until.is_some_and(|until| until > now))
                .count() as u32,
        }
    }

    pub async fn update_settings(&self, settings: EngineRateLimitSettings) {
        let mut state = self.state.lock().await;
        state.settings = settings;
        drop(state);
        self.notify.notify_waiters();
    }

    async fn remove_ticket(&self, key: &RouteKey, ticket: u64) {
        let mut state = self.state.lock().await;
        if let Some(route) = state.routes.get_mut(key) {
            remove_ticket(route, ticket);
        }
        drop(state);
        self.notify.notify_waiters();
    }
}

fn remove_ticket(route: &mut RouteState, ticket: u64) {
    if let Some(pos) = route.queue.iter().position(|queued| *queued == ticket) {
        route.queue.remove(pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, timeout};

    fn settings(rpm: u32) -> EngineRateLimitSettings {
        EngineRateLimitSettings {
            enabled: true,
            requests_per_minute: rpm,
        }
    }

    #[tokio::test]
    async fn different_keys_do_not_block_each_other() {
        let governor = ProviderRequestGovernor::new(settings(600));
        let cancel = CancellationToken::new();
        let (left, right) = tokio::join!(
            governor.acquire("provider", "key-a", cancel.clone()),
            governor.acquire("provider", "key-b", cancel),
        );
        left.unwrap();
        right.unwrap();
        assert_eq!(governor.snapshot().await.queued_requests, 0);
    }

    #[tokio::test]
    async fn cancellation_removes_a_waiting_ticket() {
        let governor = std::sync::Arc::new(ProviderRequestGovernor::new(settings(600)));
        governor
            .acquire("provider", "key", CancellationToken::new())
            .await
            .unwrap();
        let cancelled = CancellationToken::new();
        let waiting = {
            let governor = governor.clone();
            let cancelled = cancelled.clone();
            tokio::spawn(async move { governor.acquire("provider", "key", cancelled).await })
        };
        tokio::task::yield_now().await;
        cancelled.cancel();
        assert!(waiting.await.unwrap().is_err());
        assert_eq!(governor.snapshot().await.queued_requests, 0);
    }

    #[tokio::test]
    async fn cooldown_blocks_the_route() {
        let governor = ProviderRequestGovernor::new(settings(600));
        governor
            .record_rate_limit("provider", "key", Some(40))
            .await;
        let governor = std::sync::Arc::new(governor);
        let blocked_cancel = CancellationToken::new();
        let blocked_task = {
            let governor = governor.clone();
            let cancel = blocked_cancel.clone();
            tokio::spawn(async move { governor.acquire("provider", "key", cancel).await })
        };
        let blocked = timeout(
            Duration::from_millis(20),
            blocked_task,
        )
        .await;
        assert!(blocked.is_err());
        // A timed-out waiter must explicitly cancel so its FIFO ticket is removed.
        blocked_cancel.cancel();
        sleep(Duration::from_millis(25)).await;
        governor
            .acquire("provider", "key", CancellationToken::new())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn disabled_settings_bypass_the_queue() {
        let governor = ProviderRequestGovernor::new(EngineRateLimitSettings {
            enabled: false,
            requests_per_minute: 10,
        });
        governor
            .acquire("provider", "key", CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(governor.snapshot().await.queued_requests, 0);
    }
}
