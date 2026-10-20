//! Model Host 8766 窄接口 HTTP 客户端。
//!
//! 职责：只与 Model Host 8766 交互，严格遵守窄接口 Contract，
//! 严禁私自探测路径、读取 SQLite 数据库或解密 Keychain 凭据。

use super::dto::*;
use super::error::ModelQueryError;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

fn read_body(response: ureq::Response) -> Result<String, ModelQueryError> {
    let mut body = String::new();
    response
        .into_reader()
        .take(512 * 1024 + 1)
        .read_to_string(&mut body)
        .map_err(|_| ModelQueryError::Unavailable("response could not be read".into()))?;
    if body.len() > 512 * 1024 {
        return Err(ModelQueryError::Unavailable(
            "response exceeds size limit".into(),
        ));
    }
    Ok(body)
}

#[derive(Clone, Debug)]
pub struct ModelUsageQueryClient {
    base_url: String,
    agent: ureq::Agent,
    cache: Arc<super::cache::QueryCache>,
}

impl Default for ModelUsageQueryClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelUsageQueryClient {
    pub fn new() -> Self {
        let base_url = std::env::var("NATIVES_MODEL_HOST_AIQUERY_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8766".to_string());
        Self::with_base_url(base_url)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(1500))
            .timeout_read(Duration::from_millis(5000))
            .timeout_write(Duration::from_millis(2000))
            .build();
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            agent,
            cache: Default::default(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        subpath: &str,
    ) -> Result<T, ModelQueryError> {
        let body = self.get_raw(subpath)?;
        serde_json::from_str::<T>(&body)
            .map_err(|e| ModelQueryError::Parse(format!("反序列化 {subpath} 响应失败: {e}")))
    }

    pub fn get_raw(&self, subpath: &str) -> Result<String, ModelQueryError> {
        self.cache.get(subpath, || self.get_uncached(subpath))
    }

    fn get_uncached(&self, subpath: &str) -> Result<String, ModelQueryError> {
        let url = format!("{}{}", self.base_url, subpath);
        let resp = match self
            .agent
            .get(&url)
            .set("Accept", "application/json")
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, resp)) => {
                let text = read_body(resp)?;
                return Err(ModelQueryError::Status(code, text));
            }
            Err(e) => {
                return Err(ModelQueryError::Unavailable(e.to_string()));
            }
        };

        read_body(resp)
    }

    pub fn post_raw(&self, subpath: &str, body: &str) -> Result<String, ModelQueryError> {
        self.cache.invalidate();
        let url = format!("{}{}", self.base_url, subpath);
        let resp = match self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json")
            .send_string(body)
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, resp)) => {
                let text = read_body(resp)?;
                return Err(ModelQueryError::Status(code, text));
            }
            Err(e) => {
                return Err(ModelQueryError::Unavailable(e.to_string()));
            }
        };

        let body = read_body(resp)?;
        self.cache.invalidate();
        Ok(body)
    }

    pub fn healthz(&self) -> Result<bool, ModelQueryError> {
        let raw = self.get_raw("/ai/healthz")?;
        Ok(raw.contains("\"ok\""))
    }

    pub fn get_overview(&self, query: Option<&str>) -> Result<OverviewResultDTO, ModelQueryError> {
        let path = match query {
            Some(q) if !q.is_empty() => format!("/ai/usage/overview?{q}"),
            _ => "/ai/usage/overview".to_string(),
        };
        self.get_json(&path)
    }

    pub fn get_analysis(&self, query: Option<&str>) -> Result<AnalyticsResultDTO, ModelQueryError> {
        let path = match query {
            Some(q) if !q.is_empty() => format!("/ai/usage/analysis?{q}"),
            _ => "/ai/usage/analysis".to_string(),
        };
        self.get_json(&path)
    }

    pub fn get_sessions(&self, query: Option<&str>) -> Result<SessionsResultDTO, ModelQueryError> {
        let path = match query {
            Some(q) if !q.is_empty() => format!("/ai/usage/sessions?{q}"),
            _ => "/ai/usage/sessions".to_string(),
        };
        self.get_json(&path)
    }

    pub fn get_events(&self, query: Option<&str>) -> Result<EventsResultDTO, ModelQueryError> {
        let path = match query {
            Some(q) if !q.is_empty() => format!("/ai/usage/events?{q}"),
            _ => "/ai/usage/events".to_string(),
        };
        self.get_json(&path)
    }

    pub fn get_sources(&self) -> Result<SourcesResultDTO, ModelQueryError> {
        self.get_json("/ai/usage/sources")
    }

    pub fn get_budgets(&self) -> Result<serde_json::Value, ModelQueryError> {
        self.get_json("/ai/usage/budgets")
    }

    pub fn get_billing(&self, query: Option<&str>) -> Result<serde_json::Value, ModelQueryError> {
        let path = match query {
            Some(q) if !q.is_empty() => format!("/ai/usage/billing?{q}"),
            _ => "/ai/usage/billing".to_string(),
        };
        self.get_json(&path)
    }

    pub fn get_attention(&self, query: Option<&str>) -> Result<serde_json::Value, ModelQueryError> {
        let path = match query {
            Some(q) if !q.is_empty() => format!("/ai/usage/attention?{q}"),
            _ => "/ai/usage/attention".to_string(),
        };
        self.get_json(&path)
    }

    pub fn get_insights(&self) -> Result<serde_json::Value, ModelQueryError> {
        self.get_json("/ai/usage/insights")
    }

    pub fn get_pricing(&self) -> Result<serde_json::Value, ModelQueryError> {
        self.get_json("/ai/usage/pricing")
    }

    pub fn get_quota(&self) -> Result<QuotaResultDTO, ModelQueryError> {
        self.get_json("/ai/quota")
    }

    pub fn refresh_quota(&self) -> Result<serde_json::Value, ModelQueryError> {
        let raw = self.post_raw("/ai/quota/refresh", "{}")?;
        serde_json::from_str(&raw)
            .map_err(|e| ModelQueryError::Parse(format!("反序列化 quota refresh 失败: {e}")))
    }
}
