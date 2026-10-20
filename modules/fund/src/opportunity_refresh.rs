//! Session-owned, bounded opportunity refresh. Reuses market/NAV and the existing commit gate.
use crate::opportunity_history_source::{fetch, Payload};
use crate::{information::TaskRegistry, storage::Store, wire::api_error};
use app_runtime_core::http::HttpRequest;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Task {
    id: u64,
    request_id: String,
    scope: String,
    offset: usize,
    revision: String,
    pool_id: String,
    pool_total: usize,
    total: usize,
    next: Option<usize>,
    results: Vec<Value>,
    finished: bool,
    deadline_hit: bool,
    generated: Option<Value>,
    generation_error: Option<&'static str>,
}

pub(crate) struct Tasks {
    registry: TaskRegistry,
    current: Mutex<Option<Task>>,
    /// 生产开启市场发现有界池种子（v23）；测试构造保持关闭以保证离线确定性。
    enable_discovery: bool,
}

impl Tasks {
    #[cfg(test)]
    pub fn new(cancel: Option<Arc<app_runtime_core::CancellationToken>>) -> Self {
        Self::with_discovery(cancel, false)
    }
    pub fn with_discovery(
        cancel: Option<Arc<app_runtime_core::CancellationToken>>,
        enable_discovery: bool,
    ) -> Self {
        Self {
            registry: TaskRegistry::with_cancellation(cancel),
            current: Mutex::new(None),
            enable_discovery,
        }
    }
    pub fn cancel(&self) {
        self.registry.cancel_current();
    }
    pub fn is_finished(&self) -> bool {
        self.current
            .lock()
            .is_ok_and(|task| task.as_ref().is_none_or(|task| task.finished))
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
            ("POST", "/api/opportunities/refresh") => Some(self.start(store, &req.body, fetch)),
            ("GET", "/api/opportunities/status") => Some(self.status(&req.path)),
            ("POST", "/api/opportunities/cancel") => Some(self.cancel_request(&req.body)),
            _ => None,
        }
    }
    fn start<F>(
        self: &Arc<Self>,
        store: Arc<Store>,
        body: &[u8],
        fetcher: F,
    ) -> Result<String, (u16, String)>
    where
        F: Fn(&Value) -> Result<Payload, &'static str> + Send + 'static,
    {
        let v: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
        let key = v["requestId"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
            .ok_or_else(invalid)?;
        let scope = v
            .get("scope")
            .map_or(Some("all"), Value::as_str)
            .filter(|s| matches!(*s, "all" | "holdings" | "watchlist"))
            .ok_or_else(invalid)?;
        let offset = v
            .get("offset")
            .map_or(Some(0), Value::as_u64)
            .filter(|n| *n < 100)
            .ok_or_else(invalid)? as usize;
        let expected_pool = match v.get("poolId") {
            Some(v) => Some(v.as_str().ok_or_else(invalid)?),
            None => None,
        };
        if expected_pool.is_some_and(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
            || offset > 0 && expected_pool.is_none()
        {
            return Err(invalid());
        }
        if v.as_object().is_none_or(|m| {
            m.keys()
                .any(|k| !matches!(k.as_str(), "requestId" | "scope" | "offset" | "poolId"))
        }) {
            return Err(invalid());
        }
        let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = guard.as_ref() {
            if task.request_id == key {
                if task.scope != scope
                    || task.offset != offset
                    || expected_pool.is_some_and(|p| p != task.pool_id)
                {
                    return Err(api_error(
                        409,
                        "OPPORTUNITY_CONFLICT",
                        "同一请求的范围已经改变",
                    ));
                }
                return Ok(json!({"taskId":task.id}).to_string());
            }
            // Cancellation is not permission to overlap the still-draining upstream request.
            if !task.finished {
                return Err(api_error(
                    409,
                    "OPPORTUNITY_BUSY",
                    "前一刷新尚未退出，请稍后重试",
                ));
            }
        }
        let snapshot = crate::opportunity_records::generate_home(&store, "month", None, None)
            .map_err(|e| {
                api_error(
                    if matches!(e, crate::finance::FinanceError::Conflict(_)) {
                        409
                    } else {
                        500
                    },
                    "OPPORTUNITY_DB",
                    "无法生成刷新范围",
                )
            })?;
        let mut targets: Vec<Value> = snapshot["opportunities"]["items"]
            .as_array()
            .ok_or_else(|| api_error(500, "OPPORTUNITY_DB", "刷新范围无效"))?
            .iter()
            .filter(|r| {
                scope == "all"
                    || if scope == "holdings" {
                        r["isHolding"] == true
                    } else {
                        r["pool"] == "watchlist" || r["pool"] == "mixed"
                    }
            })
            .cloned()
            .collect();
        targets.sort_by(|a, b| a["securityId"].as_str().cmp(&b["securityId"].as_str()));
        let pool_id = app_runtime_core::sha256_hex(
            serde_json::to_string(&targets.iter().map(|r| &r["securityId"]).collect::<Vec<_>>())
                .unwrap()
                .as_bytes(),
        );
        if expected_pool.is_some_and(|p| p != pool_id) {
            return Err(api_error(
                409,
                "OPPORTUNITY_CONFLICT",
                "持仓或自选范围已变化，请重新刷新",
            ));
        }
        let count = targets.len();
        if offset > 0 && offset >= count {
            return Err(invalid());
        }
        let targets: Vec<_> = targets.into_iter().skip(offset).take(10).collect();
        let id = self.registry.start();
        *guard = Some(Task {
            id,
            request_id: key.into(),
            scope: scope.into(),
            offset,
            revision: snapshot["dataRevision"].as_str().unwrap_or_default().into(),
            pool_id,
            pool_total: count,
            total: targets.len(),
            next: if offset + targets.len() < count {
                Some(offset + 10)
            } else {
                None
            },
            results: vec![],
            finished: false,
            deadline_hit: false,
            generated: None,
            generation_error: None,
        });
        drop(guard);
        let owner = self.clone();
        let deadline = Instant::now() + Duration::from_secs(30);
        let network_context = crate::network::current();
        if std::thread::Builder::new()
            .name("fund-opportunities".into())
            .spawn(move || {
                let _network_scope = crate::network::bind(network_context);
                owner.run(store, id, targets, deadline, fetcher)
            })
            .is_err()
        {
            self.cancel();
            let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
            if guard.as_ref().is_some_and(|t| t.id == id) {
                guard.take();
            }
            return Err(api_error(
                503,
                "OPPORTUNITY_UNAVAILABLE",
                "刷新任务无法启动",
            ));
        }
        Ok(json!({"taskId":id}).to_string())
    }
    fn run(
        self: Arc<Self>,
        store: Arc<Store>,
        id: u64,
        mut targets: Vec<Value>,
        deadline: Instant,
        fetcher: impl Fn(&Value) -> Result<Payload, &'static str>,
    ) {
        // 市场发现有界种子（供体 market_scan 适配，v23）：仅 scope=all 首批；
        // 两笔上游请求（板块排行→领涨成分股），登记 ≤10 只、留存 ≤50 只；
        // 失败不致命，不延长 30 秒截止。pool_id 仍锚定持仓/自选范围（冲突
        // 语义不变），发现只追加 pool_total。
        if self.enable_discovery && !self.registry.is_cancelled(id) && Instant::now() < deadline {
            let (scope_all, first_batch) = {
                let guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
                guard
                    .as_ref()
                    .filter(|t| t.id == id)
                    .map(|t| (t.scope == "all", t.offset == 0))
                    .unwrap_or((false, false))
            };
            if scope_all && first_batch {
                let entries = crate::opportunity_discovery::seed_from_upstream(&store);
                if !entries.is_empty() && !self.registry.is_cancelled(id) {
                    let new_rows = crate::opportunity_discovery::target_rows(&entries);
                    let added = new_rows.len();
                    targets.extend(new_rows);
                    let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(t) = guard.as_mut().filter(|t| t.id == id) {
                        t.pool_total += added;
                    }
                }
            }
        }
        for row in targets {
            if self.registry.is_cancelled(id) || Instant::now() >= deadline {
                break;
            }
            let result = fetcher(&row).and_then(|payload| {
                self.registry
                    .commit(id, || match payload {
                        Payload::Exchange(symbol, body) => {
                            crate::market::save_opportunity_history(&symbol, body)
                        }
                        Payload::Nav(rows) => store
                            .with_write(|c| -> Result<(), crate::finance::FinanceError> {
                                let code = row["code"].as_str().ok_or_else(|| {
                                    crate::finance::FinanceError::Invalid("code".into())
                                })?;
                                let fund =
                                    crate::ledger::ensure_fund(c, code, "").map_err(|_| {
                                        crate::finance::FinanceError::Invalid("fund".into())
                                    })?;
                                crate::nav::write_nav_rows(c, fund, &rows)?;
                                crate::nav::refresh_latest(c, fund)?;
                                Ok(())
                            })
                            .map_err(|_| "store_error"),
                    })
                    .map_err(|_| "store_error")
                    .and_then(|r| r.ok_or("cancelled"))
            });
            if self.registry.is_cancelled(id) && result.is_err() {
                break;
            }
            let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(task) = guard.as_mut().filter(|t| t.id == id) {
                task.results.push(json!({"securityId":row["securityId"],"status":if result.is_ok(){"fetched"}else{"error"},"errorCode":result.err()}));
            }
        }
        let generated = self.registry.commit(id, || {
            crate::opportunity_records::generate_home(&store, "month", None, None)
        });
        let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = guard.as_mut().filter(|t| t.id == id) {
            match generated {
                Ok(Some(home)) => {
                    t.generated = Some(home["opportunities"]["recordedSnapshot"].clone())
                }
                Ok(None) => {}
                Err(crate::finance::FinanceError::Conflict(_)) => {
                    t.generation_error = Some("generation_conflict")
                }
                _ => t.generation_error = Some("store_error"),
            }
            t.finished = true;
            t.deadline_hit = Instant::now() >= deadline;
            t.next =
                (t.offset + t.results.len() < t.pool_total).then_some(t.offset + t.results.len());
        }
    }
    fn status(&self, path: &str) -> Result<String, (u16, String)> {
        let id = crate::market::query_param(path, "taskId")
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        let guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let t = guard
            .as_ref()
            .filter(|t| t.id == id)
            .ok_or_else(not_found)?;
        let cancelled = self.registry.is_cancelled(id);
        Ok(json!({"taskId":id,"status":if !t.finished {if cancelled{"cancelling"}else{"running"}}else if cancelled{"cancelled"}else{"complete"},"startDataRevision":t.revision,"poolId":t.pool_id,"scope":t.scope,"offset":t.offset,"completed":t.results.len(),"total":t.total,"poolTotal":t.pool_total,"nextOffset":t.next,"deadlineHit":t.deadline_hit,"recordedSnapshot":t.generated,"generationError":t.generation_error,"items":t.results}).to_string())
    }
    fn cancel_request(&self, body: &[u8]) -> Result<String, (u16, String)> {
        let v: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
        let request_id = v["requestId"].as_str().filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        });
        if v.as_object().is_none_or(|m| {
            m.len() != 1 || v["taskId"].as_u64().is_none_or(|n| n == 0) && request_id.is_none()
        }) {
            return Err(invalid());
        }
        let guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let t = guard
            .as_ref()
            .filter(|t| {
                v["taskId"].as_u64() == Some(t.id) || request_id == Some(t.request_id.as_str())
            })
            .ok_or_else(not_found)?;
        if t.finished {
            return Ok(
                json!({"taskId":t.id,"cancelled":self.registry.is_cancelled(t.id),"finished":true})
                    .to_string(),
            );
        }
        self.cancel();
        Ok(json!({"taskId":t.id,"cancelled":true}).to_string())
    }
}
fn invalid() -> (u16, String) {
    api_error(400, "OPPORTUNITY_INVALID", "刷新参数无效")
}
fn not_found() -> (u16, String) {
    api_error(404, "OPPORTUNITY_NOT_FOUND", "刷新任务已失效")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Arc<Store> {
        Arc::new(Store::open_in_memory().unwrap())
    }
    fn watch(store: &Store, symbol: &str, kind: &str) {
        store.with_write(|c| -> rusqlite::Result<()> {
            c.execute("INSERT OR IGNORE INTO watchlists(id,name) VALUES(1,'test')",[])?;
            c.execute("INSERT INTO watchlist_items(watchlist_id,symbol,name,asset_type,sort_order,created_at) VALUES(1,?1,'test',?2,0,CURRENT_TIMESTAMP)",rusqlite::params![symbol,kind])?;
            Ok(())
        }).unwrap();
    }
    fn id(result: Result<String, (u16, String)>) -> u64 {
        serde_json::from_str::<Value>(&result.unwrap()).unwrap()["taskId"]
            .as_u64()
            .unwrap()
    }
    fn status(owner: &Tasks, id: u64) -> Value {
        serde_json::from_str(&owner.status(&format!("?taskId={id}")).unwrap()).unwrap()
    }
    fn finish(owner: &Tasks, id: u64) -> Value {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let s = status(owner, id);
            if matches!(s["status"].as_str(), Some("complete" | "cancelled")) {
                return s;
            }
            assert!(Instant::now() < until, "worker did not finish: {s}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn strict_parameters_and_bounded_pool_continuation() {
        let owner = Arc::new(Tasks::new(None));
        let store = store();
        for extra in [
            json!({"scope":null}),
            json!({"scope":"market"}),
            json!({"offset":100}),
            json!({"offset":1}),
            json!({"offset":-1}),
            json!({"poolId":null}),
            json!({"url":"https://example.com"}),
            json!({"requestId":"bad/owner"}),
        ] {
            let mut v = json!({"requestId":"bad"});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(
                owner
                    .start(store.clone(), &serde_json::to_vec(&v).unwrap(), |_| Err(
                        "unsupported"
                    ))
                    .unwrap_err()
                    .0,
                400
            );
        }
        for n in 0..11 {
            watch(&store, &format!("sz900{n:03}"), "stock");
        }
        let first = id(
            owner.start(store.clone(), br#"{"requestId":"first"}"#, |_| {
                Err("unsupported")
            }),
        );
        let s = finish(&owner, first);
        assert_eq!(s["total"], 10);
        assert_eq!(s["completed"], 10);
        assert_eq!(s["poolTotal"], 11);
        assert_eq!(s["nextOffset"], 10);
        owner
            .cancel_request(&serde_json::to_vec(&json!({"taskId":first})).unwrap())
            .unwrap();
        assert_eq!(
            status(&owner, first)["status"],
            "complete",
            "a completed batch does not become cancelled after its writes were accepted"
        );
        assert_eq!(
            id(
                owner.start(store.clone(), br#"{"requestId":"first"}"#, |_| panic!(
                    "retry must not spawn"
                ))
            ),
            first
        );
        assert_eq!(
            owner
                .start(
                    store.clone(),
                    br#"{"requestId":"first","scope":"holdings"}"#,
                    |_| Err("unsupported")
                )
                .unwrap_err()
                .0,
            409
        );
        let second = id(owner.start(
            store.clone(),
            &serde_json::to_vec(&json!({"requestId":"second","offset":10,"poolId":s["poolId"]}))
                .unwrap(),
            |_| Err("upstream_error"),
        ));
        let last = finish(&owner, second);
        assert_eq!(last["total"], 1);
        assert!(last["nextOffset"].is_null());
        watch(&store, "sz900020", "stock");
        assert_eq!(
            owner
                .start(
                    store,
                    &serde_json::to_vec(
                        &json!({"requestId":"changed","offset":10,"poolId":s["poolId"]})
                    )
                    .unwrap(),
                    |_| Err("unsupported")
                )
                .unwrap_err()
                .0,
            409
        );
    }
    #[test]
    fn cancel_drains_one_worker_and_rejects_late_nav_commit_and_old_cancel() {
        let owner = Arc::new(Tasks::new(None));
        let store = store();
        watch(&store, "110022", "fund");
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let first = id(
            owner.start(store.clone(), br#"{"requestId":"blocking"}"#, move |_| {
                started_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(Payload::Nav(vec![crate::nav::NavRow {
                    fund_code: "110022".into(),
                    nav_date: crate::assets::today_iso(),
                    unit_nav_raw: 12345,
                    accumulated_raw: None,
                    daily_growth_raw: None,
                    source: "isolated-test".into(),
                    fetched_at: "1".into(),
                }]))
            }),
        );
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            crate::opportunity_records::snapshots(&store, 1).unwrap()["total"],
            1
        );
        assert_eq!(store.with_read(|c|c.query_row("SELECT COUNT(*) FROM funds",[],|r|r.get::<_,i64>(0))).unwrap(),0,"network does not hold the DB lock");
        owner
            .cancel_request(&serde_json::to_vec(&json!({"requestId":"blocking"})).unwrap())
            .unwrap();
        assert_eq!(status(&owner, first)["status"], "cancelling");
        assert_eq!(
            owner
                .start(store.clone(), br#"{"requestId":"overlap"}"#, |_| Err(
                    "unsupported"
                ))
                .unwrap_err()
                .0,
            409
        );
        release_tx.send(()).unwrap();
        assert_eq!(finish(&owner, first)["status"], "cancelled");
        assert!(status(&owner, first)["recordedSnapshot"].is_null());
        assert_eq!(
            crate::opportunity_records::snapshots(&store, 1).unwrap()["total"],
            1,
            "cancel prevents a finish-generation cohort"
        );
        assert_eq!(
            store
                .with_read(
                    |c| c.query_row("SELECT COUNT(*) FROM fund_nav", [], |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            0
        );
        let second = id(owner.start(store, br#"{"requestId":"new"}"#, |_| Err("unsupported")));
        finish(&owner, second);
        assert_eq!(
            owner
                .cancel_request(&serde_json::to_vec(&json!({"taskId":first})).unwrap())
                .unwrap_err()
                .0,
            404
        );
        assert!(!owner.registry.is_cancelled(second));
    }
    #[test]
    fn nav_refresh_has_real_history_and_partial_errors_without_creating_holdings() {
        let owner = Arc::new(Tasks::new(None));
        let store = store();
        watch(&store, "110022", "fund");
        watch(&store, "sh900100", "stock");
        let today = crate::assets::today_iso();
        let day = crate::assets::days_from_civil(&today).unwrap();
        let n = id(
            owner.start(store.clone(), br#"{"requestId":"mixed"}"#, move |r| {
                if r["kind"] != "fund" {
                    return Err("upstream_error");
                }
                Ok(Payload::Nav(
                    (0..60)
                        .map(|n| crate::nav::NavRow {
                            fund_code: "110022".into(),
                            nav_date: crate::assets::civil_from_days(day - 59 + n),
                            unit_nav_raw: 10000 + n * 10,
                            accumulated_raw: None,
                            daily_growth_raw: None,
                            source: "isolated-test".into(),
                            fetched_at: "1".into(),
                        })
                        .collect(),
                ))
            }),
        );
        let s = finish(&owner, n);
        assert_eq!(s["completed"], 2);
        assert_eq!(s["items"][0]["status"], "fetched");
        assert_eq!(s["items"][1]["errorCode"], "upstream_error");
        let snapshot = crate::home::build_snapshot(&store, "month").unwrap();
        let fund = &snapshot["opportunities"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "fund")
            .unwrap();
        assert_eq!(fund["quality"], "ready");
        assert_eq!(fund["source"], "isolated-test");
        assert_eq!(fund["isHolding"], false);
        assert!(fund["evidence"]["volumeRatio"].is_null());
        let count = store
            .with_read(|c| {
                c.query_row("SELECT COUNT(*) FROM transactions", [], |r| {
                    r.get::<_, i64>(0)
                })
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn runtime_cancellation_does_not_start_upstream_work() {
        let token = Arc::new(app_runtime_core::CancellationToken::new());
        let owner = Arc::new(Tasks::new(Some(token.clone())));
        let store = store();
        watch(&store, "110022", "fund");
        token.cancel();
        let task = id(owner.start(store, br#"{"requestId":"shutdown"}"#, |_| {
            panic!("cancelled runtime must not fetch")
        }));
        assert_eq!(finish(&owner, task)["status"], "cancelled");
    }

    #[test]
    fn deadline_does_not_skip_unprocessed_identities() {
        let owner = Arc::new(Tasks::new(None));
        let store = store();
        let task = owner.registry.start();
        *owner.current.lock().unwrap() = Some(Task {
            id: task,
            request_id: "deadline".into(),
            scope: "all".into(),
            offset: 10,
            revision: "1".into(),
            pool_id: "a".repeat(64),
            pool_total: 12,
            total: 2,
            next: None,
            results: vec![],
            finished: false,
            deadline_hit: false,
            generated: None,
            generation_error: None,
        });
        owner.clone().run(
            store,
            task,
            vec![json!({"securityId":"fund:110022"})],
            Instant::now(),
            |_| panic!("expired batch must not fetch"),
        );
        let s = finish(&owner, task);
        assert_eq!(s["completed"], 0);
        assert_eq!(s["nextOffset"], 10);
        assert_eq!(s["deadlineHit"], true);
    }
}
