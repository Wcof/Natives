//! 真实模型发现服务（API-003 / plan3 01-reference-audit §2）。
//!
//! 通过 GET /v1/models 或自定义 `models_url` 端点探测可用模型列表。
//! 支持 OpenAI, Anthropic (x-api-key), Gemini (x-goog-api-key) 认证头与候选中转 URL 自动回退。

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

use super::model::{ModelAvailability, ModelSource};

const FETCH_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    pub id: String,
    pub display_name: String,
    pub owned_by: Option<String>,
    pub source: ModelSource,
    pub availability: ModelAvailability,
    pub capabilities: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Option<Vec<OpenAiModelEntry>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
    owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelsResponse {
    data: Option<Vec<AnthropicModelEntry>>,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelEntry {
    id: String,
    display_name: Option<String>,
}

/// 已知的 Anthropic / 代理兼容子路径后缀（最长前缀优先匹配）
const KNOWN_COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

/// 构建模型发现的候选 URL 列表
pub fn build_models_url_candidates(
    base_url: &str,
    models_url_override: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(override_url) = models_url_override {
        let trimmed = override_url.trim();
        if !trimmed.is_empty() {
            return Ok(vec![trimmed.to_string()]);
        }
    }

    let raw = base_url.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("base_url is required".into());
    }

    let mut candidates = Vec::new();

    // 1. 如果以 /v1 结尾
    if raw.ends_with("/v1") {
        candidates.push(format!("{raw}/models"));
    } else {
        // 2. 尝试追加 /v1/models 和 /models
        candidates.push(format!("{raw}/v1/models"));
        candidates.push(format!("{raw}/models"));
    }

    // 3. 剥离兼容子路径后缀
    for suffix in KNOWN_COMPAT_SUFFIXES {
        if let Some(stripped) = raw.strip_suffix(suffix) {
            let s = stripped.trim_end_matches('/');
            if !s.is_empty() {
                if s.ends_with("/v1") {
                    candidates.push(format!("{s}/models"));
                } else {
                    candidates.push(format!("{s}/v1/models"));
                    candidates.push(format!("{s}/models"));
                }
            }
        }
    }

    candidates.dedup();
    Ok(candidates)
}

/// 构建模型发现请求头
pub fn build_model_fetch_headers(
    api_key: &str,
    custom_headers_json: Option<&str>,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Natives-AI-Platform/1.0"),
    );

    let key_trimmed = api_key.trim();
    if !key_trimmed.is_empty() {
        // 标准 Bearer 认证
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {key_trimmed}")) {
            headers.insert(AUTHORIZATION, val);
        }
        // Anthropic x-api-key 认证
        if let Ok(val) = HeaderValue::from_str(key_trimmed) {
            if let Ok(name) = HeaderName::from_bytes(b"x-api-key") {
                headers.insert(name, val.clone());
            }
            if let Ok(name) = HeaderName::from_bytes(b"x-goog-api-key") {
                headers.insert(name, val.clone());
            }
            if let Ok(name) = HeaderName::from_bytes(b"anthropic-version") {
                headers.insert(name, HeaderValue::from_static("2023-06-01"));
            }
        }
    }

    // 解析自定义非敏感 headers
    if let Some(raw_json) = custom_headers_json {
        if !raw_json.trim().is_empty() {
            if let Ok(map) = serde_json::from_str::<BTreeMap<String, String>>(raw_json) {
                for (k, v) in map {
                    let k_lower = k.to_ascii_lowercase();
                    // 过滤 Authorization / api-key 类敏感 header（应通过 api_key 传入）
                    if k_lower == "authorization"
                        || k_lower == "x-api-key"
                        || k_lower == "x-goog-api-key"
                    {
                        continue;
                    }
                    if let (Ok(name), Ok(val)) = (
                        HeaderName::from_bytes(k.as_bytes()),
                        HeaderValue::from_str(&v),
                    ) {
                        headers.insert(name, val);
                    }
                }
            }
        }
    }

    Ok(headers)
}

