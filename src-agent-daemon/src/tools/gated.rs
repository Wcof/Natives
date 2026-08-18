//! `PermissionGatedTools` facade + dependency wiring.
//!
//! Owns the struct definition, the `checkpoint_manager` constructor helper,
//! and the small retained `EngineToolRuntime` entry point. The model-visible
//! schema/capability surface and the execution + permission-gating impls live
//! in the nested `gated_schemas` / `gated_execute` modules. Domain decisions
//! live in the sibling modules: `permission`, `plan`, `mcp`, `subagent`,
//! `progress`, `artifact`, `invocation`, and `policy`.

use agent_core::{
    AgentEngine, EngineToolRuntime, EventSequencer, HookEvent, HookRequest, NoopToolProgressSink,
    PermissionManager, SubAgentManager, ToolExecutionResult, ToolProgressSink, ToolProgressUpdate,
    ToolSchema,
};
use assistant_protocol::v2::RunEventKind;
use capability_gateway::plan_mode::{self, PlanDecision};
use capability_gateway::{CapabilityGateway, SideEffect};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::{attach_tool_output_artifact, model_visible_tool_schemas};
use crate::production::{ProductionRuntime, TaskRecord};

/// Schema/capability surface and execution impls, split out of this file so
/// the facade keeps only the struct + constructor helper.
mod gated_execute;
mod gated_schemas;

/// Tools with permission gate + real task orchestration.
pub struct PermissionGatedTools {
    pub gateway: Arc<CapabilityGateway>,
    pub permissions: Arc<PermissionManager>,
    pub events: EventSequencer,
    pub interactions: Arc<crate::runtime::InteractionHub>,
    pub subagents: Arc<SubAgentManager>,
    pub task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    /// Shared with ProductionRuntime so kill_task / cascade can cancel live engines.
    pub engines: Arc<Mutex<HashMap<String, Arc<AgentEngine>>>>,
    pub runtime: Option<Arc<ProductionRuntime>>,
    pub provider_id: String,
    /// Real parent run key_id when known (never `"auto"`).
    pub key_id: Option<String>,
    pub parent_run_id: String,
    pub conversation_id: String,
    pub model_id: String,
    pub permission_profile: String,
    /// `None` = parent/unrestricted surface (permission profile still applies).
    /// `Some` = hard allowlist; tools outside the list are hidden and denied.
    pub tool_allowlist: Option<Vec<String>>,
    /// Resolved expert team for this run (ADR-0016): the task tool's `agent`
    /// parameter must name a member; runs without a team reject it.
    pub team: Option<crate::capability_resolution::ResolvedTeam>,
    /// Model-visible MCP tool schemas for the selected servers (ADR-0016).
    /// Empty = no MCP schemas surfaced (legacy runs expose none either).
    pub mcp_tool_schemas: Vec<ToolSchema>,
    /// `Some(set)` = server whitelist: `mcp__{server}__*` and `mcp_call` may
    /// only target these servers. `None` = legacy behaviour.
    pub selected_mcp_servers: Option<std::collections::HashSet<String>>,
}

impl PermissionGatedTools {
    fn checkpoint_manager(&self) -> &crate::checkpoint::CheckpointManager {
        match self.runtime.as_deref() {
            Some(runtime) => runtime.checkpoint_manager(),
            None => crate::checkpoint::global_checkpoint_manager(),
        }
    }
}

/// A4 — ReadOnly Fast Path regression tests.

#[cfg(test)]
mod readonly_fast_path_tests {
    use super::*;
    use crate::production::ProductionRuntime;
    use tokio_util::sync::CancellationToken;

    fn tools_for(run_id: &str, profile: &str, root: &std::path::Path) -> PermissionGatedTools {
        let rt = ProductionRuntime::new();
        let mut gateway = CapabilityGateway::new();
        gateway.set_project_root(root.to_string_lossy().to_string());
        let _ = gateway.register_builtins();
        PermissionGatedTools {
            gateway: Arc::new(gateway),
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "test".into(),
            key_id: None,
            parent_run_id: run_id.to_string(),
            conversation_id: format!("conv-{run_id}"),
            model_id: "test-model".into(),
            permission_profile: profile.to_string(),
            tool_allowlist: None,
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        }
    }

