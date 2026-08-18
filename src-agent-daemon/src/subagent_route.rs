//! Subagent route policy: per-parent credential bindings + routing (migration 010).
//!
//! Split from `subagent_store` (route responsibility). Stores only
//! provider/key/model **IDs** — never plaintext credentials.

use super::*;

/// One credential binding (IDs only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteBinding {
    pub provider_id: String,
    pub key_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutePolicy {
    pub parent_conversation_id: String,
    pub mode: String,
    pub bindings: Vec<RouteBinding>,
    pub created_at: String,
    pub updated_at: String,
}

pub(crate) fn parse_bindings(raw: &str) -> Vec<RouteBinding> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Upsert route policy for a parent conversation.
pub fn upsert_route_policy(
    parent_conversation_id: &str,
    mode: &str,
    bindings: &[RouteBinding],
) -> Result<RoutePolicy, String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Err("parent_conversation_id required".into());
    }
    let mode = match mode.trim() {
        "random" => "random",
        "custom" => "custom",
        _ => "default",
    };
    if bindings.is_empty() {
        return Err("bindings must be non-empty".into());
    }
    for b in bindings {
        if b.provider_id.trim().is_empty()
            || b.key_id.trim().is_empty()
            || b.model_id.trim().is_empty()
        {
            return Err("each binding needs provider_id + key_id + model_id".into());
        }
        if b.key_id.eq_ignore_ascii_case("auto") {
            return Err("key_id must not be 'auto'".into());
        }
    }
    let bindings_json =
        serde_json::to_string(bindings).map_err(|e| format!("bindings serialize: {e}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "INSERT INTO subagent_route_policy (parent_conversation_id, mode, bindings_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(parent_conversation_id) DO UPDATE SET
           mode = excluded.mode,
           bindings_json = excluded.bindings_json,
           updated_at = excluded.updated_at",
        params![parent, mode, bindings_json, now],
    )
    .map_err(|e| format!("upsert_route_policy failed: {e}"))?;
    get_route_policy(parent)?.ok_or_else(|| "route policy missing after upsert".into())
}

pub fn get_route_policy(parent_conversation_id: &str) -> Result<Option<RoutePolicy>, String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Ok(None);
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.query_row(
        "SELECT parent_conversation_id, mode, bindings_json, created_at, updated_at
         FROM subagent_route_policy WHERE parent_conversation_id = ?1",
        params![parent],
        |row| {
            let bindings_raw: String = row.get(2)?;
            Ok(RoutePolicy {
                parent_conversation_id: row.get(0)?,
                mode: row.get(1)?,
                bindings: parse_bindings(&bindings_raw),
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("get_route_policy failed: {e}"))
}

/// Pick next binding from policy (default=first unused then wrap; random=rand; custom=first).
pub fn pick_binding(
    policy: &RoutePolicy,
    attempted: &[RouteBinding],
) -> Result<RouteBinding, String> {
    if policy.bindings.is_empty() {
        return Err("route policy has empty bindings".into());
    }
    let remaining: Vec<_> = policy
        .bindings
        .iter()
        .filter(|b| {
            !attempted.iter().any(|a| {
                a.provider_id == b.provider_id && a.key_id == b.key_id && a.model_id == b.model_id
            })
        })
        .cloned()
        .collect();
    let pool = if remaining.is_empty() {
        return Err("route binding pool exhausted".into());
    } else {
        remaining
    };
    match policy.mode.as_str() {
        "random" => {
            let idx = (uuid::Uuid::new_v4().as_u128() as usize) % pool.len();
            Ok(pool[idx].clone())
        }
        _ => Ok(pool[0].clone()),
    }
}

/// Pick a policy binding that cannot reuse the parent Run's credential.
///
/// Agent Hooks are child runs too: unlike ordinary task assignment, they must
/// never fall back to the parent key when their route pool is absent or spent.
/// Keep model overrides inside the explicit binding so provider/model/key stay
/// one authorized route tuple.
pub fn pick_independent_binding(
    policy: &RoutePolicy,
    parent_key_id: &str,
    model_override: Option<&str>,
) -> Result<RouteBinding, String> {
    let parent_key_id = parent_key_id.trim();
    if parent_key_id.is_empty() {
        return Err("parent credential key_id required for independent child route".into());
    }
    let attempted: Vec<_> = policy
        .bindings
        .iter()
        .filter(|binding| {
            binding.key_id == parent_key_id
                || model_override.is_some_and(|model| binding.model_id != model)
        })
        .cloned()
        .collect();
    pick_binding(policy, &attempted)
        .map_err(|reason| format!("no independent Agent Hook route binding: {reason}"))
}

/// Remove a route policy for a parent conversation (e.g. when bindings are broken/empty).
pub fn delete_route_policy(parent_conversation_id: &str) -> Result<(), String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Ok(());
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "DELETE FROM subagent_route_policy WHERE parent_conversation_id = ?1",
        params![parent],
    )
    .map_err(|e| format!("delete_route_policy failed: {e}"))?;
    Ok(())
}

/// Parent conversation heartbeat (independent of child `last_activity_at`).
///
/// Stored on `subagent_route_policy.last_parent_heartbeat_at`. If no policy row
/// exists yet, creates a placeholder row with empty bindings so the heartbeat
/// column can still be updated (bindings must be filled later by assignment).
pub fn touch_parent_heartbeat(parent_conversation_id: &str) -> Result<(), String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Err("conversation_id required for parent heartbeat".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;

    // Ensure parent conversation exists (FK on route policy).
    let parent_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
            params![parent],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !parent_exists {
        return Err(format!("parent conversation not found: {parent}"));
    }

    let changed = conn
        .execute(
            "UPDATE subagent_route_policy
             SET last_parent_heartbeat_at = ?1, updated_at = ?1
             WHERE parent_conversation_id = ?2",
            params![now, parent],
        )
        .map_err(|e| format!("touch_parent_heartbeat update failed: {e}"))?;
    if changed == 0 {
        // No policy yet: insert a stub row so reaper/UI can still see heartbeat.
        // Empty bindings are rejected by upsert_route_policy; use a sentinel that
        // is never used for pick_binding (empty list is filtered by CHECK? none).
        // Store "[]" — pick_binding will fail until real assignment.
        conn.execute(
            "INSERT INTO subagent_route_policy (
                parent_conversation_id, mode, bindings_json,
                created_at, updated_at, last_parent_heartbeat_at
             ) VALUES (?1, 'default', '[]', ?2, ?2, ?2)
             ON CONFLICT(parent_conversation_id) DO UPDATE SET
               last_parent_heartbeat_at = excluded.last_parent_heartbeat_at,
               updated_at = excluded.updated_at",
            params![parent, now],
        )
        .map_err(|e| format!("touch_parent_heartbeat insert failed: {e}"))?;
    }
    Ok(())
}

