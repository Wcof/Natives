//! MCP resource surface (ARCH-002).
//!
//! `resources/list`, templates, `resources/read` (discovery allowlist + scheme
//! policy + byte cap), prompts, and client roots. Extracted from
//! `mcp_runtime.rs`.

use std::path::PathBuf;
use std::time::Duration;

use super::*;

impl McpRuntime {
    pub fn list_resources(&self, server_id: &str, cursor: Option<&str>) -> Result<Value, McpError> {
        self.require_capability(server_id, "resources")?;
        let mut params = json!({});
        if let Some(c) = cursor {
            params["cursor"] = json!(c);
        }
        let result = self.request(server_id, "resources/list", params, Duration::from_secs(15))?;
        let items = result
            .get("resources")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Ok(mut cache) = self.resources.lock() {
            let entry = cache.entry(server_id.to_string()).or_default();
            if cursor.is_none() {
                entry.clear();
            }
            for item in &items {
                if let Some(uri) = item.get("uri").and_then(|v| v.as_str()) {
                    if !entry
                        .iter()
                        .any(|e| e.get("uri").and_then(|v| v.as_str()) == Some(uri))
                    {
                        entry.push(item.clone());
                    }
                }
            }
        }
        Ok(json!({
            "server_id": server_id,
            "resources": items,
            "next_cursor": result.get("nextCursor").cloned().unwrap_or(Value::Null),
        }))
    }

    /// `resources/templates/list`. Caches templates for allowlist matching.
    pub fn list_resource_templates(&self, server_id: &str) -> Result<Value, McpError> {
        self.require_capability(server_id, "resources")?;
        let result = self.request(
            server_id,
            "resources/templates/list",
            json!({}),
            Duration::from_secs(15),
        )?;
        let items = result
            .get("resourceTemplates")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Ok(mut cache) = self.resource_templates.lock() {
            cache.insert(server_id.to_string(), items.clone());
        }
        Ok(json!({
            "server_id": server_id,
            "resource_templates": items,
            "next_cursor": result.get("nextCursor").cloned().unwrap_or(Value::Null),
        }))
    }