    /// A4: a genuinely ReadOnly tool must take the fast path — no side-effect
    /// ledger row, no checkpoint snapshot. ToolCallStarted/ToolCallCompleted
    /// remain durable facts (engine loop), so the invocation is still
    /// observable; only checkpoint/ledger/lease overhead is skipped.
    #[tokio::test]
    async fn readonly_tool_does_not_create_side_effect_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().unwrap();
        let run_id = format!("a4-ro-{}", uuid::Uuid::new_v4());
        let tools = tools_for(&run_id, "full_access", &root);
        // read_file is classified ReadOnly by the Gateway capability registry.
        let project_file = root.join("probe.txt");
        std::fs::write(&project_file, "a4 probe").expect("write probe");
        let out = tools
            .execute_tool(
                "read_file",
                serde_json::json!({ "path": project_file.to_string_lossy() }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error, "read_file must succeed: {:?}", out.output);
        // The ledger must have NO record for this run (fast path skipped it).
        let watermark = crate::side_effect_ledger::ledger_watermark(&run_id)
            .ok()
            .flatten();
        assert_eq!(
            watermark.as_deref(),
            Some("0"),
            "readonly tool must not create side-effect ledger records"
        );
    }

    /// B2 — Coding Read Loop (05 §4): list_dir + read_file×5 + grep×3 +
    /// read_file×4. ReadOnly tools must leave ZERO side-effect ledger rows and
    /// ZERO checkpoint snapshots, while each call still succeeds.
    #[tokio::test]
    async fn readonly_coding_loop_does_not_create_ledger_or_checkpoint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().unwrap();
        let run_id = format!("a4-b2-{}", uuid::Uuid::new_v4());
        let tools = tools_for(&run_id, "full_access", &root);
        for i in 0..6 {
            std::fs::write(
                root.join(format!("src-{i}.rs")),
                format!("// probe {i}\nfn f{i}() {{}}\n"),
            )
            .expect("write fixture");
        }

        // list_dir
        let out = tools
            .execute_tool(
                "list_dir",
                serde_json::json!({ "path": root.to_string_lossy() }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error, "list_dir must succeed: {:?}", out.output);

        // read_file × 5 + grep × 3 + read_file × 4 (B2 sequence)
        let mut reads = 0usize;
        for i in 0..5 {
            let out = tools
                .execute_tool(
                    "read_file",
                    serde_json::json!({ "path": root.join(format!("src-{i}.rs")).to_string_lossy() }),
                    &CancellationToken::new(),
                )
                .await;
            assert!(
                !out.is_error,
                "read_file #{i} must succeed: {:?}",
                out.output
            );
            reads += 1;
        }
        for i in 0..3 {
            let out = tools
                .execute_tool(
                    "grep",
                    serde_json::json!({ "pattern": "fn f", "path": root.to_string_lossy() }),
                    &CancellationToken::new(),
                )
                .await;
            assert!(!out.is_error, "grep #{i} must succeed: {:?}", out.output);
        }
        for i in 0..4 {
            let out = tools
                .execute_tool(
                    "read_file",
                    serde_json::json!({ "path": root.join(format!("src-{i}.rs")).to_string_lossy() }),
                    &CancellationToken::new(),
                )
                .await;
            assert!(
                !out.is_error,
                "read_file #b{i} must succeed: {:?}",
                out.output
            );
            reads += 1;
        }
        assert_eq!(reads, 9, "9 read_file calls executed");

        // Zero ledger rows for the whole loop.
        let watermark = crate::side_effect_ledger::ledger_watermark(&run_id)
            .ok()
            .flatten();
        assert_eq!(
            watermark.as_deref(),
            Some("0"),
            "B2 read loop must not create side-effect ledger records"
        );

        // Zero checkpoint snapshots for the run.
        let checkpoint = self_checkpoint_snapshots(&run_id);
        assert_eq!(
            checkpoint, 0,
            "B2 read loop must not create checkpoint snapshots"
        );
    }

    /// Count checkpoint snapshots persisted for a run (public query).
    fn self_checkpoint_snapshots(run_id: &str) -> usize {
        use crate::checkpoint::global_checkpoint_manager;
        match global_checkpoint_manager().checkpoint_for_run_public(run_id) {
            Ok(preview) => preview.files.len(),
            Err(_) => 0,
        }
    }

