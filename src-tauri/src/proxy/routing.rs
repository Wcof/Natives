//! 统一路由解析与凭证池调度（RTE-001..003 / plan3 02-target-architecture §6）。
//!
//! 整合 API Key 与 OAuth Credential，统一优先级、健康度、并发、冷却与故障转移。

use rusqlite::Connection as DbConn;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::model::{CredentialSelector, PoolPolicy, RouteTarget};
use super::store;
use crate::ai::model::{Connection, Credential, CredentialStatus};
use crate::ai::store as ai_store;
use crate::secrets::store::SecretRef;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct ResolvedRouteTarget {
    pub connection: Connection,
    pub upstream_model: String,
    pub credential: Credential,
    pub secret_ref: SecretRef,
}

#[derive(Debug, Clone)]
pub struct RuntimeCredentialState {
    pub in_flight: usize,
    pub consecutive_failures: u32,
    pub total_calls: u64,
    pub success_calls: u64,
    pub failed_calls: u64,
    pub cooling_until: Option<Instant>,
    pub last_selected_at: Option<Instant>,
    pub last_error: Option<String>,
}

impl Default for RuntimeCredentialState {
    fn default() -> Self {
        Self {
            in_flight: 0,
            consecutive_failures: 0,
            total_calls: 0,
            success_calls: 0,
            failed_calls: 0,
            cooling_until: None,
            last_selected_at: None,
            last_error: None,
        }
    }
}

pub struct RouteResolver {
    states: Mutex<HashMap<String, RuntimeCredentialState>>,
    rr_cursors: Mutex<HashMap<String, usize>>,
    failure_threshold: u32,
    cooldown_duration: Duration,
}

impl Default for RouteResolver {
    fn default() -> Self {
        Self::new(3, Duration::from_secs(30))
    }
}

