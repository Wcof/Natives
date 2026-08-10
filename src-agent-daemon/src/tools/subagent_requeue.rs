//! Retry re-queue, sibling-aggregate failure handling, SubagentStop hook and
//! persona redaction helpers (split from `subagent.rs` by responsibility,
//! ARCH-002).

use agent_core::{HookEvent, HookRequest, SubAgentManager, SubAgentStatus};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::production::TaskRecord;

/// Create a fresh child run for a Retry re-queue and bump the retry counter.
#[allow(clippy::too_many_arguments)] // re-queue needs the exact child scope
pub(crate) async fn requeue_child_run(
    session_id: &str,
    child_conversation_id: &str,
    parent_run_id: &str,
    binding: &crate::subagent_store::RouteBinding,
    retry_number: &u32,
    reason: &str,
    child_perm: &str,
    child_allowlist: &[String],
    child_profile_id: &Option<String>,
    child_directive: &Option<String>,
    child_max_steps: u32,
) -> Result<String, String> {
    let session = crate::subagent_store::get_subagent_session(session_id)?
        .ok_or_else(|| format!("subagent session not found: {session_id}"))?;
    // NE-P0-08: the re-queued run restores the EXACT persona. The durable
    // pending directive (same text + digest, verified) wins; the in-memory
    // argument is only a fallback for legacy sessions created before
    // directives were persisted. A digest mismatch is treated as corruption
    // and fails closed on the durable copy rather than silently re-rolling.
    let (directive_text, _directive_digest) = directive_for_requeue(
        crate::subagent_store::pending_directive_for_session(session_id)
            .ok()
            .flatten(),
        child_directive.as_deref(),
    )
    .map(|(text, digest)| (text, digest))
    .unwrap_or_default();
    let project_path = session.project_path.clone();
    let prompt = if session.task.trim().is_empty() {
        format!("Retry (attempt {retry_number}) after: {reason}")
    } else {
        session.task.to_string()
    };
    let created = crate::child_run_orchestrator::create_child_run(
        crate::child_run_orchestrator::ChildRunSpec {
            conversation_id: child_conversation_id.to_string(),
            provider_id: binding.provider_id.clone(),
            model_id: binding.model_id.clone(),
            key_id: Some(binding.key_id.clone()),
            agent_profile_id: child_profile_id.clone(),
            permission_profile: Some(child_perm.to_string()),
            content: Some(prompt.clone()),
            max_steps: Some(child_max_steps),
            parent_run_id: Some(parent_run_id.to_string()),
            project_path,
            runtime_id: Some("native".into()),
        },
    )
    .await?;
    crate::child_run_orchestrator::apply_child_surface(
        &created.id,
        child_allowlist.to_vec(),
        if directive_text.is_empty() {
            None
        } else {
            Some(directive_text)
        },
    )
    .await;
    crate::child_run_orchestrator::start_child_run(assistant_protocol::v2::StartRunRequest {
        agent_profile_id: None,
        capability_selection: None,
        run_id: Some(created.id.clone()),
        conversation_id: Some(child_conversation_id.to_string()),
        provider_id: Some(binding.provider_id.clone()),
        model_id: Some(binding.model_id.clone()),
        key_id: Some(binding.key_id.clone()),
        content: Some(prompt),
        attachments: None,
        trigger_message_id: None,
        permission_profile: Some(child_perm.to_string()),
        max_steps: Some(child_max_steps),
        project_path: session.project_path.clone(),
        idempotency_key: None,
        effort: None,
        runtime_id: Some("native".into()),
    })
    .await?;
    let _ = crate::subagent_store::bump_subagent_retry(session_id);
    Ok(created.id)
}

/// (all_other_siblings_terminal, any_other_sibling_failed) for the parent's
/// task ledger. Used by RequireAll to detect the aggregate outcome.
pub(crate) async fn sibling_settled_state(
    task_outputs: &Arc<Mutex<HashMap<String, TaskRecord>>>,
    exclude_task_id: &str,
) -> (bool, bool) {
    let map = task_outputs.lock().await;
    let mut any_running = false;
    let mut any_failed = false;
    for (tid, rec) in map.iter() {
        if tid == exclude_task_id {
            continue;
        }
        if rec.status == "running" {
            any_running = true;
        }
        if rec.status == "failed" {
            any_failed = true;
        }
    }
    (!any_running, any_failed)
}

