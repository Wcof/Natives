//! Capability library store tests (ADR-0016): migration 021, CRUD, trust
//! boundaries, selection fail-closed semantics.

use super::*;
use serde_json::json;
use uuid::Uuid;

fn with_temp_db<F: FnOnce()>(f: F) {
    let _guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(format!("capability-{}.db", Uuid::new_v4()));
    let art = dir.path().join("artifacts");
    crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
    let _warm = crate::storage::DataStore::new(&db, &art).expect("capability temp db migrate");
    f();
    crate::storage::set_test_db_override(None, None);
}

#[test]
fn migration_021_tables_exist_and_dead_table_dropped() {
    with_temp_db(|| {
        let s = store().unwrap();
        for table in [
            "capability_skill",
            "capability_mcp_server",
            "capability_expert",
            "capability_expert_team",
            "capability_expert_team_member",
            "capability_mcp_hub_cache",
        ] {
            assert!(s.has_table(table), "missing table {table}");
        }
        assert!(
            !s.has_table("mcp_server_config"),
            "dead table must be dropped"
        );
        let conn = s.conn().unwrap();
        for (table, col) in [
            ("conversation", "capability_selection_json"),
            ("run", "capability_snapshot_json"),
        ] {
            let has: i64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{col}'"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(has, 1, "missing column {table}.{col}");
        }
    });
}

#[test]
fn expert_crud_round_trip_and_auto_key_rejected() {
    with_temp_db(|| {
        let created = experts::create(&json!({
            "id": "coder",
            "name": "Coder",
            "systemPrompt": "You write code.",
            "tools": ["read_file"],
            "skills": ["user:tdd"],
            "keyId": "k1",
        }))
        .unwrap();
        assert_eq!(created["expert"]["id"], "coder");
        assert_eq!(created["expert"]["skills"][0], "user:tdd");

        let err = experts::create(&json!({
            "name": "Bad",
            "systemPrompt": "x",
            "keyId": "auto",
        }))
        .unwrap_err();
        assert!(err.contains("auto"), "auto key must be rejected: {err}");

        let updated =
            experts::update(&json!({ "id": "coder", "description": "writes code" })).unwrap();
        assert_eq!(updated["expert"]["description"], "writes code");

        let listed = experts::list(&json!({})).unwrap();
        assert_eq!(listed["experts"].as_array().unwrap().len(), 1);

        let deleted = experts::delete(&json!({ "id": "coder" })).unwrap();
        assert_eq!(deleted["deleted"], "coder");
    });
}

#[test]
fn expert_md_import_export_round_trip() {
    with_temp_db(|| {
        let md = "---\nid: reviewer\nname: Reviewer\ndescription: reviews diffs\ntools: [read_file, search_files]\nskills: [user:review]\nmaxSteps: 9\n---\n\nYou review code diffs carefully.\n";
        let imported = experts::import_md(&json!({ "content": md })).unwrap();
        assert_eq!(imported["expert"]["id"], "reviewer");
        assert_eq!(imported["expert"]["source"], "import_md");
        assert_eq!(imported["expert"]["params"]["maxSteps"], 9);

        let exported = experts::export_md(&json!({ "id": "reviewer" })).unwrap();
        let content = exported["content"].as_str().unwrap();
        assert!(content.contains("id: reviewer"));
        assert!(content.contains("tools: [read_file, search_files]"));
        assert!(content.contains("You review code diffs carefully."));
        // Round trip: exported content parses back to the same expert.
        let reparsed = agent_core::profile::parse_agent_profile_markdown(content, None).unwrap();
        assert_eq!(reparsed.id, "reviewer");
        assert_eq!(reparsed.max_steps, Some(9));
    });
}

