//! 旧数据可恢复迁移器（MIG-001 / plan3 03-data-security-contracts §6）。
//!
//! 将旧 `user_providers` / `provider_api_keys` / `provider_accounts` 迁移到
//! `ai_providers` / `ai_connections` / `ai_credentials`，并将解密后的 Secret
//! 写入 OS Keychain。

use rusqlite::Connection as DbConn;
use serde::{Deserialize, Serialize};

use super::model::{CredentialKind, CredentialStatus, UpstreamProtocol};
use super::store;
use crate::provider_key_manager;
use crate::secrets::keychain::KeychainSecretStore;
use crate::secrets::store::{SecretRef, SecretStore};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub providers_migrated: usize,
    pub connections_migrated: usize,
    pub api_keys_migrated: usize,
    pub oauth_accounts_migrated: usize,
    pub errors: Vec<String>,
}

pub fn run_ai_resources_migration(conn: &DbConn) -> Result<MigrationReport> {
    let mut report = MigrationReport {
        providers_migrated: 0,
        connections_migrated: 0,
        api_keys_migrated: 0,
        oauth_accounts_migrated: 0,
        errors: Vec::new(),
    };

    // 1. 迁移 user_providers 到 ai_providers + ai_connections
    let mut prov_stmt = conn
        .prepare(
            "SELECT id, preset_name, api_protocol, name, website_url, base_url, created_at, updated_at
             FROM user_providers",
        )
        .map_err(Error::Database)?;

    let prov_rows = prov_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;

    for (id, preset_name, api_protocol, name, website_url, base_url, _created_at, _updated_at) in
        prov_rows
    {
        // 创建 Provider
        if store::get_provider(conn, &id)?.is_none() {
            let preset_key = if preset_name.is_empty() {
                None
            } else {
                Some(preset_name.as_str())
            };
            if let Err(e) = store::create_provider(
                conn,
                Some(&id),
                preset_key,
                &name,
                &website_url,
                preset_key,
                true,
            ) {
                report
                    .errors
                    .push(format!("Failed to migrate provider {id}: {e}"));
                continue;
            }
            report.providers_migrated += 1;
        }

        // 创建默认 Connection
        let conn_id = format!("conn-{id}");
        if store::get_connection(conn, &conn_id)?.is_none() {
            let upstream_proto = UpstreamProtocol::from_str(&api_protocol)
                .unwrap_or(UpstreamProtocol::OpenaiChatCompletions);
            if let Err(e) = store::create_connection(
                conn,
                Some(&conn_id),
                &id,
                &format!("{name} Default Endpoint"),
                &base_url,
                upstream_proto,
                None,
                None,
                None,
                true,
            ) {
                report
                    .errors
                    .push(format!("Failed to migrate connection {conn_id}: {e}"));
                continue;
            }
            report.connections_migrated += 1;
        }
    }

    // 2. 迁移 provider_api_keys 到 ai_credentials (Keychain)
    let secret_store = KeychainSecretStore::default();
    let mut key_stmt = conn
        .prepare(
            "SELECT id, provider_id, label, api_key_encrypted, dek_encrypted, created_at
             FROM provider_api_keys",
        )
        .map_err(Error::Database)?;

    let key_rows = key_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;

    for (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at) in key_rows {
        if store::get_credential(conn, &id)?.is_some() {
            continue;
        }

        // 解密旧密文
        let plain_key = if !api_key_encrypted.is_empty() && !dek_encrypted.is_empty() {
            match provider_key_manager::envelope_decrypt(&api_key_encrypted, &dek_encrypted, conn) {
                Ok(plain) => plain,
                Err(e) => {
                    report
                        .errors
                        .push(format!("Failed to decrypt API key {id}: {e}"));
                    continue;
                }
            }
        } else {
            String::new()
        };

        let secret_ref_str = format!("natives/ai/credential/{id}/v1");
        let secret_ref = SecretRef::new(secret_ref_str.clone());

        // 写入 Keychain
        if !plain_key.is_empty() {
            if let Err(e) = secret_store.write(&secret_ref, plain_key.as_bytes()) {
                report
                    .errors
                    .push(format!("Failed to write API key {id} to Keychain: {e}"));
                continue;
            }
        }

        let masked = if plain_key.len() > 8 {
            format!("{}…{}", &plain_key[..3], &plain_key[plain_key.len() - 4..])
        } else {
            "sk-…".to_string()
        };

        if let Err(e) = store::insert_credential(
            conn,
            &id,
            &provider_id,
            CredentialKind::ApiKey,
            &label,
            &secret_ref_str,
            1,
            &masked,
            CredentialStatus::Active,
            0,
            10,
            None,
            Some(&created_at),
            None,
            None,
            None,
        ) {
            report
                .errors
                .push(format!("Failed to insert credential {id}: {e}"));
            continue;
        }

        // 绑定到对应 connection
        let conn_id = format!("conn-{provider_id}");
        let _ = store::bind_credential_connection(conn, &id, &conn_id);
        report.api_keys_migrated += 1;
    }

    // 3. 迁移 provider_accounts 到 ai_credentials (Keychain)
    let mut acct_stmt = conn
        .prepare(
            "SELECT id, provider_id, name, platform, credentials_encrypted, dek_encrypted, expires_at, status, identity_fingerprint, created_at
             FROM provider_accounts",
        )
        .map_err(Error::Database)?;

    let acct_rows = acct_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        })
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;

    for (
        id,
        provider_id,
        name,
        _platform,
        cred_enc,
        dek_enc,
        expires_at,
        status_str,
        fingerprint,
        created_at,
    ) in acct_rows
    {
        if store::get_credential(conn, &id)?.is_some() {
            continue;
        }

        let plain_json = if !cred_enc.is_empty() && !dek_enc.is_empty() {
            match provider_key_manager::envelope_decrypt(&cred_enc, &dek_enc, conn) {
                Ok(plain) => plain,
                Err(e) => {
                    report
                        .errors
                        .push(format!("Failed to decrypt OAuth account {id}: {e}"));
                    continue;
                }
            }
        } else {
            "{}".to_string()
        };

        let secret_ref_str = format!("natives/ai/credential/{id}/v1");
        let secret_ref = SecretRef::new(secret_ref_str.clone());

        if !plain_json.is_empty() {
            if let Err(e) = secret_store.write(&secret_ref, plain_json.as_bytes()) {
                report.errors.push(format!(
                    "Failed to write OAuth secret {id} to Keychain: {e}"
                ));
                continue;
            }
        }

        let masked = format!("oauth:{}…", &id[..id.len().min(8)]);
        let status = CredentialStatus::from_str(&status_str);

        if let Err(e) = store::insert_credential(
            conn,
            &id,
            &provider_id,
            CredentialKind::Oauth,
            &name,
            &secret_ref_str,
            1,
            &masked,
            status,
            0,
            10,
            expires_at.as_deref(),
            Some(&created_at),
            expires_at.as_deref(),
            Some(&fingerprint),
            None,
        ) {
            report
                .errors
                .push(format!("Failed to insert OAuth credential {id}: {e}"));
            continue;
        }

        let conn_id = format!("conn-{provider_id}");
        let _ = store::bind_credential_connection(conn, &id, &conn_id);
        report.oauth_accounts_migrated += 1;
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};

    #[test]
    fn test_migration_runs_idempotently() {
        let conn = DbConn::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();

        // 插入一些旧表测试数据
        conn.execute(
            "INSERT INTO user_providers (id, preset_name, api_protocol, name, website_url, base_url, created_at, updated_at)
             VALUES ('p-mig', 'openai', 'openai_chat_completions', 'OpenAI', 'https://openai.com', 'https://api.openai.com/v1', 't0', 't0')",
            [],
        )
        .unwrap();

        let rep = run_ai_resources_migration(&conn).unwrap();
        assert_eq!(rep.providers_migrated, 1);
        assert_eq!(rep.connections_migrated, 1);

        // 第二次运行幂等
        let rep2 = run_ai_resources_migration(&conn).unwrap();
        assert_eq!(rep2.providers_migrated, 0);
        assert_eq!(rep2.connections_migrated, 0);
    }
}
