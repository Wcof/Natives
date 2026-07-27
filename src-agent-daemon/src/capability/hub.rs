//! MCP Hub online browsing + explicit install (ADR-0016, decision 6).
//!
//! Read-only consumption of the official MCP Registry
//! (`https://registry.modelcontextprotocol.io`). Frozen boundary:
//! - browse is read-only; nothing local is uploaded except the search term;
//! - install requires an explicit user call and always lands as
//!   `trusted = false, enabled = false` (the UI re-confirms before enabling);
//! - search results are cached in `capability_mcp_hub_cache` (migration 021);
//!   when the network is unreachable we degrade to that cache and honestly
//!   mark the response `"stale": true` — a cache must never impersonate a
//!   live registry (ADR-0016 decision 6.4);
//! - stdio install commands are whitelisted to `npx` / `uvx` / `docker`;
//!   any other registry package type is rejected with guidance to configure
//!   the connector manually.

use super::store;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::time::Duration;

const DEFAULT_REGISTRY_BASE: &str = "https://registry.modelcontextprotocol.io";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Cached `get` payloads younger than this are served without a network trip.
const CACHE_TTL_SECONDS: i64 = 24 * 60 * 60;
const DEFAULT_PAGE_LIMIT: u32 = 30;
const MAX_PAGE_LIMIT: u32 = 100;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// One page of registry search results. Entries keep the registry wire shape
/// verbatim (`{"server": {...}, "_meta": {...}}`) so the cache never lies
/// about what the registry said.
#[derive(Debug, Clone)]
pub struct HubPage {
    pub entries: Vec<Value>,
    pub next_cursor: Option<String>,
}

/// Hub data source seam: production uses [`RegistryHttpClient`], tests inject
/// a fake (no real network in tests, ever).
#[async_trait::async_trait]
pub trait HubClient: Send + Sync {
    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<HubPage, String>;
    async fn get(&self, registry_name: &str) -> Result<Value, String>;
}

/// HTTP client for the official MCP Registry v0 API.
///
/// Wire shape (verified against the live API, 2026-07-26):
/// - `GET {base}/v0/servers?search=&cursor=&limit=&version=latest` →
///   `{"servers": [{"server": {...}, "_meta": {...}}],
///     "metadata": {"nextCursor": "...", "count": N}}`
/// - `GET {base}/v0/servers/{urlencoded name}/versions/latest` →
///   `{"server": {...}, "_meta": {...}}`
pub struct RegistryHttpClient {
    base: String,
    http: reqwest::Client,
}

