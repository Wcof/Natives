//! One bounded refresh owned by the active Fund service; no scheduler or daemon.
use super::{refresh_generation, TaskRegistry};
use crate::storage::Store;
use app_runtime_core::http::HttpRequest;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct Task {
    id: u64,
    request_id: String,
    scope: String,
    result: Option<Value>,
}
pub(crate) struct Tasks {
    registry: TaskRegistry,
    current: Mutex<Option<Task>>,
}
impl Tasks {
    pub fn new(cancel: Option<Arc<app_runtime_core::CancellationToken>>) -> Self {
        Self {
            registry: TaskRegistry::with_cancellation(cancel),
            current: Mutex::new(None),
        }
    }
    pub fn cancel(&self) {
        self.registry.cancel_current();
    }
    pub fn is_finished(&self) -> bool {
        self.current
            .lock()
            .is_ok_and(|task| task.as_ref().is_none_or(|task| task.result.is_some()))
    }
    pub fn cancel_task(&self, task_id: u64) {
        self.registry.cancel_generation(task_id);
    }
    pub fn handle(
        self: &Arc<Self>,
        store: Arc<Store>,
        req: &HttpRequest,
        route: &str,
    ) -> Option<Result<String, (u16, String)>> {
        match (req.method.as_str(), route) {
            ("POST", "/api/information/refresh") => {
                Some(self.start(store, &String::from_utf8_lossy(&req.body)))
            }
            ("POST", "/api/information/content") => {
                Some(self.start_request(store, &String::from_utf8_lossy(&req.body), true))
            }
            ("GET", "/api/information/status") => Some(self.status(&req.path)),
            ("POST", "/api/information/cancel") => {
                Some(self.cancel_request(&String::from_utf8_lossy(&req.body)))
            }
            _ => None,
        }
    }
    fn start(self: &Arc<Self>, store: Arc<Store>, body: &str) -> Result<String, (u16, String)> {
        self.start_request(store, body, false)
    }
    fn start_request(
        self: &Arc<Self>,
        store: Arc<Store>,
        body: &str,
        content: bool,
    ) -> Result<String, (u16, String)> {
        let v: Value = serde_json::from_str(body).map_err(|_| (400, "FIN_INVALID: JSON".into()))?;
        let key = v["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or((400, "FIN_INVALID: requestId".into()))?;
        let content_item = if content {
            let id = v["id"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or((400, "FIN_INVALID: id".into()))?;
            let expected = v["expectedVersion"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or((400, "FIN_INVALID: expectedVersion".into()))?;
            Some((id, expected))
        } else {
            None
        };
        let content_scope = content_item
            .as_ref()
            .map(|(id, version)| format!("content:{id}:{version}"));
        let scope = if let Some(scope) = content_scope.as_deref() {
            scope
        } else {
            v.get("scope")
                .map_or(Some("all"), Value::as_str)
                .ok_or((400, "FIN_INVALID: scope".into()))?
        };
        if !content && !matches!(scope, "all" | "holdings" | "watchlist" | "market") {
            return Err((400, "FIN_INVALID: scope".into()));
        }
        let days = if content {
            7
        } else {
            v.get("days")
                .map_or(Some(7), |v| v.as_u64())
                .filter(|d| (1..=30).contains(d))
                .ok_or((400, "FIN_INVALID: days".into()))? as u32
        };
        let offset = if content {
            0
        } else {
            v.get("identityOffset")
                .map_or(Some(0), |v| v.as_u64())
                .filter(|n| *n <= 1_000_000 && n % 50 == 0)
                .ok_or((400, "FIN_INVALID: identityOffset".into()))? as usize
        };
        let request_scope = if content {
            scope.to_string()
        } else {
            format!("{scope}:{days}:{offset}")
        };
        if v.as_object().is_none_or(|m| {
            m.keys().any(|k| {
                if content {
                    !matches!(k.as_str(), "id" | "expectedVersion" | "requestId")
                } else {
                    !matches!(
                        k.as_str(),
                        "scope" | "requestId" | "days" | "identityOffset"
                    )
                }
            })
        }) {
            return Err((400, "FIN_INVALID: fields".into()));
        }
        let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = guard.as_ref() {
            if task.request_id == key {
                if task.scope != request_scope {
                    return Err((409, "FIN_CONFLICT: requestId".into()));
                }
                return Ok(json!({"taskId":task.id}).to_string());
            }
            if task.result.is_none() {
                return Err((409, "INFORMATION_REFRESH_IN_PROGRESS".into()));
            }
        }
        let content_item = if let Some((id, expected)) = content_item {
            let row = store
                .with_read(|c| super::store::item_detail(c, id))
                .map_err(|_| (500, "INFORMATION_DB".into()))?
                .ok_or((404, "INFORMATION_NOT_FOUND".into()))?;
            if row.row.archived {
                return Err((404, "INFORMATION_NOT_FOUND".into()));
            }
            if row.row.content_version != expected {
                return Err((409, "FIN_CONFLICT: contentVersion".into()));
            }
            if row.row.source_id != super::adapters::SRC_STOCK_ANN {
                return Err((400, "INFORMATION_UNSUPPORTED".into()));
            }
            let enabled = store
                .with_read(|c| super::enabled_sources(c, &[row.row.source_id.as_str()]))
                .map_err(|_| (500, "INFORMATION_DB".into()))?;
            if enabled.is_empty() {
                return Err((400, "INFORMATION_SOURCE_DISABLED".into()));
            }
            let external = store
                .with_read(|c| {
                    c.query_row(
                        "SELECT external_id FROM information_items WHERE id=?1",
                        [id],
                        |r| r.get::<_, String>(0),
                    )
                })
                .map_err(|_| (500, "INFORMATION_DB".into()))?;
            Some((id, expected, external))
        } else {
            None
        };
        let watchlist = if content || matches!(scope, "holdings" | "market") {
            vec![]
        } else {
            store.with_read(|c|{
   let mut stmt=c.prepare("SELECT DISTINCT symbol FROM watchlist_items WHERE asset_type IN ('stock','etf') ORDER BY symbol")?;
   let rows=stmt.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>();rows
  }).map_err(|_|(500,"INFORMATION_DB".into()))?
        };
        let cache_revision = store
            .with_read(super::cache::revision)
            .map_err(|_| (500, "INFORMATION_DB".into()))?;
        let id = self.registry.start();
        *guard = Some(Task {
            id,
            request_id: key.into(),
            scope: request_scope,
            result: None,
        });
        drop(guard);
        let owner = self.clone();
        let include_holdings = matches!(scope, "all" | "holdings");
        let include_market = matches!(scope, "all" | "market");
        let network_context = crate::network::current();
        let spawned = std::thread::Builder::new()
            .name("fund-information".into())
            .spawn(move || {
                let _network_scope = crate::network::bind(network_context);
                let now = crate::finance::now_secs();
                let stamp =
                    super::super::api::information_api::format_unix(now.parse().unwrap_or(0));
                let result=if let Some((item,version,external))=content_item{
                    let cached=store.with_read(|c|super::store::content_cache_fresh(c,item,&stamp)).map_err(|_|(500,"INFORMATION_DB".into()));
                    let fresh=cached.as_ref()==Ok(&true);
                    let fetched=cached.and_then(|_|if fresh {store.with_read(|c|super::store::item_detail(c,item)).map_err(|_|(500,"INFORMATION_DB".into())).and_then(|d|d.map(|d|(d.content_text.unwrap_or_default(),match d.row.completeness.as_str(){"full"=>"full","truncated"=>"truncated",_=>"unavailable"})).ok_or((404,"INFORMATION_NOT_FOUND".into())))}else{super::adapters::fetch_announcement_content(&external).map_err(|e|(502,format!("INFORMATION_UPSTREAM: {}",e.code())))});
                    match fetched.and_then(|(text,completeness)|owner.registry.commit(id,||store.with_write(|c|{
                        if super::cache::revision(c)?!=cache_revision{return Err(crate::finance::FinanceError::Conflict("缓存已清理，请重试".into()));}
                        if super::enabled_sources(c,&[super::adapters::SRC_STOCK_ANN])?.is_empty(){return Err(crate::finance::FinanceError::Conflict("来源已停用".into()));}
                        let current:i64=c.query_row("SELECT content_version FROM information_items WHERE id=?1 AND archived=0",[item],|r|r.get(0))?;
                        if current!=version{return Err(crate::finance::FinanceError::Conflict("contentVersion".into()));}
                        if !fresh { super::store::save_content(c,item,&text,completeness,&stamp)?; }
                        Ok(json!({"id":item,"completeness":completeness,"length":text.len(),"cached":fresh}))
                    })).map_err(|e:crate::finance::FinanceError|if matches!(e,crate::finance::FinanceError::Conflict(_)){(409,"FIN_CONFLICT: contentVersion".into())}else{(500,"INFORMATION_DB".into())})){Ok(Some(value))=>json!({"content":value}),Ok(None)=>json!({"cancelled":true}),Err((status,error))=>json!({"error":error,"status":status})}
                }else{
                let outcome = refresh_generation(
                    &owner.registry,
                    &store,
                    &watchlist,
                    &stamp,
                    id,
                    include_holdings,
                    include_market,
                    days,
                    offset,
                );
                json!({"outcome":outcome,"coverage":super::COVERAGE_NOTE})
                };
                let mut guard = owner.current.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(task) = guard.as_mut().filter(|t| t.id == id) {
                    task.result = Some(result);
                }
            });
        if spawned.is_err() {
            self.cancel();
            self.current
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            return Err((503, "INFORMATION_TASK_UNAVAILABLE".into()));
        }
        Ok(json!({"taskId":id}).to_string())
    }
    fn status(&self, path: &str) -> Result<String, (u16, String)> {
        let id = path
            .split('?')
            .nth(1)
            .and_then(|s| s.split('&').find_map(|p| p.strip_prefix("taskId=")))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or((400, "FIN_INVALID: taskId".into()))?;
        let guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let task = guard
            .as_ref()
            .filter(|t| t.id == id)
            .ok_or((404, "INFORMATION_TASK_NOT_FOUND".into()))?;
        Ok(json!({"taskId":id,"status":if self.registry.is_cancelled(id){"cancelled"}else if task.result.is_some(){"complete"}else{"running"},"result":task.result}).to_string())
    }
    fn cancel_request(&self, body: &str) -> Result<String, (u16, String)> {
        let v: Value = serde_json::from_str(body).map_err(|_| (400, "FIN_INVALID: JSON".into()))?;
        let guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let task = guard
            .as_ref()
            .filter(|t| v["taskId"].as_u64() == Some(t.id))
            .ok_or((404, "INFORMATION_TASK_NOT_FOUND".into()))?;
        self.cancel();
        Ok(json!({"taskId":task.id,"cancelled":true}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_rejects_invalid_windows_offsets_and_changed_idempotency_scope() {
        let owner = Arc::new(Tasks::new(None));
        let store = Arc::new(Store::open_in_memory().unwrap());
        for extra in [
            json!({"scope":null}),
            json!({"scope":123}),
            json!({"days":0}),
            json!({"days":31}),
            json!({"days":1.5}),
            json!({"identityOffset":1}),
            json!({"identityOffset":-50}),
        ] {
            let mut request = json!({"requestId":"invalid","scope":"holdings"});
            request
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(
                owner
                    .start(store.clone(), &request.to_string())
                    .unwrap_err()
                    .0,
                400
            );
        }
        *owner.current.lock().unwrap() = Some(Task {
            id: 9,
            request_id: "same".into(),
            scope: "holdings:7:0".into(),
            result: Some(json!({})),
        });
        assert_eq!(
            owner
                .start(
                    store.clone(),
                    r#"{"requestId":"same","scope":"holdings","days":30}"#
                )
                .unwrap_err()
                .0,
            409
        );
        assert_eq!(
            owner
                .start(
                    store.clone(),
                    r#"{"requestId":"same","scope":"holdings","identityOffset":50}"#
                )
                .unwrap_err()
                .0,
            409
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &owner
                    .start(store, r#"{"requestId":"same","scope":"holdings"}"#)
                    .unwrap()
            )
            .unwrap()["taskId"],
            9
        );
    }

    #[test]
    fn completed_content_retry_reuses_result_before_changed_version_lookup() {
        let owner = Arc::new(Tasks::new(None));
        let store = Arc::new(Store::open_in_memory().unwrap());
        *owner.current.lock().unwrap() = Some(Task {
            id: 7,
            request_id: "retry".into(),
            scope: "content:42:1".into(),
            result: Some(json!({"content":{"id":42}})),
        });
        let body = r#"{"requestId":"retry","id":42,"expectedVersion":1}"#;
        let result = owner.start_request(store.clone(), body, true).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&result).unwrap()["taskId"], 7);
        assert_eq!(
            owner
                .start_request(
                    store.clone(),
                    r#"{"requestId":"retry","id":42,"expectedVersion":2}"#,
                    true
                )
                .unwrap_err()
                .0,
            409
        );
        assert_eq!(owner.start_request(store, r#"{"requestId":"retry","id":42,"expectedVersion":1,"url":"https://example.com"}"#, true).unwrap_err().0, 400);
    }
    #[test]
    fn refresh_is_owned_bounded_and_late_cancel_cannot_cancel_next() {
        let owner = Arc::new(Tasks::new(None));
        let store = Arc::new(Store::open_in_memory().unwrap());
        assert!(owner.start(store.clone(), "{}").is_err());
        let id: Value = serde_json::from_str(
            &owner
                .start(store.clone(), r#"{"requestId":"a","scope":"holdings"}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(
                &owner
                    .start(store.clone(), r#"{"requestId":"a","scope":"holdings"}"#)
                    .unwrap()
            )
            .unwrap()["taskId"],
            id["taskId"]
        );
        let first = id["taskId"].as_u64().unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while owner
            .current
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .result
            .is_none()
            && std::time::Instant::now() < until
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(owner
            .current
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .result
            .is_some());
        let second: Value = serde_json::from_str(
            &owner
                .start(store.clone(), r#"{"requestId":"b","scope":"holdings"}"#)
                .unwrap(),
        )
        .unwrap();
        assert_ne!(second["taskId"], id["taskId"]);
        assert!(owner
            .cancel_request(&format!("{{\"taskId\":{first}}}"))
            .is_err());
        assert!(!owner
            .registry
            .is_cancelled(second["taskId"].as_u64().unwrap()));
        owner.cancel();
    }
}
