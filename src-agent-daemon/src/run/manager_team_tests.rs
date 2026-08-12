use super::*;

#[tokio::test]
async fn expert_team_roster_spawns_real_child_run_and_rejects_outsiders() {
    let _env_guard = crate::storage::DataStore::env_test_lock();
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    // DB-backed Expert profiles so `load_profile("builder")` resolves (the task
    // tool fail-closes when a roster member's profile cannot be loaded).
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(format!("team-{}.db", Uuid::new_v4()));
    let art = dir.path().join("artifacts");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
    std::env::set_var("NATIVES_DB_PATH", &db);
    crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
    let _store = crate::storage::DataStore::new(&db, &art).unwrap();
    for id in ["builder", "lead"] {
        crate::capability::experts::create(&serde_json::json!({
            "id": id,
            "name": id,
            "systemPrompt": format!("You are {id}."),
        }))
        .unwrap();
    }
    let rt = crate::production::ProductionRuntime::new();
    let team = crate::capability_resolution::ResolvedTeam {
        team_id: "growth".into(),
        lead_expert_id: "lead".into(),
        members: vec![
            crate::capability_resolution::ResolvedTeamMember {
                expert_id: "builder".into(),
                name: "Builder".into(),
                description: String::new(),
                role_hint: "builds features".into(),
            },
            crate::capability_resolution::ResolvedTeamMember {
                expert_id: "lead".into(),
                name: "Lead".into(),
                description: String::new(),
                role_hint: String::new(),
            },
        ],
        failure_policy: "isolate".into(),
        max_concurrent: 3,
    };
    let tools = crate::production::PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            let _ = g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        interactions: rt.interactions.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engine_handles().await,
        runtime: None,
        provider_id: "openai".into(),
        key_id: Some("parent-team-key".into()),
        parent_run_id: "team-parent".into(),
        conversation_id: "c-team".into(),
        model_id: "gpt-4o".into(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
        team: Some(team),
        mcp_tool_schemas: Vec::new(),
        selected_mcp_servers: None,
    };

    // 1) roster member spawns a real child run via the fixture provider.
    let member = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "agent": "builder",
                "prompt": "child work",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(
        !member.is_error,
        "roster member task error: {:?}",
        member.output
    );
    let child_run_id = member
        .output
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    assert!(!child_run_id.is_empty(), "child run id must be returned");
    let evs = rt.events.replay_after("team-parent", 0);
    assert!(
        evs.iter()
            .any(|e| matches!(e.payload, RunEventKind::SubagentCreated { .. })),
        "expected SubagentCreated for roster-member child run"
    );
    // The child run is a REAL daemon run row: task_output resolves it to a
    // run_id. Fixture mode (runtime: None) has no live engine for children, so
    // the run stays queued/running until killed — the created run row itself is
    // the proof of an actually-created child run, never a fake green.
    let out = tools
        .execute_tool(
            "task_output",
            serde_json::json!({ "task_id": child_run_id }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!out.is_error, "task_output error: {:?}", out.output);
    let run_id = out
        .output
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        !run_id.is_empty(),
        "child run row must exist (run_id present)"
    );
    // Kill the queued child so the test leaves no live run behind.
    let kill = tools
        .execute_tool(
            "kill_task",
            serde_json::json!({ "task_id": child_run_id }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!kill.is_error, "kill_task error: {:?}", kill.output);

    // 2) non-roster agent is rejected fail-closed.
    let outsider = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "agent": "outsider",
                "prompt": "should be rejected",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(outsider.is_error);
    assert_eq!(
        outsider.output.get("code").and_then(|v| v.as_str()),
        Some("TEAM_MEMBER_INVALID")
    );

    // 3) without a team, `agent` is rejected fail-closed.
    let no_team_tools = crate::production::PermissionGatedTools {
        team: None,
        ..tools
    };
    let no_team = no_team_tools
        .execute_tool(
            "task",
            serde_json::json!({
                "agent": "builder",
                "prompt": "no team",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(no_team.is_error);
    assert_eq!(
        no_team.output.get("code").and_then(|v| v.as_str()),
        Some("TEAM_NOT_ACTIVE")
    );
}