impl RouteResolver {
    pub fn new(failure_threshold: u32, cooldown_duration: Duration) -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
            rr_cursors: Mutex::new(HashMap::new()),
            failure_threshold: failure_threshold.max(1),
            cooldown_duration,
        }
    }

    /// 解析指定本地模型名，选择最优可用上游目标与凭证
    pub fn resolve_route_target(
        &self,
        conn: &DbConn,
        model_name: &str,
    ) -> Result<ResolvedRouteTarget> {
        let route = store::get_route_by_model(conn, model_name)?.ok_or_else(|| {
            Error::NotFound(format!(
                "No active route configured for model '{model_name}'"
            ))
        })?;

        if !route.enabled || route.targets.is_empty() {
            return Err(Error::NotFound(format!(
                "Route for '{model_name}' is disabled or has no targets"
            )));
        }

        // 按 target position / priority 依次尝试
        let mut last_rejection_reason = "No target available".to_string();

        for target in &route.targets {
            if !target.enabled {
                continue;
            }

            let connection = match ai_store::get_connection(conn, &target.connection_id)? {
                Some(c) if c.enabled => c,
                _ => {
                    last_rejection_reason = format!(
                        "Connection {} is disabled or not found",
                        target.connection_id
                    );
                    continue;
                }
            };

            let credential = match self.select_credential_for_target(conn, target, &connection.id) {
                Ok(c) => c,
                Err(e) => {
                    last_rejection_reason = e.to_string();
                    continue;
                }
            };

            let secret_ref = SecretRef::new(credential.secret_ref.clone());

            // 标记 in_flight
            {
                let mut states = self.states.lock().unwrap();
                let state = states.entry(credential.id.clone()).or_default();
                state.in_flight += 1;
                state.total_calls += 1;
                state.last_selected_at = Some(Instant::now());
            }

            return Ok(ResolvedRouteTarget {
                connection,
                upstream_model: target.model_id.clone(),
                credential,
                secret_ref,
            });
        }

        Err(Error::Internal(format!(
            "Failed to route model '{model_name}': {last_rejection_reason}"
        )))
    }

    fn select_credential_for_target(
        &self,
        conn: &DbConn,
        target: &RouteTarget,
        connection_id: &str,
    ) -> Result<Credential> {
        let now = Instant::now();

        match &target.credential_selector {
            CredentialSelector::Credential { id } => {
                let cred = ai_store::get_credential(conn, id)?.ok_or_else(|| {
                    Error::NotFound(format!("Explicit credential {id} not found"))
                })?;
                if cred.status != CredentialStatus::Active {
                    return Err(Error::Internal(format!("Credential {id} is not active")));
                }

                let states = self.states.lock().unwrap();
                if let Some(state) = states.get(id) {
                    if let Some(cooling) = state.cooling_until {
                        if now < cooling {
                            return Err(Error::Internal(format!(
                                "Credential {id} is cooling down"
                            )));
                        }
                    }
                    if cred.concurrency_limit > 0
                        && state.in_flight >= cred.concurrency_limit as usize
                    {
                        return Err(Error::Internal(format!(
                            "Credential {id} concurrency limit reached"
                        )));
                    }
                }

                Ok(cred)
            }
            CredentialSelector::Pool { policy } => {
                // 查询绑定了该 connection 的所有可用 active credentials
                let all_creds = ai_store::list_credentials(conn, None)?;
                let mut candidate_creds = Vec::new();

                for c in all_creds {
                    if c.status != CredentialStatus::Active {
                        continue;
                    }
                    let bound_conns = ai_store::list_credential_connections(conn, &c.id)?;
                    if bound_conns.is_empty() || bound_conns.contains(&connection_id.to_string()) {
                        candidate_creds.push(c);
                    }
                }

                if candidate_creds.is_empty() {
                    return Err(Error::NotFound(format!(
                        "No available active credentials bound to connection {connection_id}"
                    )));
                }

                // 过滤处于冷却中或并发超限的 credentials
                let states = self.states.lock().unwrap();
                candidate_creds.retain(|c| {
                    if let Some(state) = states.get(&c.id) {
                        if let Some(cooling) = state.cooling_until {
                            if now < cooling {
                                return false;
                            }
                        }
                        if c.concurrency_limit > 0
                            && state.in_flight >= c.concurrency_limit as usize
                        {
                            return false;
                        }
                    }
                    true
                });

                if candidate_creds.is_empty() {
                    return Err(Error::Internal(format!("All credentials for connection {connection_id} are cooling down or at max concurrency")));
                }

                // 根据策略选择
                match policy {
                    PoolPolicy::PriorityRoundRobin => {
                        let best_priority = candidate_creds
                            .iter()
                            .map(|c| c.priority)
                            .min()
                            .unwrap_or(0);
                        let top_priority_creds: Vec<_> = candidate_creds
                            .into_iter()
                            .filter(|c| c.priority == best_priority)
                            .collect();

                        let mut cursors = self.rr_cursors.lock().unwrap();
                        let cursor = cursors.entry(target.id.clone()).or_insert(0);
                        let idx = *cursor % top_priority_creds.len();
                        *cursor = (*cursor + 1) % top_priority_creds.len();

                        Ok(top_priority_creds[idx].clone())
                    }
                    PoolPolicy::RoundRobin => {
                        let mut cursors = self.rr_cursors.lock().unwrap();
                        let cursor = cursors.entry(target.id.clone()).or_insert(0);
                        let idx = *cursor % candidate_creds.len();
                        *cursor = (*cursor + 1) % candidate_creds.len();

                        Ok(candidate_creds[idx].clone())
                    }
                    PoolPolicy::LeastInflight => {
                        candidate_creds
                            .sort_by_key(|c| states.get(&c.id).map(|s| s.in_flight).unwrap_or(0));
                        Ok(candidate_creds.remove(0))
                    }
                }
            }
        }
    }

    /// 记录请求完成（释放 in_flight，记录成功）
    pub fn record_success(&self, credential_id: &str) {
        let mut states = self.states.lock().unwrap();
        if let Some(state) = states.get_mut(credential_id) {
            state.in_flight = state.in_flight.saturating_sub(1);
            state.consecutive_failures = 0;
            state.success_calls += 1;
            state.cooling_until = None;
            state.last_error = None;
        }
    }

    /// 记录请求失败（释放 in_flight，增加失败计数并触发冷却）
    pub fn record_failure(&self, credential_id: &str, error_msg: &str) {
        let mut states = self.states.lock().unwrap();
        let state = states.entry(credential_id.to_string()).or_default();
        state.in_flight = state.in_flight.saturating_sub(1);
        state.failed_calls += 1;
        state.consecutive_failures += 1;
        state.last_error = Some(error_msg.to_string());

        if state.consecutive_failures >= self.failure_threshold {
            state.cooling_until = Some(Instant::now() + self.cooldown_duration);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::model::{CredentialKind, UpstreamProtocol};
    use crate::db::{apply_migrations, create_tables};
    use crate::proxy::model::Route;

    fn setup_test_db() -> DbConn {
        let conn = DbConn::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_route_resolver_flow() {
        let conn = setup_test_db();
        let _p = ai_store::create_provider(
            &conn,
            Some("p1"),
            None,
            "OpenAI",
            "https://openai.com",
            None,
            true,
        )
        .unwrap();
        let c = ai_store::create_connection(
            &conn,
            Some("c1"),
            "p1",
            "OpenAI Chat",
            "https://api.openai.com/v1",
            UpstreamProtocol::OpenaiChatCompletions,
            None,
            None,
            None,
            true,
        )
        .unwrap();

        let cred = ai_store::insert_credential(
            &conn,
            "cred-1",
            "p1",
            CredentialKind::ApiKey,
            "Key 1",
            "natives/ai/credential/cred-1/v1",
            1,
            "sk-1234",
            CredentialStatus::Active,
            0,
            5,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        ai_store::bind_credential_connection(&conn, &cred.id, &c.id).unwrap();

        let route = Route {
            id: "r1".into(),
            local_model: "gpt-4o".into(),
            enabled: true,
            strategy: "ordered".into(),
            targets: vec![RouteTarget {
                id: "t1".into(),
                route_id: "r1".into(),
                position: 0,
                connection_id: "c1".into(),
                model_id: "gpt-4o-2024-08-06".into(),
                credential_selector: CredentialSelector::Pool {
                    policy: PoolPolicy::PriorityRoundRobin,
                },
                priority: 0,
                enabled: true,
            }],
            created_at: "t0".into(),
            updated_at: "t0".into(),
        };
        store::save_route(&conn, &route).unwrap();

        let resolver = RouteResolver::default();
        let resolved = resolver.resolve_route_target(&conn, "gpt-4o").unwrap();
        assert_eq!(resolved.connection.id, "c1");
        assert_eq!(resolved.upstream_model, "gpt-4o-2024-08-06");
        assert_eq!(resolved.credential.id, "cred-1");

        resolver.record_success("cred-1");
    }
}