/// True when parent heartbeat is fresher than `within_secs`.
pub fn parent_heartbeat_recent(parent_conversation_id: &str, within_secs: i64) -> bool {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return false;
    }
    let Ok(s) = store() else {
        return false;
    };
    let Ok(conn) = s.conn() else {
        return false;
    };
    let ts: Option<String> = conn
        .query_row(
            "SELECT last_parent_heartbeat_at FROM subagent_route_policy
             WHERE parent_conversation_id = ?1",
            params![parent],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let Some(ts) = ts.filter(|s| !s.trim().is_empty()) else {
        return false;
    };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&ts) else {
        return false;
    };
    let age = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
    age.num_seconds() <= within_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("subagent-{}.db", Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("subagent temp db migrate");
        // Ensure parent conversation exists for FK tests.
        crate::conversation_store::ensure_conversation_stub(
            "parent-1", "openai", "gpt-4o", None, None,
        )
        .unwrap();
        f();
        crate::storage::set_test_db_override(None, None);
    }

    #[test]
    fn route_policy_upsert_get_and_pick() {
        with_temp_db(|| {
            let b1 = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let b2 = RouteBinding {
                provider_id: "anthropic".into(),
                key_id: "k2".into(),
                model_id: "claude".into(),
            };
            upsert_route_policy("parent-1", "default", &[b1.clone(), b2.clone()]).unwrap();
            let p = get_route_policy("parent-1").unwrap().unwrap();
            assert_eq!(p.bindings.len(), 2);
            let first = pick_binding(&p, &[]).unwrap();
            assert_eq!(first, b1);
            let second = pick_binding(&p, std::slice::from_ref(&b1)).unwrap();
            assert_eq!(second, b2);
            assert!(pick_binding(&p, &[b1, b2]).is_err());
        });
    }

    #[test]
    fn rejects_auto_key_id() {
        with_temp_db(|| {
            let b = RouteBinding {
                provider_id: "openai".into(),
                key_id: "auto".into(),
                model_id: "gpt-4o".into(),
            };
            assert!(upsert_route_policy("parent-1", "default", &[b]).is_err());
        });
    }

    #[test]
    fn three_tasks_share_one_route_policy_pool() {
        with_temp_db(|| {
            // Simulates batch assignment: one policy covers multiple task bindings.
            let bindings = vec![
                RouteBinding {
                    provider_id: "openai".into(),
                    key_id: "k1".into(),
                    model_id: "gpt-4o".into(),
                },
                RouteBinding {
                    provider_id: "anthropic".into(),
                    key_id: "k2".into(),
                    model_id: "claude".into(),
                },
                RouteBinding {
                    provider_id: "openai".into(),
                    key_id: "k3".into(),
                    model_id: "gpt-4o-mini".into(),
                },
            ];
            upsert_route_policy("parent-1", "default", &bindings).unwrap();
            let policy = get_route_policy("parent-1").unwrap().unwrap();
            let mut attempted = Vec::new();
            let mut assigned = Vec::new();
            for _ in 0..3 {
                let b = pick_binding(&policy, &attempted).unwrap();
                attempted.push(b.clone());
                assigned.push(b);
            }
            assert_eq!(assigned.len(), 3);
            assert_eq!(assigned[0].key_id, "k1");
            assert_eq!(assigned[1].key_id, "k2");
            assert_eq!(assigned[2].key_id, "k3");
            // Fourth would exhaust under exclusive attempt tracking.
            assert!(pick_binding(&policy, &attempted).is_err());
        });
    }

    #[test]
    fn independent_binding_never_reuses_the_parent_key() {
        with_temp_db(|| {
            let parent = RouteBinding {
                provider_id: "openai".into(),
                key_id: "parent-key".into(),
                model_id: "gpt-4o".into(),
            };
            let child = RouteBinding {
                provider_id: "anthropic".into(),
                key_id: "child-key".into(),
                model_id: "claude".into(),
            };
            upsert_route_policy("parent-1", "default", &[parent, child.clone()]).unwrap();
            let policy = get_route_policy("parent-1").unwrap().unwrap();

            assert_eq!(
                pick_independent_binding(&policy, "parent-key", None).unwrap(),
                child
            );
            assert!(pick_independent_binding(&policy, "parent-key", Some("gpt-4o")).is_err());
        });
    }

    #[tokio::test]
    async fn touch_parent_only_rpc_does_not_require_subagent_id() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        let art = dir.path().join("a");
        std::fs::create_dir_all(&art).unwrap();
        let _ = crate::storage::DataStore::new(&db, &art).unwrap();
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        crate::conversation_store::ensure_conversation_stub(
            "parent-1",
            "openai",
            "gpt-4o",
            Some("ask"),
            None,
        )
        .unwrap();
        let out = request("subagent.touch", json!({ "conversation_id": "parent-1" }))
            .await
            .unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["parent_heartbeat"], true);
        crate::storage::set_test_db_override(None, None);
    }
}