/// FailFast / RequireAll aggregate action: cancel every sibling child's engine
/// and metadata, then fail the parent run (idempotent).
pub(crate) async fn fail_parent_and_cancel_siblings(
    subagents: &Arc<SubAgentManager>,
    parent_run_id: &str,
    reason: &str,
) {
    for sibling in subagents.get_children(parent_run_id).await {
        let _ = subagents
            .update_status(&sibling.id, SubAgentStatus::Cancelled)
            .await;
        crate::global_run_manager()
            .runtime
            .cancel_run(&sibling.run_id)
            .await;
    }
    let _ = crate::global_run_manager()
        .runtime
        .cancel_run(parent_run_id)
        .await;
    crate::global_run_manager().fail_run_if_active(
        parent_run_id,
        reason.to_string(),
        "SUBAGENT_FAILFAST",
    );
}

/// SubagentStop hook fired for every terminal outcome (matches prior behavior).
pub(crate) async fn fire_subagent_stop(
    project_path: &Option<String>,
    parent_run_id: &str,
    child_run_id: &str,
    status: &str,
    output: &str,
) {
    let _ = crate::production_hooks::build_production_hooks_for_project(
        project_path.as_deref().map(std::path::Path::new),
    )
    .dispatch(HookRequest {
        event: HookEvent::SubagentStop,
        run_id: parent_run_id.to_string(),
        tool_name: Some("task".into()),
        input: serde_json::json!({
            "sub_run_id": child_run_id,
            "status": status,
            "output": output,
        }),
    })
    .await;
}

