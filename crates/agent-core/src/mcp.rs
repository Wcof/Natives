//! MCP Gateway skeleton — list/discover tools from stdio or HTTP/SSE configs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Http,
    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub trusted: bool,
    /// Optional bearer / OAuth access token (never logged). Loaded via Credential Store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// Optional static headers (e.g. `Authorization` already resolved by host).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

/// Short-lived MCP OAuth/bearer lease (memory only; never persisted to events).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCredentialLease {
    pub server_id: String,
    /// Opaque token type for clients (`bearer` / `oauth_access`).
    pub token_type: String,
    /// Expiry unix secs if known; None = session-scoped.
    pub expires_at: Option<u64>,
    /// Redacted status only — never include raw token in Serialize responses to UI.
    pub has_token: bool,
}

/// Process-local MCP credential store (OAuth access tokens / bearer).
#[derive(Default)]
pub struct McpCredentialStore {
    /// server_id → (token, token_type, expires_at)
    tokens: HashMap<String, (String, String, Option<u64>)>,
}

impl McpCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_token(
        &mut self,
        server_id: &str,
        token: String,
        token_type: &str,
        expires_at: Option<u64>,
    ) -> Result<(), String> {
        if server_id.trim().is_empty() {
            return Err("server_id required".into());
        }
        if token.trim().is_empty() {
            return Err("token required".into());
        }
        self.tokens.insert(
            server_id.to_string(),
            (token, token_type.to_string(), expires_at),
        );
        Ok(())
    }

    pub fn clear(&mut self, server_id: &str) {
        self.tokens.remove(server_id);
    }

    pub fn get_token(&self, server_id: &str) -> Option<&str> {
        self.tokens.get(server_id).map(|(t, _, _)| t.as_str())
    }

    /// Status without secret material — safe for RPC/UI.
    pub fn lease_status(&self, server_id: &str) -> McpCredentialLease {
        match self.tokens.get(server_id) {
            Some((_, ty, exp)) => McpCredentialLease {
                server_id: server_id.to_string(),
                token_type: ty.clone(),
                expires_at: *exp,
                has_token: true,
            },
            None => McpCredentialLease {
                server_id: server_id.to_string(),
                token_type: "none".into(),
                expires_at: None,
                has_token: false,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    pub server_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// In-memory MCP registry (dynamic discovery hooks in later).
#[derive(Default)]
pub struct McpRegistry {
    servers: HashMap<String, McpServerConfig>,
    tools: Vec<McpToolDescriptor>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_server(&mut self, config: McpServerConfig) -> Result<(), String> {
        if !config.trusted && matches!(config.transport, McpTransport::Stdio) {
            return Err("untrusted stdio MCP servers are not auto-started".into());
        }
        if !config.trusted && matches!(config.transport, McpTransport::Http | McpTransport::Sse) {
            if let Some(url) = &config.url {
                if url.contains("127.0.0.1")
                    || url.contains("localhost")
                    || url.contains("169.254.")
                    || url.starts_with("file:")
                    || url.contains("0.0.0.0")
                    || url.contains("[::1]")
                {
                    return Err(
                        "SSRF: local MCP HTTP endpoints blocked unless trusted".into(),
                    );
                }
            }
        }
        self.servers.insert(config.id.clone(), config);
        Ok(())
    }

    pub fn upsert_tool(&mut self, tool: McpToolDescriptor) {
        self.tools.retain(|t| !(t.server_id == tool.server_id && t.name == tool.name));
        self.tools.push(tool);
    }

    pub fn list_servers(&self) -> Vec<&McpServerConfig> {
        self.servers.values().collect()
    }

    pub fn list_tools(&self) -> &[McpToolDescriptor] {
        &self.tools
    }

    /// Build tool schemas namespaced as `mcp__{server}__{tool}`.
    pub fn namespaced_tool_schemas(&self) -> Vec<(String, String, serde_json::Value)> {
        self.tools
            .iter()
            .map(|t| {
                (
                    format!("mcp__{}__{}", t.server_id, t.name),
                    t.description.clone(),
                    t.input_schema.clone(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_stdio() {
        let mut reg = McpRegistry::new();
        let err = reg
            .register_server(McpServerConfig {
                id: "fs".into(),
                transport: McpTransport::Stdio,
                command: Some("npx".into()),
                args: None,
                url: None,
                trusted: false,
            auth_token: None,
            headers: None,
            })
            .unwrap_err();
        assert!(err.contains("untrusted"));
    }

    #[test]
    fn namespaces_tools() {
        let mut reg = McpRegistry::new();
        reg.register_server(McpServerConfig {
            id: "mem".into(),
            transport: McpTransport::Http,
            command: None,
            args: None,
            url: Some("https://mcp.example.com".into()),
            trusted: true,
            auth_token: None,
            headers: None,
        })
        .unwrap();
        reg.upsert_tool(McpToolDescriptor {
            server_id: "mem".into(),
            name: "search".into(),
            description: "search memory".into(),
            input_schema: serde_json::json!({"type":"object"}),
        });
        let schemas = reg.namespaced_tool_schemas();
        assert_eq!(schemas[0].0, "mcp__mem__search");
    }

    #[test]
    fn credential_store_never_serializes_token_in_lease() {
        let mut store = McpCredentialStore::new();
        store
            .set_token("s1", "secret-oauth-token-xyz".into(), "bearer", None)
            .unwrap();
        let lease = store.lease_status("s1");
        let json = serde_json::to_string(&lease).unwrap();
        assert!(lease.has_token);
        assert!(!json.contains("secret-oauth"));
        assert!(json.contains("has_token"));
    }
}