#[test]
fn team_crud_validates_members_and_delete_reference_check() {
    with_temp_db(|| {
        for id in ["lead", "member-a"] {
            experts::create(&json!({
                "id": id,
                "name": id,
                "systemPrompt": "persona",
            }))
            .unwrap();
        }
        // Unknown member rejected.
        let err = experts::team_create(&json!({
            "id": "team-x",
            "name": "Team X",
            "members": [{ "expertId": "ghost" }],
        }))
        .unwrap_err();
        assert!(err.contains("expert not found"), "{err}");

        let team = experts::team_create(&json!({
            "id": "growth",
            "name": "Growth",
            "coordinatorExpertId": "lead",
            "failurePolicy": "fail_fast",
            "members": [
                { "expertId": "member-a", "roleHint": "builder" },
                { "expertId": "lead" }
            ],
        }))
        .unwrap();
        assert_eq!(team["team"]["failurePolicy"], "fail_fast");
        assert_eq!(team["team"]["members"].as_array().unwrap().len(), 2);

        // Expert referenced by a team: delete without force is refused.
        let err = experts::delete(&json!({ "id": "member-a" })).unwrap_err();
        assert!(err.contains("referenced by teams"), "{err}");
        let ok = experts::delete(&json!({ "id": "member-a", "force": true })).unwrap();
        assert_eq!(ok["deleted"], "member-a");
        // Member row cascaded away.
        let team = experts::team_get(&json!({ "id": "growth" })).unwrap();
        assert_eq!(team["team"]["members"].as_array().unwrap().len(), 1);

        experts::team_delete(&json!({ "id": "growth" })).unwrap();
        assert!(experts::team_get(&json!({ "id": "growth" })).is_err());
    });
}

#[test]
fn mcp_create_rejects_plaintext_authorization_and_secretlike_env() {
    with_temp_db(|| {
        let err = mcp::create(&json!({
            "id": "bad-header",
            "name": "Bad",
            "transport": "http",
            "url": "https://mcp.example.com",
            "headers": { "Authorization": "Bearer sk-live-123" },
        }))
        .unwrap_err();
        assert!(err.contains("Authorization"), "{err}");

        let err = mcp::create(&json!({
            "id": "bad-env",
            "name": "Bad Env",
            "transport": "stdio",
            "command": "npx",
            "env": { "FIGMA_API_KEY": "sk-plaintext" },
        }))
        .unwrap_err();
        assert!(err.contains("secret:"), "{err}");

        // Secret reference is accepted and never echoed back.
        let ok = mcp::create(&json!({
            "id": "figma",
            "name": "Figma",
            "transport": "stdio",
            "command": "npx",
            "args": ["figma-mcp"],
            "env": { "FIGMA_API_KEY": "secret:abc123", "LOG_LEVEL": "info" },
        }))
        .unwrap();
        let rendered = serde_json::to_string(&ok).unwrap();
        assert!(
            !rendered.contains("secret:abc123"),
            "secret ref value must not leak"
        );
        assert!(!rendered.contains("sk-"), "no secret material in response");
    });
}