pub(crate) fn use_fixture_flag(input: &Value) -> bool {
    input
        .get("fixture")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

/// NE-P0-08: resolve the directive to apply on a re-queued run. The durable
/// pending directive wins when its recorded digest matches a fresh hash of the
/// text (crash-safe: same text + same digest). A digest mismatch is treated as
/// corruption — the in-memory argument is used instead, never a silently
/// re-rolled durable persona. Legacy sessions fall back to the in-memory arg.
fn directive_for_requeue(
    durable: Option<crate::subagent_store::PendingDirective>,
    in_memory: Option<&str>,
) -> Option<(String, String)> {
    if let Some(pd) = durable {
        if crate::subagent_store::directive_sha256_hex(&pd.text) == pd.digest {
            return Some((pd.text, pd.digest));
        }
    }
    in_memory.filter(|s| !s.trim().is_empty()).map(|s| {
        (
            s.to_string(),
            crate::subagent_store::directive_sha256_hex(s),
        )
    })
}

/// 19.3-⑤ field-level visibility for the Parent Tool Input's `system_prompt`:
/// exports/logs get a digest marker in place of the persona text, other fields
/// pass through untouched. The digest lets an operator verify the same persona
/// was restored without ever exposing the prompt body.
pub fn redact_task_input_system_prompt(input: &Value) -> Value {
    let mut out = input.clone();
    let Some(obj) = out.as_object_mut() else {
        return out;
    };
    for key in ["system_prompt", "agent_prompt"] {
        if let Some(v) = obj.get_mut(key) {
            if let Some(text) = v.as_str().filter(|s| !s.trim().is_empty()) {
                *v = Value::String(format!(
                    "[REDACTED system_prompt sha256:{}]",
                    crate::subagent_store::directive_sha256_hex(text)
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::SubAgentConfig;

    /// T05 FailFast action: one child failure cancels every sibling and fails
    /// the parent run — the parent-side outcome the watcher applies.
    #[tokio::test]
    async fn fail_fast_action_cancels_siblings_and_fails_parent() {
        let _env = crate::storage::DataStore::env_test_lock();
        // Restore env on drop so `NATIVES_RUN_MANAGER_MEMORY` cannot leak into
        // later tests in the same process.
        let _env_restore = crate::storage::EnvRestore::capture();
        // Memory-only global RunManager (hermetic; the durable budget side is
        // covered by the subagent_store restart/recovery tests).
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        let rm = crate::run_manager::RunManager::new();
        let make_run = |conversation_id: &str, parent: Option<&str>| {
            rm.create_run(assistant_protocol::v2::CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: conversation_id.to_string(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("x".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: parent.map(|p| p.to_string()),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap()
        };
        let probe_parent = make_run("c-ff", None);
        let parent = make_run("c-ff", None);
        let child_a = make_run("c-ff", Some(&parent.id));
        let child_b = make_run("c-ff", Some(&parent.id));

        let subagents = Arc::new(SubAgentManager::new(SubAgentConfig::default()));
        subagents.register_root_depth(&parent.id).await;
        let a = subagents
            .register(
                "session-a".into(),
                child_a.id.clone(),
                &parent.id,
                "a".into(),
                1,
                "openai".into(),
                "k".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let b = subagents
            .register(
                "session-b".into(),
                child_b.id.clone(),
                &parent.id,
                "b".into(),
                1,
                "openai".into(),
                "k".into(),
                "gpt-4o".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                None,
                None,
            )
            .await
            .unwrap();

        crate::run_manager::install_global_for_test(rm);

        // Probe: a queued run must be able to fail (FailFast depends on it).
        let commit_result = crate::global_run_manager().commit_status(
            &probe_parent.id,
            assistant_protocol::v2::RunStatusV2::Failed,
            agent_core::TransitionMetadata::empty()
                .with_error_code("PROBE")
                .with_lifecycle_hint("failed"),
        );
        match &commit_result {
            Ok(run) => assert_eq!(
                run.status.as_str(),
                "failed",
                "commit_status Ok must report the failed run: {run:?}"
            ),
            Err(e) => panic!("queued → failed must be legal for FailFast: {e}"),
        }

        fail_parent_and_cancel_siblings(&subagents, &parent.id, "injected child failure").await;

        assert_eq!(
            subagents.get(&a.id).await.unwrap().status,
            SubAgentStatus::Cancelled,
            "FailFast must cancel sibling A"
        );
        assert_eq!(
            subagents.get(&b.id).await.unwrap().status,
            SubAgentStatus::Cancelled,
            "FailFast must cancel sibling B"
        );
        let parent_run = crate::global_run_manager().get_run(&parent.id).unwrap();
        assert_eq!(
            parent_run.status.as_str(),
            "failed",
            "FailFast must fail the parent run"
        );
        crate::run_manager::install_memory_global_for_test();
    }

    // ── NE-P0-08 / 19.3-①: retry restores the exact persona ──

    #[test]
    fn directive_for_requeue_prefers_verified_durable_copy() {
        let text = "You are a terse Rust reviewer.";
        let digest = crate::subagent_store::directive_sha256_hex(text);
        let durable = Some(crate::subagent_store::PendingDirective {
            text: text.to_string(),
            digest: digest.clone(),
            persisted_at: "2026-01-01T00:00:00Z".into(),
        });
        let got =
            directive_for_requeue(durable, Some("in-memory persona")).expect("durable copy wins");
        assert_eq!(got.0, text, "retry uses the exact same text");
        assert_eq!(got.1, digest, "retry uses the exact same digest");
    }

    #[test]
    fn directive_for_requeue_falls_back_when_durable_missing_or_corrupt() {
        // No durable copy: legacy session → in-memory arg wins. The text is
        // preserved byte-for-byte (19.3-①: retry restores the same text and
        // digest — trimming would silently re-roll the persona).
        let got =
            directive_for_requeue(None, Some("  in-memory persona  ")).expect("in-memory fallback");
        assert_eq!(got.0, "  in-memory persona  ", "in-memory arg is preserved");
        assert_eq!(
            got.1,
            crate::subagent_store::directive_sha256_hex("  in-memory persona  "),
            "digest matches the preserved text"
        );
        // Digest mismatch (corruption signal): never silently re-roll the
        // durable text — fall back to the in-memory arg instead.
        let corrupt = Some(crate::subagent_store::PendingDirective {
            text: "tampered persona".into(),
            digest: crate::subagent_store::directive_sha256_hex("original"),
            persisted_at: "2026-01-01T00:00:00Z".into(),
        });
        let got = directive_for_requeue(corrupt, Some("in-memory persona"))
            .expect("in-memory fallback on digest mismatch");
        assert_eq!(got.0, "in-memory persona");
        assert_eq!(
            got.1,
            crate::subagent_store::directive_sha256_hex("in-memory persona")
        );
        // Neither source → None.
        assert!(directive_for_requeue(None, None).is_none());
        assert!(directive_for_requeue(None, Some("   ")).is_none());
    }

    // ── 19.3-⑤: field-level redaction of the Parent Tool Input ──

    #[test]
    fn redact_task_input_hides_system_prompt_text_keeps_digest() {
        let input = serde_json::json!({
            "prompt": "do the thing",
            "system_prompt": "secret persona body",
            "agent_prompt": "also secret",
            "max_steps": 5,
        });
        let redacted = redact_task_input_system_prompt(&input);
        let serialized = redacted.to_string();
        assert!(!serialized.contains("secret persona body"), "text redacted");
        assert!(!serialized.contains("also secret"), "text redacted");
        assert!(
            serialized.contains("[REDACTED system_prompt sha256:"),
            "digest marker"
        );
        assert_eq!(redacted["prompt"], "do the thing", "other fields untouched");
        assert_eq!(redacted["max_steps"], 5, "other fields untouched");
        // The digest marker contains the real digest for field-level verification.
        let expected_digest = crate::subagent_store::directive_sha256_hex("secret persona body");
        assert!(
            serialized.contains(&expected_digest),
            "digest stays visible for verification"
        );
        // Non-object input passes through untouched.
        assert_eq!(
            redact_task_input_system_prompt(&serde_json::json!("plain")),
            "plain"
        );
    }
}
