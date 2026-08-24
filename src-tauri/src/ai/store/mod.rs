//! AI Resources Repository & Store（DAT-002）。

pub mod connections;
pub mod credentials;
pub mod models;
pub mod providers;
pub mod quotas;

pub use connections::*;
pub use credentials::*;
pub use models::*;
pub use providers::*;
pub use quotas::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::model::{
        CredentialKind, CredentialStatus, ModelAvailability, ModelSource, QuotaSnapshot,
        QuotaStatus, QuotaWindow, UpstreamProtocol,
    };
    use crate::db::{apply_migrations, create_tables};
    use rusqlite::Connection as DbConn;

    fn setup_test_db() -> DbConn {
        let conn = DbConn::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn provider_crud_and_impact() {
        let conn = setup_test_db();
        let p = create_provider(
            &conn,
            Some("p-openai"),
            Some("openai"),
            "OpenAI",
            "https://openai.com",
            Some("openai-icon"),
            true,
        )
        .unwrap();
        assert_eq!(p.id, "p-openai");
        assert_eq!(p.name, "OpenAI");

        let p_get = get_provider(&conn, "p-openai").unwrap().unwrap();
        assert_eq!(p_get.name, "OpenAI");

        let updated = update_provider(
            &conn,
            "p-openai",
            Some("OpenAI Official"),
            None,
            None,
            Some(false),
        )
        .unwrap();
        assert_eq!(updated.name, "OpenAI Official");
        assert!(!updated.enabled);

        let list = list_providers(&conn).unwrap();
        assert_eq!(list.len(), 1);

        let impact = get_provider_delete_impact(&conn, "p-openai").unwrap();
        assert_eq!(impact.connection_count, 0);

        let deleted = delete_provider(&conn, "p-openai").unwrap();
        assert!(deleted);
        assert!(get_provider(&conn, "p-openai").unwrap().is_none());
    }

    #[test]
    fn connection_and_credential_lifecycle() {
        let conn = setup_test_db();
        let _p = create_provider(
            &conn,
            Some("p1"),
            None,
            "Anthropic",
            "https://anthropic.com",
            None,
            true,
        )
        .unwrap();

        let c1 = create_connection(
            &conn,
            Some("c1"),
            "p1",
            "Official Messages",
            "https://api.anthropic.com",
            UpstreamProtocol::AnthropicMessages,
            None,
            None,
            None,
            true,
        )
        .unwrap();
        assert_eq!(c1.upstream_protocol, UpstreamProtocol::AnthropicMessages);

        let cred1 = insert_credential(
            &conn,
            "cred-1",
            "p1",
            CredentialKind::ApiKey,
            "Primary Key",
            "natives/ai/credential/cred-1/v1",
            1,
            "sk-ant…1234",
            CredentialStatus::Active,
            0,
            5,
            None,
            None,
            None,
            Some("fingerprint-1"),
            None,
        )
        .unwrap();
        assert_eq!(cred1.label, "Primary Key");

        bind_credential_connection(&conn, "cred-1", "c1").unwrap();
        let bindings = list_credential_connections(&conn, "cred-1").unwrap();
        assert_eq!(bindings, vec!["c1"]);

        // Models
        let m = upsert_model(
            &conn,
            Some("p1"),
            Some("c1"),
            Some("cred-1"),
            "claude-3-7-sonnet-20250219",
            "Claude 3.7 Sonnet",
            ModelSource::Discovered,
            Some("{\"reasoning\":true}"),
            ModelAvailability::Available,
        )
        .unwrap();
        assert_eq!(m.model_id, "claude-3-7-sonnet-20250219");

        let models = list_models(&conn, Some("c1"), None).unwrap();
        assert_eq!(models.len(), 1);

        // Quota
        let snapshot = QuotaSnapshot {
            id: "snap-1".into(),
            credential_id: "cred-1".into(),
            provider_adapter: "anthropic".into(),
            status: QuotaStatus::Available,
            plan_name: Some("Scale".into()),
            error_category: None,
            error_message: None,
            fetched_at: chrono::Utc::now().to_rfc3339(),
            expires_at: None,
            windows: vec![QuotaWindow {
                id: "win-1".into(),
                snapshot_id: "snap-1".into(),
                label: "Requests / Min".into(),
                remaining: Some(950.0),
                limit_value: Some(1000.0),
                used: Some(50.0),
                unit: Some("req".into()),
                reset_at: None,
            }],
        };
        save_quota_snapshot(&conn, &snapshot).unwrap();

        let read_snap = get_quota_snapshot(&conn, "cred-1").unwrap().unwrap();
        assert_eq!(read_snap.plan_name.as_deref(), Some("Scale"));
        assert_eq!(read_snap.windows.len(), 1);
        assert_eq!(read_snap.windows[0].remaining, Some(950.0));

        let summary = get_ai_resources_summary(&conn).unwrap();
        assert_eq!(summary.provider_count, 1);
        assert_eq!(summary.connection_count, 1);
        assert_eq!(summary.credential_count, 1);
        assert_eq!(summary.available_model_count, 1);
    }
}