#[test]
fn mcp_update_null_sentinel_keeps_stored_values() {
    with_temp_db(|| {
        mcp::create(&json!({
            "id": "kv-keep",
            "name": "KV Keep",
            "transport": "stdio",
            "command": "npx",
            "env": { "FIGMA_API_KEY": "secret:abc123", "LOG_LEVEL": "info" },
        }))
        .unwrap();

        let read_env = || -> serde_json::Value {
            let s = store().unwrap();
            let conn = s.conn().unwrap();
            let env_json: String = conn
                .query_row(
                    "SELECT env_json FROM capability_mcp_server WHERE id = 'kv-keep'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            serde_json::from_str(&env_json).unwrap()
        };

        // null = keep stored value; a real string replaces it.
        mcp::update(&json!({
            "id": "kv-keep",
            "env": { "FIGMA_API_KEY": null, "LOG_LEVEL": "debug" },
        }))
        .unwrap();
        let env = read_env();
        assert_eq!(
            env["FIGMA_API_KEY"], "secret:abc123",
            "null must keep stored value"
        );
        assert_eq!(env["LOG_LEVEL"], "debug", "explicit value must replace");

        // Absent key = deletion (whole-map replace semantics unchanged).
        mcp::update(&json!({ "id": "kv-keep", "env": { "FIGMA_API_KEY": null } })).unwrap();
        let env = read_env();
        assert_eq!(env["FIGMA_API_KEY"], "secret:abc123");
        assert!(env.get("LOG_LEVEL").is_none(), "absent key must be deleted");

        // null for a key with no stored value fails closed.
        let err =
            mcp::update(&json!({ "id": "kv-keep", "env": { "NOPE_TOKEN": null } })).unwrap_err();
        assert!(err.contains("cannot keep unknown env key"), "{err}");

        // create still rejects non-string values — null is update-only.
        let err = mcp::create(&json!({
            "id": "kv-null-create",
            "name": "Bad",
            "transport": "stdio",
            "command": "npx",
            "env": { "FOO": null },
        }))
        .unwrap_err();
        assert!(err.contains("must be a string"), "{err}");
    });
}

#[test]
fn mcp_import_json_standard_format_defaults_untrusted() {
    with_temp_db(|| {
        let payload = json!({
            "mcpServers": {
                "linear": { "command": "npx", "args": ["-y", "linear-mcp"] },
                "remote-docs": { "url": "https://docs-mcp.example.com/sse", "type": "sse" },
                "broken": { "neither": true }
            }
        })
        .to_string();
        let result = mcp::import_json(&json!({ "json": payload })).unwrap();
        let imported = result["imported"].as_array().unwrap();
        assert_eq!(imported.len(), 2);
        assert_eq!(result["errors"].as_array().unwrap().len(), 1);

        let listed = mcp::list(&json!({})).unwrap();
        for server in listed["servers"].as_array().unwrap() {
            assert_eq!(server["trusted"], false, "imports default untrusted");
            assert_eq!(server["source"], "import_json");
        }
    });
}

#[test]
fn mcp_enabled_runtime_configs_projects_rows() {
    with_temp_db(|| {
        mcp::create(&json!({
            "id": "docs",
            "name": "Docs",
            "transport": "http",
            "url": "https://mcp.example.com",
            "trusted": true,
        }))
        .unwrap();
        mcp::create(&json!({
            "id": "disabled",
            "name": "Disabled",
            "transport": "http",
            "url": "https://other.example.com",
            "enabled": false,
        }))
        .unwrap();
        let configs = mcp::enabled_runtime_configs().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].id, "docs");
        assert!(configs[0].trusted);
    });
}