    /// §5 exact-name regression: readonly tools do not checkpoint (A4/B2).
    #[test]
    fn readonly_tool_does_not_checkpoint() {
        readonly_coding_loop_does_not_create_ledger_or_checkpoint();
    }

    #[tokio::test]
    async fn creative_proposal_fails_when_approval_fact_cannot_be_persisted() {
        let _env_guard = crate::storage::DataStore::env_test_lock();
        crate::run_manager::install_memory_global_for_test();
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical root");
        let tools = tools_for("proposal-no-store", "full_access", &root);

        let out = tools
            .execute_tool(
                crate::proposal_fact::PROPOSAL_TOOL,
                serde_json::json!({
                    "schemaVersion": 1,
                    "kind": "create",
                    "ownership": "managed",
                    "title": "Dashboard",
                    "projectRoot": root,
                    "driver": {"kind": "staticHttp"},
                    "openPath": "/",
                    "healthPath": "/",
                    "environmentKeys": [],
                }),
                &CancellationToken::new(),
            )
            .await;

        assert!(
            out.is_error,
            "undurable proposal must fail: {:?}",
            out.output
        );
        assert_eq!(out.output["ok"], serde_json::json!(false));
        assert_eq!(out.output["error_code"], "PERSISTENCE_FAILED");
        assert!(
            !out.output.to_string().contains("Awaiting user approval"),
            "an undurable proposal must not claim approval is pending"
        );
    }

    /// NE-P0-05 §19.5: the gated tool runtime shares the Run's frozen Hook
    /// Dispatcher with the permission gate and the notification hook. Two
    /// resolves for the same run return the same dispatcher (idempotent freeze),
    /// and the plan hash is stable — a mid-Run `hooks.json` edit cannot replace
    /// it. This is the consistency guarantee the notification and permission
    /// tool paths rely on.
    #[tokio::test]
    async fn frozen_dispatcher_is_shared_and_stable_across_tool_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().unwrap();
        // Seed a hooks.json so the lazy compile fallback has a real plan.
        // The loopback URL is rejected by URL validation before any socket is
        // opened, so it is a fast hermetic probe — the definition is created
        // but the handler is inert. We never dispatch here, only resolve.
        const PROBE_URL: &str = "http://127.0.0.1:1/hook";
        std::fs::create_dir_all(root.join(".natives")).expect("create .natives dir");
        std::fs::write(
            root.join(".natives").join("hooks.json"),
            format!(r#"{{"hooks":{{"Notification":[{{"hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}]}}}}"#),
        )
        .expect("write hooks.json");
        let run_id = format!("ne-p0-05-stable-{}", uuid::Uuid::new_v4());
        let tools = tools_for(&run_id, "full_access", &root);
        let project_root = Some(root.as_path());
        let first = crate::production_hooks::resolve_frozen_dispatcher(
            &run_id,
            tools.events.clone(),
            project_root,
        );
        let plan_hash = first.plan_hash().to_string();
        // Second resolve for the same run must return the same frozen dispatcher.
        let second = crate::production_hooks::resolve_frozen_dispatcher(
            &run_id,
            tools.events.clone(),
            project_root,
        );
        assert_eq!(
            second.plan_hash(),
            plan_hash,
            "two resolves for the same run must agree on the plan hash"
        );
        // Mid-run edit: add a second hook group. The frozen plan must not change.
        std::fs::write(
            root.join(".natives").join("hooks.json"),
            format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        )
        .expect("overwrite hooks.json");
        let third = crate::production_hooks::resolve_frozen_dispatcher(
            &run_id,
            tools.events.clone(),
            project_root,
        );
        assert_eq!(
            third.plan_hash(),
            plan_hash,
            "a mid-Run hooks.json edit must not change the frozen plan hash"
        );
        // A *new* run started after the edit gets a different plan hash.
        let new_run_id = format!("ne-p0-05-new-{}", uuid::Uuid::new_v4());
        let new_tools = tools_for(&new_run_id, "full_access", &root);
        let new_plan = crate::production_hooks::resolve_frozen_dispatcher(
            &new_run_id,
            new_tools.events.clone(),
            project_root,
        );
        assert_ne!(
            new_plan.plan_hash(),
            plan_hash,
            "a new run after the edit must get a different plan hash"
        );
    }
}