impl RegistryHttpClient {
    /// Base URL comes from `NATIVES_MCP_REGISTRY_URL` when set (tests /
    /// private registries), the official registry otherwise.
    pub fn from_env() -> Self {
        let base = std::env::var("NATIVES_MCP_REGISTRY_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_REGISTRY_BASE.to_string());
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self { base, http }
    }
}

#[async_trait::async_trait]
impl HubClient for RegistryHttpClient {
    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<HubPage, String> {
        let url = format!("{}/v0/servers", self.base);
        let mut query_params: Vec<(&str, String)> = vec![
            ("limit", limit.clamp(1, MAX_PAGE_LIMIT).to_string()),
            // Only the latest version of each server is browsable.
            ("version", "latest".to_string()),
        ];
        if let Some(q) = query {
            query_params.push(("search", q.to_string()));
        }
        if let Some(c) = cursor {
            query_params.push(("cursor", c.to_string()));
        }
        let response = self
            .http
            .get(&url)
            .query(&query_params)
            .send()
            .await
            .map_err(|e| format!("registry request failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("registry returned HTTP {}", response.status()));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("registry response is not JSON: {e}"))?;
        Ok(parse_page(&body))
    }

    async fn get(&self, registry_name: &str) -> Result<Value, String> {
        let mut url = reqwest::Url::parse(&self.base)
            .map_err(|e| format!("invalid registry base url: {e}"))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| "registry base url cannot carry paths".to_string())?;
            segments.pop_if_empty();
            segments.extend(["v0", "servers"]);
            // Registry names are reverse-DNS with '/' — push() percent-encodes
            // the whole name into a single path segment.
            segments.push(registry_name);
            segments.extend(["versions", "latest"]);
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("registry request failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("registry returned HTTP {}", response.status()));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("registry response is not JSON: {e}"))?;
        if body.get("server").is_none() {
            return Err("registry response missing 'server' object".to_string());
        }
        Ok(body)
    }
}

/// Lenient list-response parse: unknown fields ignored, `nextCursor` /
/// `next_cursor` both accepted.
fn parse_page(body: &Value) -> HubPage {
    let entries = body
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let next_cursor = body
        .get("metadata")
        .and_then(|m| m.get("nextCursor").or_else(|| m.get("next_cursor")))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    HubPage {
        entries,
        next_cursor,
    }
}

// ---------------------------------------------------------------------------
// RPC surface (wired from capability::request)
// ---------------------------------------------------------------------------

/// `capability.mcp.hub.search` — `{query?, cursor?, limit?}` →
/// `{servers: [entry + installed], nextCursor, stale}`.
pub async fn search(params: &Value) -> Result<Value, String> {
    search_with(&RegistryHttpClient::from_env(), params).await
}

/// `capability.mcp.hub.get` — `{registryName}` →
/// `{server, _meta, installed, stale}`.
pub async fn get(params: &Value) -> Result<Value, String> {
    get_with(&RegistryHttpClient::from_env(), params).await
}

/// `capability.mcp.hub.install` — `{registryName, packageIndex? | remoteIndex?,
/// envOverrides?}` → the created connector row (always untrusted + disabled).
pub async fn install(params: &Value) -> Result<Value, String> {
    install_with(&RegistryHttpClient::from_env(), params).await
}

pub(crate) async fn search_with(client: &dyn HubClient, params: &Value) -> Result<Value, String> {
    let query = params
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let cursor = params
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);

    match client.search(query, cursor, limit).await {
        Ok(page) => {
            cache_upsert(&page.entries)?;
            let installed = installed_hub_refs()?;
            let servers: Vec<Value> = page
                .entries
                .iter()
                .map(|e| decorate(e, &installed))
                .collect();
            Ok(json!({
                "servers": servers,
                "nextCursor": page.next_cursor,
                "stale": false,
            }))
        }
        Err(network_error) => {
            // Degrade to the local cache — honestly flagged, never silent
            // (ADR-0016 decision 6.4). No cache at all → honest error that
            // points at the offline JSON import path.
            let cached = cache_list()?;
            if cached.is_empty() {
                return Err(format!(
                    "hub search failed and no local cache exists (use capability.mcp.importJson for offline setup): {network_error}"
                ));
            }
            let needle = query.map(str::to_ascii_lowercase);
            let installed = installed_hub_refs()?;
            let servers: Vec<Value> = cached
                .iter()
                .filter(|entry| matches_query(entry, needle.as_deref()))
                .map(|e| decorate(e, &installed))
                .collect();
            Ok(json!({
                "servers": servers,
                // Cursor pagination is a live-registry concept; the cache
                // fallback returns everything it has in one page.
                "nextCursor": Value::Null,
                "stale": true,
            }))
        }
    }
}

pub(crate) async fn get_with(client: &dyn HubClient, params: &Value) -> Result<Value, String> {
    let name = required_registry_name(params)?;
    let (entry, stale) = resolve_entry(client, name).await?;
    let installed = installed_hub_refs()?;
    let mut out = decorate(&entry, &installed);
    if let Some(obj) = out.as_object_mut() {
        obj.insert("stale".into(), json!(stale));
    }
    Ok(out)
}

pub(crate) async fn install_with(client: &dyn HubClient, params: &Value) -> Result<Value, String> {
    let name = required_registry_name(params)?;
    let (entry, _stale) = resolve_entry(client, name).await?;
    let server = entry.get("server").cloned().unwrap_or(entry);

    let package_index = params
        .get("packageIndex")
        .and_then(Value::as_u64)
        .map(|v| v as usize);
    let remote_index = params
        .get("remoteIndex")
        .and_then(Value::as_u64)
        .map(|v| v as usize);
    if package_index.is_some() && remote_index.is_some() {
        return Err("packageIndex and remoteIndex are mutually exclusive".into());
    }

    let display_name = server
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("registry entry missing server.name")?;

    let mut draft = Map::new();
    draft.insert("id".into(), json!(super::mcp::sanitize_id(display_name)));
    draft.insert("name".into(), json!(display_name));
    // Frozen install contract (ADR-0016 decision 6.1): never trusted, never
    // enabled — the user re-confirms in the UI before anything can run.
    draft.insert("trusted".into(), json!(false));
    draft.insert("enabled".into(), json!(false));
    draft.insert("source".into(), json!("hub"));
    draft.insert("hubRef".into(), json!(name));
    if let Some(env) = params.get("envOverrides").filter(|v| v.is_object()) {
        // Secret-like keys must be `secret:<id>` references — enforced by
        // mcp::create's validation, not re-implemented here.
        draft.insert("env".into(), env.clone());
    }

    let packages = server.get("packages").and_then(Value::as_array);
    let remotes = server.get("remotes").and_then(Value::as_array);
    let pick_package = |i: usize| -> Result<&Value, String> {
        packages
            .and_then(|p| p.get(i))
            .ok_or_else(|| format!("packageIndex {i} out of range"))
    };
    let pick_remote = |i: usize| -> Result<&Value, String> {
        remotes
            .and_then(|r| r.get(i))
            .ok_or_else(|| format!("remoteIndex {i} out of range"))
    };
    match (package_index, remote_index) {
        (Some(i), None) => apply_package(&mut draft, pick_package(i)?)?,
        (None, Some(i)) => apply_remote(&mut draft, pick_remote(i)?)?,
        (None, None) => {
            // Default preference: a remote needs no local process at all, so
            // it is the safer zero-install choice; fall back to package 0.
            if remotes.is_some_and(|r| !r.is_empty()) {
                apply_remote(&mut draft, pick_remote(0)?)?;
            } else if packages.is_some_and(|p| !p.is_empty()) {
                apply_package(&mut draft, pick_package(0)?)?;
            } else {
                return Err("registry entry has neither packages nor remotes".into());
            }
        }
        (Some(_), Some(_)) => unreachable!("guarded above"),
    }

    super::mcp::create(&Value::Object(draft))
}

/// Map a registry package onto a stdio connector draft. The command whitelist
/// is deliberately tiny: `npx` / `uvx` / `docker` only — arbitrary commands
/// from a remote registry are never executed.
fn apply_package(draft: &mut Map<String, Value>, package: &Value) -> Result<(), String> {
    let registry_type = package
        .get("registryType")
        .or_else(|| package.get("registry_type"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let identifier = package
        .get("identifier")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("registry package missing identifier")?;
    let version = package
        .get("version")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let (command, args): (&str, Vec<String>) = match registry_type.as_str() {
        "npm" => (
            "npx",
            vec![
                "-y".into(),
                match version {
                    Some(v) => format!("{identifier}@{v}"),
                    None => identifier.to_string(),
                },
            ],
        ),
        "pypi" => (
            "uvx",
            vec![match version {
                Some(v) => format!("{identifier}=={v}"),
                None => identifier.to_string(),
            }],
        ),
        "docker" | "oci" => {
            let image = match version {
                // A tagged/digested identifier keeps its own reference.
                Some(v) if !identifier.contains(':') => format!("{identifier}:{v}"),
                _ => identifier.to_string(),
            };
            (
                "docker",
                vec!["run".into(), "--rm".into(), "-i".into(), image],
            )
        }
        other => {
            return Err(format!(
                "registry type '{other}' is outside the stdio install whitelist (npx/uvx/docker); configure the connector manually via capability.mcp.create"
            ));
        }
    };
    draft.insert("transport".into(), json!("stdio"));
    draft.insert("command".into(), json!(command));
    draft.insert("args".into(), json!(args));
    Ok(())
}

/// Map a registry remote onto an http/sse connector draft.
fn apply_remote(draft: &mut Map<String, Value>, remote: &Value) -> Result<(), String> {
    let transport = match remote.get("type").and_then(Value::as_str) {
        Some("streamable-http") | Some("http") => "http",
        Some("sse") => "sse",
        other => {
            return Err(format!(
                "unsupported remote type: {}",
                other.unwrap_or("<missing>")
            ));
        }
    };
    let url = remote
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("registry remote missing url")?;
    if url.contains('{') {
        // Registry remotes may be URL templates ({variable} substitution);
        // installing one verbatim would produce a broken endpoint.
        return Err(
            "remote url contains template variables; configure the connector manually via capability.mcp.create".into(),
        );
    }
    draft.insert("transport".into(), json!(transport));
    draft.insert("url".into(), json!(url));
    Ok(())
}

/// Fresh cache → live registry → stale cache, in that order. Returns the
/// verbatim entry plus whether the caller must flag it stale.
async fn resolve_entry(client: &dyn HubClient, name: &str) -> Result<(Value, bool), String> {
    if let Some((payload, age_seconds)) = cache_get(name)? {
        if age_seconds <= CACHE_TTL_SECONDS {
            return Ok((payload, false));
        }
    }
    match client.get(name).await {
        Ok(entry) => {
            cache_upsert(std::slice::from_ref(&entry))?;
            Ok((entry, false))
        }
        Err(network_error) => match cache_get(name)? {
            Some((payload, _)) => Ok((payload, true)),
            None => Err(format!(
                "hub get '{name}' failed and no local cache exists: {network_error}"
            )),
        },
    }
}

// ---------------------------------------------------------------------------
// Cache + installed-flag helpers (capability_mcp_hub_cache, migration 021)
// ---------------------------------------------------------------------------

fn entry_registry_name(entry: &Value) -> Option<&str> {
    entry
        .get("server")
        .and_then(|s| s.get("name"))
        .and_then(Value::as_str)
        .or_else(|| entry.get("name").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

fn cache_upsert(entries: &[Value]) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let data = store()?;
    let conn = data.conn()?;
    let now = now_iso();
    for entry in entries {
        let Some(name) = entry_registry_name(entry) else {
            continue; // lenient: nameless entries are unaddressable, skip
        };
        let payload = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO capability_mcp_hub_cache (registry_name, payload_json, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(registry_name) DO UPDATE SET
                payload_json = excluded.payload_json,
                fetched_at = excluded.fetched_at",
            params![name, payload, now],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cache_list() -> Result<Vec<Value>, String> {
    let data = store()?;
    let conn = data.conn()?;
    let mut stmt = conn
        .prepare("SELECT payload_json FROM capability_mcp_hub_cache ORDER BY registry_name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .filter_map(|raw| serde_json::from_str(raw).ok())
        .collect())
}

/// Returns the cached entry plus its age in seconds (unparsable timestamps
/// count as expired, never as fresh).
fn cache_get(name: &str) -> Result<Option<(Value, i64)>, String> {
    let data = store()?;
    let conn = data.conn()?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_json, fetched_at FROM capability_mcp_hub_cache WHERE registry_name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((payload, fetched_at)) = row else {
        return Ok(None);
    };
    let payload: Value = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(_) => return Ok(None), // corrupt cache row = no cache
    };
    let age_seconds = chrono::DateTime::parse_from_rfc3339(&fetched_at)
        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
        .unwrap_or(i64::MAX);
    Ok(Some((payload, age_seconds)))
}

/// hub_refs of already-installed connectors, for the `installed` flag.
fn installed_hub_refs() -> Result<HashSet<String>, String> {
    let data = store()?;
    let conn = data.conn()?;
    let mut stmt = conn
        .prepare("SELECT hub_ref FROM capability_mcp_server WHERE hub_ref IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Verbatim entry + top-level `installed` flag, without touching the wire shape.
fn decorate(entry: &Value, installed: &HashSet<String>) -> Value {
    let is_installed = entry_registry_name(entry)
        .map(|name| installed.contains(name))
        .unwrap_or(false);
    let mut out = entry.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("installed".into(), json!(is_installed));
    }
    out
}

fn matches_query(entry: &Value, needle: Option<&str>) -> bool {
    let Some(needle) = needle else { return true };
    let server = entry.get("server").unwrap_or(entry);
    ["name", "title", "description"].iter().any(|key| {
        server
            .get(*key)
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase().contains(needle))
            .unwrap_or(false)
    })
}

fn required_registry_name(params: &Value) -> Result<&str, String> {
    params
        .get("registryName")
        .or_else(|| params.get("registry_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "registryName required".to_string())
}

// ---------------------------------------------------------------------------
// Tests — FakeHubClient only, no real network (ADR-0016 decision 6)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::future::Future;
    use uuid::Uuid;

    /// Async twin of capability/tests.rs `with_temp_db`: thread-local DB
    /// override + re-entrant env lock; tokio's current-thread test runtime
    /// keeps everything on this thread so the override stays visible.
    async fn with_temp_db<F, Fut>(f: F)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ()>,
    {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("hub-{}.db", Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("hub temp db migrate");
        f().await;
        crate::storage::set_test_db_override(None, None);
    }

    struct FakeHubClient {
        fail: bool,
        entries: Vec<Value>,
        next_cursor: Option<String>,
    }

    impl FakeHubClient {
        fn ok(entries: Vec<Value>, next_cursor: Option<&str>) -> Self {
            Self {
                fail: false,
                entries,
                next_cursor: next_cursor.map(str::to_string),
            }
        }
        fn down() -> Self {
            Self {
                fail: true,
                entries: Vec::new(),
                next_cursor: None,
            }
        }
    }

    #[async_trait::async_trait]
    impl HubClient for FakeHubClient {
        async fn search(
            &self,
            _query: Option<&str>,
            _cursor: Option<&str>,
            _limit: u32,
        ) -> Result<HubPage, String> {
            if self.fail {
                return Err("network down".into());
            }
            Ok(HubPage {
                entries: self.entries.clone(),
                next_cursor: self.next_cursor.clone(),
            })
        }

        async fn get(&self, registry_name: &str) -> Result<Value, String> {
            if self.fail {
                return Err("network down".into());
            }
            self.entries
                .iter()
                .find(|e| entry_registry_name(e) == Some(registry_name))
                .cloned()
                .ok_or_else(|| format!("not found: {registry_name}"))
        }
    }

    fn npm_entry() -> Value {
        json!({
            "server": {
                "name": "io.github.acme/files",
                "title": "Acme Files",
                "description": "File tools over MCP",
                "version": "1.2.3",
                "packages": [{
                    "registryType": "npm",
                    "identifier": "@acme/files-mcp",
                    "version": "1.2.3",
                    "transport": { "type": "stdio" }
                }]
            },
            "_meta": { "io.modelcontextprotocol.registry/official": { "isLatest": true } }
        })
    }

    fn remote_entry() -> Value {
        json!({
            "server": {
                "name": "com.example/docs",
                "description": "Hosted docs server",
                "version": "2.0.0",
                "remotes": [{ "type": "streamable-http", "url": "https://docs.example.com/mcp" }]
            },
            "_meta": {}
        })
    }

    fn cargo_entry() -> Value {
        json!({
            "server": {
                "name": "io.github.acme/rusty",
                "description": "Cargo-only server",
                "version": "0.1.0",
                "packages": [{
                    "registryType": "cargo",
                    "identifier": "rusty-mcp",
                    "version": "0.1.0"
                }]
            },
            "_meta": {}
        })
    }

    #[tokio::test]
    async fn search_upserts_cache_and_flags_not_installed() {
        with_temp_db(|| async {
            let client = FakeHubClient::ok(vec![npm_entry(), remote_entry()], Some("cur-2"));
            let out = search_with(&client, &json!({ "query": "files", "limit": 10 }))
                .await
                .unwrap();
            assert_eq!(out["stale"], false);
            assert_eq!(out["nextCursor"], "cur-2");
            let servers = out["servers"].as_array().unwrap();
            assert_eq!(servers.len(), 2);
            assert!(servers.iter().all(|s| s["installed"] == false));

            let data = store().unwrap();
            let conn = data.conn().unwrap();
            let cached: i64 = conn
                .query_row("SELECT COUNT(*) FROM capability_mcp_hub_cache", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(cached, 2);
        })
        .await;
    }

    #[tokio::test]
    async fn search_network_failure_degrades_to_stale_cache() {
        with_temp_db(|| async {
            // No cache yet: failure must be honest, not an empty fake page.
            let err = search_with(&FakeHubClient::down(), &json!({}))
                .await
                .unwrap_err();
            assert!(err.contains("no local cache"), "{err}");

            // Seed the cache with a live page, then lose the network.
            search_with(
                &FakeHubClient::ok(vec![npm_entry(), remote_entry()], None),
                &json!({}),
            )
            .await
            .unwrap();
            let out = search_with(&FakeHubClient::down(), &json!({}))
                .await
                .unwrap();
            assert_eq!(out["stale"], true);
            assert_eq!(out["nextCursor"], Value::Null);
            assert_eq!(out["servers"].as_array().unwrap().len(), 2);

            // Cache fallback still filters by the query string.
            let out = search_with(&FakeHubClient::down(), &json!({ "query": "docs" }))
                .await
                .unwrap();
            let servers = out["servers"].as_array().unwrap();
            assert_eq!(servers.len(), 1);
            assert_eq!(servers[0]["server"]["name"], "com.example/docs");
        })
        .await;
    }

    #[tokio::test]
    async fn install_npm_package_maps_to_npx_untrusted_disabled() {
        with_temp_db(|| async {
            let client = FakeHubClient::ok(vec![npm_entry()], None);
            let out = install_with(
                &client,
                &json!({ "registryName": "io.github.acme/files", "packageIndex": 0 }),
            )
            .await
            .unwrap();
            let server = &out["server"];
            assert_eq!(server["id"], "io-github-acme-files");
            assert_eq!(server["transport"], "stdio");
            assert_eq!(server["command"], "npx");
            assert_eq!(server["args"], json!(["-y", "@acme/files-mcp@1.2.3"]));
            assert_eq!(server["trusted"], false, "hub installs are never trusted");
            assert_eq!(
                server["enabled"], false,
                "hub installs need explicit enabling"
            );
            assert_eq!(server["source"], "hub");
            assert_eq!(server["hubRef"], "io.github.acme/files");

            // Search now reports the entry as installed.
            let out = search_with(&client, &json!({})).await.unwrap();
            let entry = &out["servers"].as_array().unwrap()[0];
            assert_eq!(entry["installed"], true);
        })
        .await;
    }

    #[tokio::test]
    async fn install_remote_maps_streamable_http_to_http_transport() {
        with_temp_db(|| async {
            let client = FakeHubClient::ok(vec![remote_entry()], None);
            let out = install_with(
                &client,
                &json!({ "registryName": "com.example/docs", "remoteIndex": 0 }),
            )
            .await
            .unwrap();
            let server = &out["server"];
            assert_eq!(server["transport"], "http");
            assert_eq!(server["url"], "https://docs.example.com/mcp");
            assert_eq!(server["trusted"], false);
            assert_eq!(server["enabled"], false);
        })
        .await;
    }

    #[tokio::test]
    async fn install_rejects_non_whitelisted_registry_type() {
        with_temp_db(|| async {
            let client = FakeHubClient::ok(vec![cargo_entry()], None);
            let err = install_with(
                &client,
                &json!({ "registryName": "io.github.acme/rusty", "packageIndex": 0 }),
            )
            .await
            .unwrap_err();
            assert!(err.contains("whitelist"), "{err}");
            // Nothing must have been created.
            let listed = super::super::mcp::list(&json!({})).unwrap();
            assert!(listed["servers"].as_array().unwrap().is_empty());
        })
        .await;
    }

    #[tokio::test]
    async fn install_uses_stale_cache_when_network_is_down() {
        with_temp_db(|| async {
            // Seed via a live search, then install with the network gone.
            search_with(&FakeHubClient::ok(vec![npm_entry()], None), &json!({}))
                .await
                .unwrap();
            let out = install_with(
                &FakeHubClient::down(),
                &json!({ "registryName": "io.github.acme/files" }),
            )
            .await
            .unwrap();
            assert_eq!(out["server"]["command"], "npx");

            // Unknown name with no cache stays an honest error.
            let err = install_with(
                &FakeHubClient::down(),
                &json!({ "registryName": "com.example/ghost" }),
            )
            .await
            .unwrap_err();
            assert!(err.contains("no local cache"), "{err}");
        })
        .await;
    }
}