/// 执行真实模型发现请求
pub async fn discover_models(
    base_url: &str,
    api_key: &str,
    models_url_override: Option<&str>,
    custom_headers_json: Option<&str>,
) -> Result<Vec<DiscoveredModel>, String> {
    let candidates = build_models_url_candidates(base_url, models_url_override)?;
    let headers = build_model_fetch_headers(api_key, custom_headers_json)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut last_err = None;

    for url in &candidates {
        let resp = match client.get(url).headers(headers.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(format!("Request failed to {url}: {e}"));
                continue;
            }
        };

        let status = resp.status();

        if status.is_success() {
            let body_text = resp
                .text()
                .await
                .map_err(|e| format!("Failed to read response body: {e}"))?;

            // 1. 尝试解析 OpenAI 兼容格式
            if let Ok(openai_resp) = serde_json::from_str::<OpenAiModelsResponse>(&body_text) {
                if let Some(entries) = openai_resp.data {
                    if !entries.is_empty() {
                        let mut models: Vec<DiscoveredModel> = entries
                            .into_iter()
                            .map(|e| DiscoveredModel {
                                id: e.id.clone(),
                                display_name: e.id,
                                owned_by: e.owned_by,
                                source: ModelSource::Discovered,
                                availability: ModelAvailability::Available,
                                capabilities: None,
                            })
                            .collect();
                        models.sort_by(|a, b| a.id.cmp(&b.id));
                        models.dedup_by(|a, b| a.id == b.id);
                        return Ok(models);
                    }
                }
            }

            // 2. 尝试解析 Anthropic 兼容格式
            if let Ok(anthropic_resp) = serde_json::from_str::<AnthropicModelsResponse>(&body_text)
            {
                if let Some(entries) = anthropic_resp.data {
                    if !entries.is_empty() {
                        let mut models: Vec<DiscoveredModel> = entries
                            .into_iter()
                            .map(|e| DiscoveredModel {
                                id: e.id.clone(),
                                display_name: e.display_name.unwrap_or_else(|| e.id.clone()),
                                owned_by: Some("anthropic".into()),
                                source: ModelSource::Discovered,
                                availability: ModelAvailability::Available,
                                capabilities: None,
                            })
                            .collect();
                        models.sort_by(|a, b| a.id.cmp(&b.id));
                        models.dedup_by(|a, b| a.id == b.id);
                        return Ok(models);
                    }
                }
            }

            // 3. 尝试解析纯字符串数组或简单对象数组
            if let Ok(raw_list) = serde_json::from_str::<Vec<serde_json::Value>>(&body_text) {
                let mut models = Vec::new();
                for item in raw_list {
                    if let Some(id_str) = item.as_str() {
                        models.push(DiscoveredModel {
                            id: id_str.to_string(),
                            display_name: id_str.to_string(),
                            owned_by: None,
                            source: ModelSource::Discovered,
                            availability: ModelAvailability::Available,
                            capabilities: None,
                        });
                    } else if let Some(id_str) = item.get("id").and_then(|v| v.as_str()) {
                        let display = item
                            .get("display_name")
                            .or_else(|| item.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(id_str);
                        models.push(DiscoveredModel {
                            id: id_str.to_string(),
                            display_name: display.to_string(),
                            owned_by: item
                                .get("owned_by")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            source: ModelSource::Discovered,
                            availability: ModelAvailability::Available,
                            capabilities: None,
                        });
                    }
                }
                if !models.is_empty() {
                    models.sort_by(|a, b| a.id.cmp(&b.id));
                    models.dedup_by(|a, b| a.id == b.id);
                    return Ok(models);
                }
            }

            last_err = Some("Response status was 200, but data format is not recognized".into());
        } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(format!("Authentication failed ({status})"));
        } else if status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED {
            last_err = Some(format!("Endpoint {url} returned {status}"));
            continue;
        } else {
            last_err = Some(format!("Endpoint {url} returned error status {status}"));
        }
    }

    Err(last_err.unwrap_or_else(|| "Failed to discover models on all candidate URLs".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_generation_standard() {
        let c1 = build_models_url_candidates("https://api.openai.com/v1", None).unwrap();
        assert_eq!(c1, vec!["https://api.openai.com/v1/models"]);

        let c2 = build_models_url_candidates("https://api.deepseek.com", None).unwrap();
        assert_eq!(
            c2,
            vec![
                "https://api.deepseek.com/v1/models",
                "https://api.deepseek.com/models"
            ]
        );
    }

    #[test]
    fn candidates_strip_compat_suffixes() {
        let c = build_models_url_candidates("https://api.example.com/anthropic", None).unwrap();
        assert!(c.contains(&"https://api.example.com/anthropic/v1/models".to_string()));
        assert!(c.contains(&"https://api.example.com/v1/models".to_string()));
    }

    #[test]
    fn candidates_override_wins() {
        let c = build_models_url_candidates(
            "https://api.example.com",
            Some("https://api.custom.com/my-models"),
        )
        .unwrap();
        assert_eq!(c, vec!["https://api.custom.com/my-models"]);
    }
}