    /// `resources/read`, wrapped in a provenance envelope.
    ///
    /// See the module docs 第 2 节 for why this is the most tightly bounded call
    /// in the file. The returned contents are untrusted server output; the
    /// envelope says so explicitly so nothing downstream has to infer it.
    pub fn read_resource(&self, server_id: &str, uri: &str) -> Result<Value, McpError> {
        if uri.trim().is_empty() {
            return Err(McpError::Invalid("resource uri required".into()));
        }
        self.require_capability(server_id, "resources")?;
        let config = self.server_config(server_id).map_err(McpError::NotFound)?;
        let matched_by = self.assert_resource_uri_allowed(&config, uri)?;

        let result = self.request(
            server_id,
            "resources/read",
            json!({ "uri": uri }),
            Duration::from_secs(30),
        )?;

        let max_bytes = resource_max_bytes();
        let mut truncated = false;
        let contents: Vec<Value> = result
            .get("contents")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut item| {
                // Text is capped on a char boundary. `blob` is base64 and is
                // never decoded here — we only measure and cap it.
                let text_len = item.get("text").and_then(|v| v.as_str()).map(str::len);
                if text_len.is_some_and(|len| len > max_bytes) {
                    let capped = item
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|t| truncate_on_char_boundary(t, max_bytes))
                        .unwrap_or_default();
                    truncated = true;
                    item["text"] = json!(capped);
                    item["truncated"] = json!(true);
                }
                let blob_len = item.get("blob").and_then(|v| v.as_str()).map(str::len);
                if let Some(len) = blob_len {
                    item["blob_bytes"] = json!(len);
                    if len > max_bytes {
                        truncated = true;
                        item["blob"] = Value::Null;
                        item["truncated"] = json!(true);
                        item["dropped_reason"] = json!("blob exceeds resource byte cap");
                    }
                }
                item
            })
            .collect();

        Ok(json!({
            "server_id": server_id,
            "uri": uri,
            // Load-bearing for anything that later puts this in a model context.
            "untrusted": true,
            "origin": "mcp_resource",
            "allowlist_match": matched_by,
            "truncated": truncated,
            "max_bytes": max_bytes,
            "contents": contents,
        }))
    }

    /// The `resources/read` gate. Returns how the URI was allowed, for audit.
    ///
    /// Rules, in order (see module docs 第 2 节):
    /// 1. dangerous schemes are refused for everyone;
    /// 2. `file:` needs a trusted server, no `..`, and no remote authority;
    /// 3. the URI must be one this server published, or match a published template.
    pub(crate) fn assert_resource_uri_allowed(
        &self,
        config: &McpServerConfig,
        uri: &str,
    ) -> Result<String, McpError> {
        let lowered = uri.trim().to_ascii_lowercase();

        // 1. Code / inline-payload carriers have no resource meaning here.
        for scheme in ["javascript:", "data:", "vbscript:", "blob:"] {
            if lowered.starts_with(scheme) {
                return Err(McpError::Denied(format!(
                    "resource scheme `{scheme}` is never readable"
                )));
            }
        }

        // 2. Local filesystem reads. An untrusted server must not be able to
        //    name a path at all; a trusted one still may not traverse or point
        //    at another host.
        if lowered.starts_with("file:") {
            if !config.trusted {
                return Err(McpError::Denied(
                    "file:// resources are blocked for untrusted MCP servers".into(),
                ));
            }
            if uri.contains("..") {
                return Err(McpError::Denied(
                    "file:// resource path traversal (`..`) blocked".into(),
                ));
            }
            let after_scheme = &uri[5..];
            // `file://host/path` — anything but an empty or `localhost` authority
            // is a remote fetch wearing a local scheme.
            if let Some(rest) = after_scheme.strip_prefix("//") {
                let authority = rest.split('/').next().unwrap_or("");
                if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
                    return Err(McpError::Denied(format!(
                        "file:// resource with non-local authority `{authority}` blocked"
                    )));
                }
            }
        }

        // 3. Discovery allowlist. Same principle as `call_tool`: the reachable
        //    set is what the server published, not what a caller can type.
        let listed = self
            .resources
            .lock()
            .ok()
            .and_then(|m| m.get(&config.id).cloned())
            .unwrap_or_default();
        if listed
            .iter()
            .any(|r| r.get("uri").and_then(|v| v.as_str()) == Some(uri))
        {
            return Ok("listed".into());
        }

        let templates = self
            .resource_templates
            .lock()
            .ok()
            .and_then(|m| m.get(&config.id).cloned())
            .unwrap_or_default();
        for tpl in &templates {
            if let Some(pattern) = tpl.get("uriTemplate").and_then(|v| v.as_str()) {
                if uri_matches_template(uri, pattern) {
                    return Ok(format!("template:{pattern}"));
                }
            }
        }

        Err(McpError::Denied(format!(
            "resource `{uri}` was not published by `{}` — call mcp.resources.list \
             (and mcp.resources.templates.list) first; arbitrary URIs are not readable",
            config.id
        )))
    }

    // ---------------------------------------------------------------------
    // 第 6 节 — Prompts
    // ---------------------------------------------------------------------

    /// `prompts/list`.
    pub fn list_prompts(&self, server_id: &str, cursor: Option<&str>) -> Result<Value, McpError> {
        self.require_capability(server_id, "prompts")?;
        let mut params = json!({});
        if let Some(c) = cursor {
            params["cursor"] = json!(c);
        }
        let result = self.request(server_id, "prompts/list", params, Duration::from_secs(15))?;
        Ok(json!({
            "server_id": server_id,
            "prompts": result.get("prompts").cloned().unwrap_or_else(|| json!([])),
            "next_cursor": result.get("nextCursor").cloned().unwrap_or(Value::Null),
        }))
    }

    /// `prompts/get`. The rendered messages are server-authored text destined for
    /// a model context, so they carry the same provenance envelope as resources.
    pub fn get_prompt(
        &self,
        server_id: &str,
        name: &str,
        arguments: Value,
    ) -> Result<Value, McpError> {
        if name.trim().is_empty() {
            return Err(McpError::Invalid("prompt name required".into()));
        }
        self.require_capability(server_id, "prompts")?;
        let mut params = json!({ "name": name });
        if !arguments.is_null() {
            params["arguments"] = arguments;
        }
        let result = self.request(server_id, "prompts/get", params, Duration::from_secs(30))?;
        Ok(json!({
            "server_id": server_id,
            "name": name,
            "untrusted": true,
            "origin": "mcp_prompt",
            "description": result.get("description").cloned().unwrap_or(Value::Null),
            "messages": result.get("messages").cloned().unwrap_or_else(|| json!([])),
        }))
    }

    // ---------------------------------------------------------------------
    // 第 7 节 — Roots (client-side obligation)
    // ---------------------------------------------------------------------

    /// Replace the granted root set. Entries that are not existing absolute
    /// directories are rejected rather than trimmed, so a caller never believes
    /// it granted something it did not.
    pub fn set_roots(&self, paths: &[String]) -> Result<Vec<McpRoot>, McpError> {
        let mut roots = Vec::new();
        for raw in paths {
            roots.push(validate_root(raw)?);
        }
        let snapshot = roots.clone();
        self.roots
            .lock()
            .map_err(|e| McpError::Transport(e.to_string()))?
            .replace(roots);
        Ok(snapshot)
    }

    /// Roots we would answer `roots/list` with.
    ///
    /// Source order: an explicit [`set_roots`](Self::set_roots) wins; otherwise
    /// `NATIVES_MCP_ROOTS` (a `:`-separated path list). Empty means no roots
    /// granted — the fail-closed default, and an honest answer rather than a
    /// silent fallback to the process cwd.
    pub fn client_roots(&self) -> Vec<McpRoot> {
        self.effective_roots()
            .into_iter()
            .filter_map(|v| {
                Some(McpRoot {
                    uri: v.get("uri")?.as_str()?.to_string(),
                    name: v
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect()
    }

    /// Wire form of the roots, as `roots/list` returns them.
    pub(crate) fn effective_roots(&self) -> Vec<Value> {
        if let Ok(guard) = self.roots.lock() {
            if let Some(explicit) = guard.as_ref() {
                return explicit
                    .iter()
                    .map(|r| json!({ "uri": r.uri, "name": r.name }))
                    .collect();
            }
        }
        let Ok(raw) = std::env::var("NATIVES_MCP_ROOTS") else {
            return Vec::new();
        };
        raw.split(':')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|p| validate_root(p).ok())
            .map(|r| json!({ "uri": r.uri, "name": r.name }))
            .collect()
    }
}

/// Byte ceiling for one `resources/read` payload.
pub(crate) fn resource_max_bytes() -> usize {
    std::env::var("NATIVES_MCP_RESOURCE_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_RESOURCE_MAX_BYTES)
        .clamp(1024, 8 * 1024 * 1024)
}

pub(crate) fn truncate_on_char_boundary(text: &str, max_bytes: usize) -> String {
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// A root must be an existing absolute directory. Anything else would be a
/// grant we cannot back with a real path.
pub(crate) fn validate_root(raw: &str) -> Result<McpRoot, McpError> {
    let path = PathBuf::from(raw.trim());
    if !path.is_absolute() {
        return Err(McpError::Invalid(format!(
            "mcp root must be an absolute path: {raw}"
        )));
    }
    if !path.is_dir() {
        return Err(McpError::Invalid(format!(
            "mcp root is not an existing directory: {raw}"
        )));
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    Ok(McpRoot {
        uri: format!("file://{}", path.to_string_lossy()),
        name,
    })
}

/// Match a URI against an RFC 6570-style `{var}` template.
///
/// Deliberately conservative: literal segments must match exactly and a `{var}`
/// expands to one or more characters that are **not** `/` and do not contain
/// `..`. A permissive matcher here would silently widen the read allowlist,
/// which is the one thing this function must never do.
pub(crate) fn uri_matches_template(uri: &str, template: &str) -> bool {
    if uri.contains("..") {
        return false;
    }
    let mut rest = uri;
    let mut parts = template.split('{');

    // Text before the first `{` is a literal prefix.
    let Some(prefix) = parts.next() else {
        return false;
    };
    let Some(after_prefix) = rest.strip_prefix(prefix) else {
        return false;
    };
    rest = after_prefix;

    let mut segments: Vec<&str> = Vec::new();
    for part in parts {
        // Each part is `varname}literal`. A template without the closing brace
        // is malformed; refuse rather than guess.
        let Some((_var, literal)) = part.split_once('}') else {
            return false;
        };
        segments.push(literal);
    }

    for (index, literal) in segments.iter().enumerate() {
        let is_last = index + 1 == segments.len();
        if literal.is_empty() {
            if is_last {
                // Trailing variable: must consume at least one non-slash char.
                return !rest.is_empty() && !rest.contains('/');
            }
            // Two adjacent variables with no separator are unmatchable.
            return false;
        }
        let Some(found) = rest.find(literal) else {
            return false;
        };
        if found == 0 {
            // Variable matched nothing.
            return false;
        }
        if rest[..found].contains('/') {
            return false;
        }
        rest = &rest[found + literal.len()..];
    }

    rest.is_empty()
}
