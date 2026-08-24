//! AI Resources facade —— 桥接 Command 与 deep Store（DAT-002 / API-001..004）。
//!
//! 所有操作走 `crate::ai::store`；
//! 持久 Secret 一律经 `secrets::SecretStore`（OS Keychain）的 `secret_ref` 引用。

use rusqlite::Connection as DbConn;

use super::model::{
    AiResourcesSummary, Connection, Credential, CredentialKind, CredentialStatus, DeleteImpact,
    Model, ModelAvailability, ModelSource, Provider, UpstreamProtocol,
};
use crate::ai::store;
use crate::secrets::keychain::KeychainSecretStore;
use crate::secrets::store::{SecretRef, SecretStore};
use crate::{Error, Result};

pub fn list_providers(conn: &DbConn) -> Result<Vec<Provider>> {
    store::list_providers(conn)
}

pub fn get_provider(conn: &DbConn, id: &str) -> Result<Option<Provider>> {
    store::get_provider(conn, id)
}

pub fn create_provider(
    conn: &DbConn,
    id: Option<&str>,
    preset_key: Option<&str>,
    name: &str,
    website_url: &str,
    icon_key: Option<&str>,
    enabled: bool,
) -> Result<Provider> {
    store::create_provider(conn, id, preset_key, name, website_url, icon_key, enabled)
}

pub fn update_provider(
    conn: &DbConn,
    id: &str,
    name: Option<&str>,
    website_url: Option<&str>,
    icon_key: Option<&str>,
    enabled: Option<bool>,
) -> Result<Provider> {
    store::update_provider(conn, id, name, website_url, icon_key, enabled)
}

pub fn delete_provider(conn: &DbConn, id: &str) -> Result<bool> {
    store::delete_provider(conn, id)
}

pub fn get_provider_delete_impact(conn: &DbConn, id: &str) -> Result<DeleteImpact> {
    store::get_provider_delete_impact(conn, id)
}

pub fn list_connections(conn: &DbConn, provider_id: &str) -> Result<Vec<Connection>> {
    store::list_connections(conn, Some(provider_id))
}

pub fn get_connection(conn: &DbConn, id: &str) -> Result<Option<Connection>> {
    store::get_connection(conn, id)
}

pub fn create_connection(
    conn: &DbConn,
    id: Option<&str>,
    provider_id: &str,
    name: &str,
    base_url: &str,
    upstream_protocol: UpstreamProtocol,
    models_url: Option<&str>,
    proxy_url: Option<&str>,
    headers_json: Option<&str>,
    enabled: bool,
) -> Result<Connection> {
    store::create_connection(
        conn,
        id,
        provider_id,
        name,
        base_url,
        upstream_protocol,
        models_url,
        proxy_url,
        headers_json,
        enabled,
    )
}

pub fn update_connection(
    conn: &DbConn,
    id: &str,
    name: Option<&str>,
    base_url: Option<&str>,
    upstream_protocol: Option<UpstreamProtocol>,
    models_url: Option<Option<&str>>,
    proxy_url: Option<Option<&str>>,
    headers_json: Option<Option<&str>>,
    enabled: Option<bool>,
) -> Result<Connection> {
    store::update_connection(
        conn,
        id,
        name,
        base_url,
        upstream_protocol,
        models_url,
        proxy_url,
        headers_json,
        enabled,
        None,
        None,
    )
}

pub fn delete_connection(conn: &DbConn, id: &str) -> Result<bool> {
    store::delete_connection(conn, id)
}

pub fn list_credentials(conn: &DbConn, provider_id: &str) -> Result<Vec<Credential>> {
    store::list_credentials(conn, Some(provider_id))
}

pub fn get_credential(conn: &DbConn, id: &str) -> Result<Option<Credential>> {
    store::get_credential(conn, id)
}

pub fn create_api_key_credential(
    conn: &DbConn,
    provider_id: &str,
    label: &str,
    secret_value: &str,
    priority: u32,
    concurrency_limit: u32,
    connection_ids: &[String],
) -> Result<Credential> {
    let cred_id = format!("key-{}", uuid::Uuid::new_v4());
    let secret_ref_str = format!("natives/ai/credential/{}/v1", cred_id);
    let secret_ref = SecretRef::new(secret_ref_str.clone());

    // 1. 写入 OS Keychain
    let store = KeychainSecretStore::default();
    store
        .write(&secret_ref, secret_value.as_bytes())
        .map_err(|e| Error::Internal(format!("Failed to store secret in Keychain: {e}")))?;

    // 2. 掩码
    let masked = if secret_value.len() > 8 {
        format!(
            "{}…{}",
            &secret_value[..3],
            &secret_value[secret_value.len() - 4..]
        )
    } else {
        "sk-…".to_string()
    };

    // 3. 写入 DB
    let cred = store::insert_credential(
        conn,
        &cred_id,
        provider_id,
        CredentialKind::ApiKey,
        label,
        &secret_ref_str,
        1,
        &masked,
        CredentialStatus::Active,
        priority,
        concurrency_limit,
        None,
        None,
        None,
        None,
        None,
    )?;

    // 4. 绑定 connections
    for cid in connection_ids {
        store::bind_credential_connection(conn, &cred_id, cid)?;
    }

    Ok(cred)
}

pub fn delete_credential(conn: &DbConn, _provider_id: &str, credential_id: &str) -> Result<bool> {
    if let Some(cred) = store::get_credential(conn, credential_id)? {
        let store = KeychainSecretStore::default();
        let _ = store.delete(&SecretRef::new(cred.secret_ref));
    }
    store::delete_credential(conn, credential_id)
}

pub fn list_models(
    conn: &DbConn,
    connection_id: Option<&str>,
    credential_id: Option<&str>,
) -> Result<Vec<Model>> {
    store::list_models(conn, connection_id, credential_id)
}

pub fn upsert_model(
    conn: &DbConn,
    provider_id: Option<&str>,
    connection_id: Option<&str>,
    source_credential_id: Option<&str>,
    model_id: &str,
    display_name: &str,
    source: ModelSource,
    capabilities_json: Option<&str>,
    availability: ModelAvailability,
) -> Result<Model> {
    store::upsert_model(
        conn,
        provider_id,
        connection_id,
        source_credential_id,
        model_id,
        display_name,
        source,
        capabilities_json,
        availability,
    )
}

pub fn get_ai_resources_summary(conn: &DbConn) -> Result<AiResourcesSummary> {
    store::get_ai_resources_summary(conn)
}

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
                (false, Some(format!("HTTP status: {status}")))
            }
        }
        Err(e) => (false, Some(e.to_string())),
    }
}
