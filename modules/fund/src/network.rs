//! Session-owned network configuration; no ambient proxy or credential authority.
use crate::{finance::FinanceError, storage::Store};
use app_runtime_core::secrets::SecretStore;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) struct Context {
    pub store: Arc<Store>,
    pub secrets: Option<Arc<SecretStore>>,
    receipt: Mutex<Option<(Value, String, Instant)>>,
}
impl Context {
    pub fn new(store: Arc<Store>, secrets: Option<Arc<SecretStore>>) -> Arc<Self> {
        Arc::new(Self {
            store,
            secrets,
            receipt: Mutex::new(None),
        })
    }
    pub fn config(&self) -> Result<Value, FinanceError> {
        self.store.with_read(read)
    }
    pub fn password(&self) -> Result<String, FinanceError> {
        self.secrets
            .as_ref()
            .ok_or_else(|| invalid("代理凭据服务不可用"))?
            .reveal("proxy:password")
            .map_err(|_| invalid("代理凭据不可读取"))?
            .ok_or_else(|| invalid("请先配置代理密码"))
    }
    pub fn credential_revision(&self) -> Result<Value, FinanceError> {
        match &self.secrets {
            Some(s) => {
                let v = s
                    .status("proxy:password")
                    .map_err(|_| invalid("代理凭据状态不可读取"))?;
                Ok(json!({"exists":v.exists,"updatedAt":v.updated_at}))
            }
            None => Ok(json!({"exists":false,"updatedAt":null})),
        }
    }
    pub fn probe(&self, input: &Value) -> Result<Value, FinanceError> {
        validate_input(input, &["config", "expectedVersion"])?;
        validate(&input["config"])?;
        let current = self.config()?;
        if current["version"] != input["expectedVersion"] {
            return Err(FinanceError::Conflict("代理配置已变化".into()));
        }
        if input["config"]["mode"] != "https_proxy" {
            return Err(invalid("直连无需验证代理"));
        }
        let credential_revision = self.credential_revision()?;
        let agent = transport::agent(self, &input["config"])
            .map_err(|_| invalid("代理配置或凭据不可用"))?;
        let mut response = agent
            .get("https://qt.gtimg.cn/q=sh000001")
            .call()
            .map_err(|_| invalid("代理连通验证失败，原配置未修改"))?;
        if !response.status().is_success() {
            return Err(invalid("代理验证上游状态无效"));
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(8192)
            .read_to_vec()
            .map_err(|_| invalid("代理验证响应无效"))?;
        if !body.starts_with(b"v_sh000001=") {
            return Err(invalid("代理验证上游内容无效"));
        }
        if self.credential_revision()? != credential_revision {
            return Err(FinanceError::Conflict("验证期间凭据已变化".into()));
        }
        self.accept_probe(input, current, credential_revision)
    }
    fn accept_probe(
        &self,
        input: &Value,
        current: Value,
        credential_revision: Value,
    ) -> Result<Value, FinanceError> {
        if self.config()?["version"] != current["version"] {
            return Err(FinanceError::Conflict("验证期间配置已变化".into()));
        }
        if self.credential_revision()? != credential_revision {
            return Err(FinanceError::Conflict("验证期间凭据已变化".into()));
        }
        let mut binding = input.clone();
        binding["credentialRevision"] = credential_revision;
        let token =
            app_runtime_core::session::random_id().ok_or_else(|| invalid("无法生成验证身份"))?;
        *self.receipt.lock().map_err(|_| invalid("验证状态不可用"))? =
            Some((binding, token.clone(), Instant::now()));
        Ok(json!({"ok":true,"probeToken":token,"validForSeconds":60}))
    }
    pub fn validate_save(&self, input: &Value) -> Result<(), FinanceError> {
        validate_input(
            input,
            &["config", "expectedVersion", "probeToken", "requestId"],
        )?;
        validate(&input["config"])?;
        if input["expectedVersion"].as_i64().is_none_or(|v| v < 0) {
            return Err(invalid("网络版本无效"));
        }
        if input["config"]["mode"] == "https_proxy" {
            let binding = json!({"config":input["config"],"expectedVersion":input["expectedVersion"],"credentialRevision":self.credential_revision()?});
            let receipt = self.receipt.lock().map_err(|_| invalid("验证状态不可用"))?;
            if !receipt.as_ref().is_some_and(|(value, token, at)| {
                *value == binding
                    && input["probeToken"].as_str() == Some(token)
                    && at.elapsed() < Duration::from_secs(60)
            }) {
                return Err(invalid("请先验证当前代理配置"));
            }
        }
        Ok(())
    }
    pub fn save(&self, input: &Value) -> Result<Value, FinanceError> {
        if let Some(result) = self.replay(input)? {
            return Ok(result);
        }
        self.validate_save(input)?;
        let credential_revision = if input["config"]["username"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            self.receipt
                .lock()
                .map_err(|_| invalid("验证状态不可用"))?
                .as_ref()
                .ok_or_else(|| invalid("验证状态不存在"))?
                .0["credentialRevision"]
                .clone()
        } else {
            Value::Null
        };
        crate::config_receipts::write(&self.store, "network", input, |c| {
            let current = read(c)?;
            if current["version"] != input["expectedVersion"] {
                return Err(FinanceError::Conflict("网络配置已变化".into()));
            }
            let version = current["version"]
                .as_i64()
                .and_then(|v| v.checked_add(1))
                .ok_or_else(|| FinanceError::Overflow("网络配置版本".into()))?;
            let mut saved = input["config"].clone();
            saved["version"] = json!(version);
            c.execute("INSERT INTO app_meta(key,value) VALUES('network_settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[saved.to_string()])?;
            c.execute("INSERT INTO app_meta(key,value) VALUES('network_credential_revision',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[credential_revision.to_string()])?;
            Ok(saved)
        })
    }
    pub fn replay(&self, input: &Value) -> Result<Option<Value>, FinanceError> {
        crate::config_receipts::replay(&self.store, "network", input)
    }
    pub fn verify_active_credentials(&self) -> Result<(), FinanceError> {
        let saved = self.store.with_read(|c| {
            c.query_row(
                "SELECT value FROM app_meta WHERE key='network_credential_revision'",
                [],
                |r| r.get::<_, String>(0),
            )
        })?;
        let saved: Value = serde_json::from_str(&saved).map_err(|_| invalid("代理凭据版本损坏"))?;
        if saved != self.credential_revision()? {
            return Err(invalid("代理凭据已变化，请重新验证代理"));
        }
        Ok(())
    }
}
fn invalid(s: &str) -> FinanceError {
    FinanceError::Invalid(s.into())
}
fn validate_input(v: &Value, keys: &[&str]) -> Result<(), FinanceError> {
    if !v
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && o.keys().all(|k| keys.contains(&k.as_str())))
    {
        return Err(invalid("网络配置字段无效"));
    }
    Ok(())
}
pub(crate) fn validate(v: &Value) -> Result<(), FinanceError> {
    validate_input(v, &["mode", "url", "username"])?;
    let url = v["url"].as_str().ok_or_else(|| invalid("代理地址无效"))?;
    let username = v["username"]
        .as_str()
        .filter(|s| s.len() <= 128 && !s.chars().any(char::is_control) && !s.contains(':'))
        .ok_or_else(|| invalid("代理用户名无效"))?;
    match v["mode"].as_str() {
        Some("direct") if url.is_empty() && username.is_empty() => Ok(()),
        Some("https_proxy") => {
            let parsed = url::Url::parse(url).map_err(|_| invalid("代理地址无效"))?;
            if url.len() > 512
                || !crate::channels::host_allowed(url)
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
                || parsed.path() != "/"
            {
                return Err(invalid("代理必须为公共 HTTPS 443 地址，不能包含凭据或路径"));
            }
            Ok(())
        }
        _ => Err(invalid("网络模式无效")),
    }
}
fn read(c: &rusqlite::Connection) -> Result<Value, FinanceError> {
    let text: Option<String> = c
        .query_row(
            "SELECT value FROM app_meta WHERE key='network_settings'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let mut value = match text {
        Some(s) => serde_json::from_str::<Value>(&s).map_err(|_| invalid("网络配置损坏"))?,
        None => json!({"version":0,"mode":"direct","url":"","username":""}),
    };
    if value["version"].as_i64().is_none_or(|v| v < 0) {
        return Err(invalid("网络版本损坏"));
    }
    let version = value
        .as_object_mut()
        .ok_or_else(|| invalid("网络配置损坏"))?
        .remove("version")
        .unwrap();
    validate(&value)?;
    value["version"] = version;
    Ok(value)
}
thread_local! {static CURRENT:RefCell<Option<Arc<Context>>>=const{RefCell::new(None)};}
thread_local! {static DEADLINE:RefCell<Option<Instant>>=const{RefCell::new(None)};}
pub(crate) struct DeadlineScope(Option<Instant>);
pub(crate) fn limit_duration(duration: Duration) -> DeadlineScope {
    let proposed = Instant::now() + duration;
    DeadlineScope(DEADLINE.with(|slot| {
        let previous = *slot.borrow();
        slot.replace(Some(previous.map_or(proposed, |old| old.min(proposed))));
        previous
    }))
}
pub(crate) fn remaining() -> Option<Duration> {
    DEADLINE.with(|slot| {
        slot.borrow()
            .map(|at| at.saturating_duration_since(Instant::now()))
    })
}
impl Drop for DeadlineScope {
    fn drop(&mut self) {
        DEADLINE.with(|slot| {
            slot.replace(self.0);
        });
    }
}
pub(crate) fn current() -> Option<Arc<Context>> {
    CURRENT.with(|c| c.borrow().clone())
}
pub(crate) struct Scope(Option<Arc<Context>>);
pub(crate) fn bind(context: Option<Arc<Context>>) -> Scope {
    Scope(CURRENT.with(|c| c.replace(context)))
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|c| {
            c.replace(self.0.take());
        });
    }
}
#[path = "network_transport.rs"]
mod transport;
pub(crate) use transport::{client, Client, Error};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_deadline_is_nested_and_released() {
        assert!(remaining().is_none());
        let outer = limit_duration(Duration::from_millis(10));
        {
            let _inner = limit_duration(Duration::from_secs(20));
            assert!(remaining().unwrap() <= Duration::from_millis(10));
            std::thread::sleep(Duration::from_millis(12));
            assert!(remaining().unwrap().is_zero());
        }
        assert!(remaining().unwrap().is_zero());
        drop(outer);
        assert!(remaining().is_none());
    }
    #[test]
    fn unverified_proxy_cannot_replace_direct_and_scope_is_session_owned() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let context = Context::new(store, None);
        assert_eq!(context.config().unwrap()["mode"], "direct");
        let input = json!({"config":{"mode":"https_proxy","url":"https://proxy.example.com/","username":""},"expectedVersion":0,"probeToken":"unverified","requestId":"proxy-save"});
        assert!(context.save(&input).is_err());
        assert_eq!(context.config().unwrap()["version"], 0);
        assert!(current().is_none());
        {
            let _scope = bind(Some(context.clone()));
            assert!(Arc::ptr_eq(&current().unwrap(), &context));
            {
                let _nested = bind(None);
                assert!(current().is_none());
            }
            assert!(current().is_some());
        }
        assert!(current().is_none());
        let probe = json!({"config":input["config"],"expectedVersion":0});
        let result = context
            .accept_probe(
                &probe,
                context.config().unwrap(),
                context.credential_revision().unwrap(),
            )
            .unwrap();
        let mut verified = input;
        verified["probeToken"] = result["probeToken"].clone();
        assert_eq!(context.save(&verified).unwrap()["mode"], "https_proxy");
        verified["config"]["url"] = json!("https://other.example.com/");
        assert!(context.save(&verified).is_err());
    }
    #[test]
    fn url_and_credentials_are_strictly_separated() {
        for url in [
            "http://example.com/",
            "https://u:p@example.com/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://169.254.169.254/",
            "https://example.com:8443/",
            "https://example.com/path",
            "https://example.com/?token=x",
            "https://example.com/#x",
        ] {
            assert!(
                validate(&json!({"mode":"https_proxy","url":url,"username":""})).is_err(),
                "{url}"
            );
        }
        assert!(
            validate(&json!({"mode":"direct","url":"","username":"","password":"NEVER"})).is_err()
        );
        assert!(
            validate(&json!({"mode":"direct","url":"https://example.com/","username":""})).is_err()
        );
    }
}
