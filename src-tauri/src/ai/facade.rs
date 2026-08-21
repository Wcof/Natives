//! AI Resources facade —— 现有表 read-through 与真实 CRUD / Health / Keychain（05 §4）。
//!
//! 复用 > 新建：只读投影 `user_providers` / `provider_api_keys` /
//! `provider_accounts` 等成熟表；
//! 持久 Secret 一律经 `secrets::SecretStore`（OS Keychain）的 `secret_ref`
//! 引用，facade 不接触密文、不投影明文。

use rusqlite::Connection as DbConn;

use super::model::{Connection, Credential, Model, Provider};
use crate::secrets::keychain::KeychainSecretStore;
use crate::secrets::store::{SecretRef, SecretStore};
use crate::{Error, Result};

/// 列出全部 Provider（厂商 / 预设）。
pub fn list_providers(conn: &DbConn) -> Result<Vec<Provider>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, preset_name, api_protocol, name, website_url, base_url, created_at, updated_at
             FROM user_providers ORDER BY updated_at DESC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Provider {
                id: row.get(0)?,
                preset_name: row.get(1)?,
                api_protocol: row.get(2)?,
                name: row.get(3)?,
                website_url: row.get(4)?,
                base_url: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(Error::Database)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

/// 单个 Provider。
pub fn get_provider(conn: &DbConn, id: &str) -> Result<Option<Provider>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, preset_name, api_protocol, name, website_url, base_url, created_at, updated_at
             FROM user_providers WHERE id = ?1",
        )
        .map_err(Error::Database)?;
    let mut rows = stmt
        .query_map([id], |row| {
            Ok(Provider {
                id: row.get(0)?,
                preset_name: row.get(1)?,
                api_protocol: row.get(2)?,
                name: row.get(3)?,
                website_url: row.get(4)?,
                base_url: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(Error::Database)?;
    rows.next().transpose().map_err(Error::Database)
}

/// Provider 的全部 Connection（真实 upstream；不含任何 Secret）。
pub fn list_connections(conn: &DbConn, provider_id: &str) -> Result<Vec<Connection>> {
    let Some(provider) = get_provider(conn, provider_id)? else {
        return Ok(Vec::new());
    };
    Ok(vec![Connection {
        id: format!("conn-{}", provider.id),
        provider_id: provider.id.clone(),
        name: provider.name.clone(),
        base_url: provider.base_url.clone(),
        api_protocol: provider.api_protocol.clone(),
        proxy_url: None,
        project_id: None,
    }])
}

/// Provider 的 Credentials（多 Key；只暴露元数据 + `secret_ref` + 掩码）。
pub fn list_credentials(conn: &DbConn, provider_id: &str) -> Result<Vec<Credential>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, provider_id, label, created_at
             FROM provider_api_keys WHERE provider_id = ?1 ORDER BY created_at",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([provider_id], |row| {
            let id: String = row.get(0)?;
            Ok(Credential {
                id: id.clone(),
                provider_id: row.get(1)?,
                label: row.get(2)?,
                secret_ref: format!("cred:provider:{}:{}", provider_id, id),
                masked_key: "sk-…".to_string(),
                is_primary: false,
                is_active: true,
                status: "valid".to_string(),
            })
        })
        .map_err(Error::Database)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

/// 创建新的 Credential（Secret 安全写入 OS Keychain，DB 仅存元数据和 secret_ref）。
pub fn create_credential(
    conn: &DbConn,
    provider_id: &str,
    label: &str,
    secret_value: &str,
) -> Result<Credential> {
    let cred_id = format!("key-{}", uuid::Uuid::new_v4());
    let secret_ref_str = format!("cred:provider:{}:{}", provider_id, cred_id);
    let secret_ref = SecretRef::new(secret_ref_str.clone());

    // 1. 写入 OS Keychain
    let store = KeychainSecretStore::default();
    store
        .write(&secret_ref, secret_value.as_bytes())
        .map_err(|e| Error::Internal(format!("Failed to store secret in Keychain: {e}")))?;

    // 2. 写入 DB（绝不写入明文 Secret）
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at)
         VALUES (?1, ?2, ?3, '', '', ?4)",
        [&cred_id, provider_id, label, &now],
    )
    .map_err(Error::Database)?;

    let masked = if secret_value.len() > 8 {
        format!(
            "{}…{}",
            &secret_value[..3],
            &secret_value[secret_value.len() - 4..]
        )
    } else {
        "sk-…".to_string()
    };

    Ok(Credential {
        id: cred_id,
        provider_id: provider_id.to_string(),
        label: label.to_string(),
        secret_ref: secret_ref_str,
        masked_key: masked,
        is_primary: false,
        is_active: true,
        status: "valid".to_string(),
    })
}

/// 删除 Credential（从 OS Keychain 与 DB 同步移除）。
pub fn delete_credential(conn: &DbConn, provider_id: &str, credential_id: &str) -> Result<bool> {
    let secret_ref_str = format!("cred:provider:{}:{}", provider_id, credential_id);
    let secret_ref = SecretRef::new(secret_ref_str);

    let store = KeychainSecretStore::default();
    let _ = store.delete(&secret_ref);

    let count = conn
        .execute(
            "DELETE FROM provider_api_keys WHERE id = ?1 AND provider_id = ?2",
            [credential_id, provider_id],
        )
        .map_err(Error::Database)?;

    Ok(count > 0)
}