#[test]
fn skill_rescan_upserts_and_selection_fails_closed() {
    with_temp_db(|| {
        let project = tempfile::tempdir().unwrap();
        let skill_dir = project.path().join(".natives/skills/demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Demo\n\nDo the demo thing.\n").unwrap();

        let stats = skills::rescan(&json!({ "projectRoot": project.path() })).unwrap();
        assert!(stats["new"].as_u64().unwrap() >= 1);

        let listed = skills::list(&json!({ "scope": "project" })).unwrap();
        let demo = listed["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "demo")
            .expect("demo skill discovered");
        let demo_id = demo["id"].as_str().unwrap().to_string();

        // Category tagging via update.
        let updated =
            skills::update(&json!({ "id": demo_id, "category": "效率", "tags": ["demo"] }))
                .unwrap();
        assert_eq!(updated["skill"]["category"], "效率");

        // A freshly scanned *project* skill is discovered but not trusted:
        // a skill body carries the same authority as the system prompt, so
        // dropping a file into a project tree must not grant it. Only the
        // `$HOME` namespace is trusted by rule (see `skill_store`).
        let untrusted = skills::prompt_for_selection(std::slice::from_ref(&demo_id)).unwrap_err();
        assert_eq!(untrusted, vec![demo_id.clone()]);

        // Trust is an explicit act. Once granted, selection delivers the body —
        // progressive disclosure keeps bodies out of the *catalog*, not out of
        // a selection the user made on purpose.
        skills::update(&json!({ "id": demo_id, "trusted": true })).unwrap();
        let prompt = skills::prompt_for_selection(std::slice::from_ref(&demo_id)).unwrap();
        assert!(prompt.contains("Do the demo thing"));

        // Unknown id fails closed with the missing list.
        let missing =
            skills::prompt_for_selection(&[demo_id.clone(), "user:ghost".into()]).unwrap_err();
        assert_eq!(missing, vec!["user:ghost".to_string()]);

        // Disabled skill fails closed too.
        skills::update(&json!({ "id": demo_id, "enabled": false })).unwrap();
        let missing = skills::prompt_for_selection(std::slice::from_ref(&demo_id)).unwrap_err();
        assert_eq!(missing, vec![demo_id]);
    });
}

#[test]
fn skill_delete_remove_dir_only_for_imports_inside_skills_root() {
    with_temp_db(|| {
        let project = tempfile::tempdir().unwrap();
        let skill_dir = project.path().join(".natives/skills/scanme");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Scan\n\nbody\n").unwrap();
        skills::rescan(&json!({ "projectRoot": project.path() })).unwrap();
        let listed = skills::list(&json!({})).unwrap();
        let id = listed["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "scanme")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        // Scan-sourced rows refuse remove_dir (files are the user's own).
        let err = skills::delete(&json!({ "id": id, "mode": "remove_dir" })).unwrap_err();
        assert!(err.contains("imported"), "{err}");
        // Unregister keeps the directory.
        skills::delete(&json!({ "id": id })).unwrap();
        assert!(skill_dir.exists());
    });
}

#[test]
fn host_subagent_migration_runs_once_and_maps_fields() {
    with_temp_db(|| {
        let dir = tempfile::tempdir().unwrap();
        let natives_db = dir.path().join("natives.db");
        let host = rusqlite::Connection::open(&natives_db).unwrap();
        host.execute_batch(
            "CREATE TABLE subagents (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL DEFAULT '',
                instructions TEXT NOT NULL DEFAULT '', tools TEXT NOT NULL DEFAULT '',
                provider_id TEXT, provider_key_id TEXT, model_id TEXT NOT NULL DEFAULT '',
                fallback_enabled INTEGER NOT NULL DEFAULT 0, max_runs INTEGER NOT NULL DEFAULT 10,
                enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            INSERT INTO subagents VALUES
              ('h1','Writer','copywriter','Write well.','read_file, search_files',
               'openai','auto','gpt-4o',0,10,1,'2026-01-01','2026-01-01'),
              ('h2','Empty','','','','anthropic','k9','',0,10,0,'2026-01-01','2026-01-01');",
        )
        .unwrap();
        drop(host);

        let migrated = experts::migrate_host_subagents_from(&natives_db).unwrap();
        assert_eq!(migrated, 2);

        let writer = experts::get(&json!({ "id": "h1" })).unwrap();
        assert_eq!(writer["expert"]["systemPrompt"], "Write well.");
        assert_eq!(writer["expert"]["source"], "host_migration");
        // 'auto' key routing is dropped (IDs only).
        assert!(writer["expert"]["keyId"].is_null());
        assert_eq!(writer["expert"]["tools"][1], "search_files");

        let empty = experts::get(&json!({ "id": "h2" })).unwrap();
        assert_eq!(empty["expert"]["systemPrompt"], "You are Empty.");
        assert_eq!(empty["expert"]["enabled"], false);
        assert_eq!(empty["expert"]["keyId"], "k9");

        // Second invocation is a no-op (one-shot guard).
        let again = experts::migrate_host_subagents_from(&natives_db).unwrap();
        assert_eq!(again, 0);
    });
}

#[tokio::test]
async fn dispatch_rejects_unknown_method() {
    let err = request("capability.mcp.hub.bogus", json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("unsupported"), "{err}");
}