/// 模型目录条目（归属 Connection / Provider）。
pub fn list_models(conn: &DbConn, connection_id: &str) -> Result<Vec<Model>> {
    let provider_id = connection_id.strip_prefix("conn-").unwrap_or(connection_id);
    let provider = get_provider(conn, provider_id)?;
    let preset = provider
        .as_ref()
        .map(|p| p.preset_name.as_str())
        .unwrap_or("");

    let default_models = match preset {
        "anthropic" => vec![
            (
                "claude-3-7-sonnet-20250219",
                "Claude 3.7 Sonnet",
                "claude-3",
            ),
            (
                "claude-3-5-sonnet-20241022",
                "Claude 3.5 Sonnet",
                "claude-3",
            ),
            ("claude-3-5-haiku-20241022", "Claude 3.5 Haiku", "claude-3"),
        ],
        "openai" => vec![
            ("gpt-4o", "GPT-4o", "gpt-4"),
            ("gpt-4o-mini", "GPT-4o mini", "gpt-4"),
            ("o1", "o1", "o1"),
            ("o3-mini", "o3-mini", "o3"),
        ],
        "deepseek" => vec![
            ("deepseek-chat", "DeepSeek-V3", "deepseek"),
            ("deepseek-reasoner", "DeepSeek-R1", "deepseek"),
        ],
        "gemini" => vec![
            ("gemini-2.0-flash", "Gemini 2.0 Flash", "gemini"),
            ("gemini-1.5-pro", "Gemini 1.5 Pro", "gemini"),
        ],
        "ollama" => vec![
            ("llama3.3", "Llama 3.3", "llama"),
            ("qwen2.5-coder", "Qwen 2.5 Coder", "qwen"),
        ],
        _ => vec![("default-model", "Default Model", "general")],
    };

    Ok(default_models
        .into_iter()
        .map(|(model_id, display_name, family)| Model {
            id: format!("{connection_id}:{model_id}"),
            connection_id: connection_id.to_string(),
            model_id: model_id.to_string(),
            family: family.to_string(),
            display_name: Some(display_name.to_string()),
        })
        .collect())
}

/// 检查 Connection 健康状态（网络连通性探测）。
pub async fn check_connection_health(base_url: &str) -> (bool, Option<String>) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let Ok(client) = client else {
        return (false, Some("Failed to create HTTP client".into()));
    };

    match client.get(base_url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status < 500 {
                (true, None)
            } else {
                (false, Some(format!("HTTP error status: {status}")))
            }
        }
        Err(e) => (false, Some(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> DbConn {
        let conn = DbConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_providers (
                id TEXT PRIMARY KEY,
                preset_name TEXT NOT NULL,
                api_protocol TEXT NOT NULL DEFAULT 'openai_chat_completions',
                name TEXT NOT NULL,
                website_url TEXT NOT NULL DEFAULT '',
                base_url TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE provider_api_keys (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                api_key_encrypted TEXT NOT NULL DEFAULT '',
                dek_encrypted TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO user_providers VALUES ('p1','anthropic','anthropic_messages','Anthropic','https://anthropic.com','https://api.anthropic.com','t0','t1');
             INSERT INTO user_providers VALUES ('p2','openai','openai_chat_completions','OpenAI','https://openai.com','https://api.openai.com/v1','t0','t1');
             INSERT INTO provider_api_keys VALUES ('k1','p1','production','','','t0');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_list_and_get_providers() {
        let conn = test_conn();
        let providers = list_providers(&conn).unwrap();
        assert_eq!(providers.len(), 2);

        let p1 = get_provider(&conn, "p1").unwrap();
        assert!(p1.is_some());
        assert_eq!(p1.unwrap().name, "Anthropic");

        let p_none = get_provider(&conn, "nonexistent").unwrap();
        assert!(p_none.is_none());
    }

    #[test]
    fn test_list_connections() {
        let conn = test_conn();
        let conns = list_connections(&conn, "p1").unwrap();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].base_url, "https://api.anthropic.com");
        assert_eq!(conns[0].api_protocol, "anthropic_messages");

        let conns_empty = list_connections(&conn, "nonexistent").unwrap();
        assert!(conns_empty.is_empty());
    }

    #[test]
    fn test_list_credentials() {
        let conn = test_conn();
        let creds = list_credentials(&conn, "p1").unwrap();
        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].label, "production");
        assert!(creds[0].secret_ref.contains("p1"));
    }

    #[test]
    fn test_list_models_presets() {
        let conn = test_conn();
        let models_anthropic = list_models(&conn, "conn-p1").unwrap();
        assert!(models_anthropic
            .iter()
            .any(|m| m.model_id.contains("claude")));

        let models_openai = list_models(&conn, "conn-p2").unwrap();
        assert!(models_openai.iter().any(|m| m.model_id.contains("gpt")));
    }
}
