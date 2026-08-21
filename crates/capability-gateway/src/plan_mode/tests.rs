//! Plan Mode unit tests.

use super::session::{evict_settled, MAX_RETAINED_SESSIONS};
use super::*;
use crate::{PermissionClass, SideEffect};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str) -> String {
        format!("{name}-{}", uuid::Uuid::new_v4())
    }

    // -- gate ---------------------------------------------------------------

    #[test]
    fn read_only_project_tools_pass_the_latch() {
        for (name, se, pc) in [
            (
                "read_file",
                SideEffect::ReadOnly,
                PermissionClass::ProjectRead,
            ),
            ("grep", SideEffect::ReadOnly, PermissionClass::ProjectRead),
            (
                "skill",
                SideEffect::ReadOnly,
                PermissionClass::AlwaysAllowed,
            ),
        ] {
            assert_eq!(decision(name, se, pc), PlanDecision::Allow, "{name}");
        }
    }

    #[test]
    fn write_process_and_mcp_are_blocked() {
        for (name, se, pc) in [
            (
                "write_file",
                SideEffect::Write,
                PermissionClass::ProjectWrite,
            ),
            (
                "apply_patch",
                SideEffect::Write,
                PermissionClass::ProjectWrite,
            ),
            (
                "run_terminal",
                SideEffect::Process,
                PermissionClass::DestructiveCommand,
            ),
            ("task", SideEffect::Process, PermissionClass::ProjectWrite),
            (
                "mcp_call",
                SideEffect::Network,
                PermissionClass::ExternalWrite,
            ),
        ] {
            assert_eq!(decision(name, se, pc), PlanDecision::Deny, "{name}");
        }
    }

    #[test]
    fn unknown_write_tool_defaults_to_deny() {
        assert_eq!(
            decision(
                "some_future_tool",
                SideEffect::Write,
                PermissionClass::AlwaysAllowed
            ),
            PlanDecision::Deny
        );
    }

    #[test]
    fn network_read_tools_reach_the_profile_gate() {
        assert_eq!(
            decision(
                "web_fetch",
                SideEffect::Network,
                PermissionClass::ExternalWrite
            ),
            PlanDecision::Allow
        );
        assert_eq!(
            decision(
                "web_search",
                SideEffect::Network,
                PermissionClass::ExternalWrite
            ),
            PlanDecision::Allow
        );
    }

    #[test]
    fn control_tools_are_control() {
        assert_eq!(
            decision(
                EXIT_PLAN_MODE_TOOL,
                SideEffect::Write,
                PermissionClass::AlwaysAllowed
            ),
            PlanDecision::Control
        );
        assert_eq!(
            decision(
                ENTER_PLAN_MODE_TOOL,
                SideEffect::ReadOnly,
                PermissionClass::AlwaysAllowed
            ),
            PlanDecision::Control
        );
    }

    // -- plan parsing -------------------------------------------------------

    fn sample_plan_json() -> serde_json::Value {
        serde_json::json!({
            "plan": {
                "title": "Add retry to the uploader",
                "summary": "Wrap the upload call in a bounded retry.",
                "steps": [
                    {"title": "Read uploader.rs", "kind": "research"},
                    {
                        "title": "Add retry loop",
                        "kind": "edit",
                        "targets": ["src/uploader.rs"],
                        "risk": "medium"
                    },
                    {"title": "cargo test", "kind": "command", "reversible": false}
                ],
                "risks": ["Retry could mask a real auth failure"],
                "open_questions": ["Max attempts?"],
                "out_of_scope": ["Changing the transport"]
            }
        })
    }

    #[test]
    fn parses_a_full_plan() {
        let plan = parse_plan(&sample_plan_json()).unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].id, "s1");
        assert_eq!(plan.steps[1].kind, PlanStepKind::Edit);
        assert_eq!(plan.steps[1].targets, vec!["src/uploader.rs".to_string()]);
        assert_eq!(plan.peak_risk(), PlanRisk::Medium);
        assert!(plan.has_irreversible_step());
        assert_eq!(plan.open_questions.len(), 1);
    }

    #[test]
    fn accepts_an_unwrapped_plan_object() {
        let raw = sample_plan_json();
        let inner = raw.get("plan").unwrap().clone();
        assert_eq!(parse_plan(&inner).unwrap().steps.len(), 3);
    }

    #[test]
    fn rejects_empty_and_untitled_plans() {
        assert!(parse_plan(&serde_json::json!({})).is_err());
        assert!(parse_plan(&serde_json::json!({"title": "x"})).is_err());
        assert!(parse_plan(&serde_json::json!({"title": "x", "steps": []})).is_err());
        assert!(parse_plan(&serde_json::json!({
            "title": "x",
            "steps": [{"detail": "no title"}]
        }))
        .is_err());
    }

    #[test]
    fn rejects_duplicate_step_ids() {
        let err = parse_plan(&serde_json::json!({
            "title": "x",
            "steps": [{"id": "a", "title": "one"}, {"id": "a", "title": "two"}]
        }))
        .unwrap_err();
        assert_eq!(err.code, "invalid_plan");
        assert!(err.message.contains("duplicate"));
    }

    #[test]
    fn rejects_oversized_step_list() {
        let steps: Vec<serde_json::Value> = (0..MAX_PLAN_STEPS + 1)
            .map(|i| serde_json::json!({"title": format!("s{i}")}))
            .collect();
        assert!(parse_plan(&serde_json::json!({"title": "x", "steps": steps})).is_err());
    }

    #[test]
    fn command_steps_are_irreversible_unless_stated() {
        let plan = parse_plan(&serde_json::json!({
            "title": "x",
            "steps": [{"title": "rm build dir", "kind": "command"}]
        }))
        .unwrap();
        assert!(!plan.steps[0].reversible);
        assert_eq!(plan.steps[0].risk, PlanRisk::Medium);
    }

    // -- latch --------------------------------------------------------------

    #[test]
    fn enter_then_approve_restores_declared_profile() {
        let id = run("latch");
        assert!(!is_active(&id));
        enter(&id, "autonomous");
        assert!(is_active(&id));
        assert_eq!(effective_profile(&id, "autonomous"), PLAN_PROFILE);

        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(approve(&id).unwrap(), "autonomous");
        assert!(!is_active(&id));
        assert_eq!(effective_profile(&id, "autonomous"), "autonomous");
        clear(&id);
    }

    #[test]
    fn run_declared_as_plan_falls_back_to_ask_not_plan() {
        let id = run("declared");
        assert_eq!(effective_profile(&id, PLAN_PROFILE), PLAN_PROFILE);
        enter(&id, PLAN_PROFILE);
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(approve(&id).unwrap(), PLAN_DEFAULT_FALLBACK);
        assert_eq!(effective_profile(&id, PLAN_PROFILE), PLAN_DEFAULT_FALLBACK);
        clear(&id);
    }

    #[test]
    fn approval_without_a_submitted_plan_is_refused() {
        let id = run("no-plan");
        enter(&id, "ask");
        let err = approve(&id).unwrap_err();
        assert_eq!(err.code, "plan_missing");
        assert!(is_active(&id), "latch must stay closed");
        clear(&id);
    }

    #[test]
    fn rejection_keeps_the_latch_closed() {
        let id = run("reject");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(reject(&id).unwrap(), 1);
        assert_eq!(reject(&id).unwrap(), 2);
        assert!(is_active(&id));
        assert_eq!(effective_profile(&id, "ask"), PLAN_PROFILE);
        clear(&id);
    }

    #[test]
    fn re_entering_after_approval_does_not_reopen_the_latch() {
        let id = run("reenter");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        approve(&id).unwrap();
        // A model trying to hand the user a second approval card gets nothing.
        enter(&id, "ask");
        assert!(!is_active(&id));
        assert!(record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).is_err());
        clear(&id);
    }

    #[test]
    fn entering_never_escalates_a_readonly_run() {
        let id = run("readonly");
        enter(&id, "readonly");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(approve(&id).unwrap(), "readonly");
        clear(&id);
    }

    #[test]
    fn eviction_never_drops_a_live_latch() {
        let mut map: HashMap<String, PlanSession> = HashMap::new();
        let live = "live-run".to_string();
        map.insert(
            live.clone(),
            PlanSession {
                run_id: live.clone(),
                state: PlanState::Planning,
                fallback_profile: "ask".into(),
                plan: None,
                rejections: 0,
                // Oldest entry in the map: an age-only policy would evict it.
                entered_at: chrono::Utc::now() - chrono::Duration::days(1),
                approved_at: None,
            },
        );
        for i in 0..MAX_RETAINED_SESSIONS + 10 {
            let id = format!("settled-{i}");
            map.insert(
                id.clone(),
                PlanSession {
                    run_id: id,
                    state: PlanState::Approved,
                    fallback_profile: "ask".into(),
                    plan: None,
                    rejections: 0,
                    entered_at: chrono::Utc::now(),
                    approved_at: Some(chrono::Utc::now()),
                },
            );
        }
        evict_settled(&mut map);
        assert!(map.len() <= MAX_RETAINED_SESSIONS + 1);
        assert!(
            map.contains_key(&live),
            "an unapproved run must never lose its latch to eviction"
        );
    }

    // -- timeline events ----------------------------------------------------

    fn event_fields(
        kind: &assistant_protocol::v2::RunEventKind,
    ) -> (&str, &str, bool, Option<u32>, Option<&str>) {
        match kind {
            assistant_protocol::v2::RunEventKind::PlanModeChanged {
                transition,
                effective_profile,
                plan,
                rejections,
                reason,
            } => (
                transition,
                effective_profile,
                plan.is_some(),
                *rejections,
                reason.as_deref(),
            ),
            other => panic!("expected plan_mode_changed, got {}", other.type_name()),
        }
    }

    #[test]
    fn entering_reports_the_plan_gear_and_carries_no_plan() {
        let id = run("event-enter");
        let session = enter(&id, "autonomous");
        let kind = changed_event(
            &session,
            PlanTransition::Entered,
            Some("  wide blast radius  "),
        );
        let (transition, profile, has_plan, rejections, reason) = event_fields(&kind);
        assert_eq!(transition, "entered");
        assert_eq!(profile, PLAN_PROFILE);
        assert!(!has_plan, "there is no plan at entry");
        assert_eq!(rejections, None);
        assert_eq!(reason, Some("wide blast radius"), "reason must be trimmed");
        clear(&id);
    }

    #[test]
    fn approval_reports_the_restored_profile_not_the_plan_gear() {
        let id = run("event-approve");
        enter(&id, "autonomous");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();

        let submitted = changed_event(&snapshot(&id).unwrap(), PlanTransition::Submitted, None);
        let (transition, profile, has_plan, _, _) = event_fields(&submitted);
        assert_eq!(transition, "submitted");
        assert_eq!(
            profile, PLAN_PROFILE,
            "submitting must not read as having left Plan Mode"
        );
        assert!(has_plan, "the card content belongs on the timeline");

        approve(&id).unwrap();
        let approved = changed_event(&snapshot(&id).unwrap(), PlanTransition::Approved, None);
        let (transition, profile, has_plan, _, _) = event_fields(&approved);
        assert_eq!(transition, "approved");
        assert_eq!(profile, "autonomous");
        assert!(has_plan, "the timeline must show what was agreed to");
        clear(&id);
    }

    #[test]
    fn rejection_keeps_the_plan_gear_and_surfaces_the_count() {
        let id = run("event-reject");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        reject(&id).unwrap();
        reject(&id).unwrap();
        let kind = changed_event(&snapshot(&id).unwrap(), PlanTransition::Rejected, None);
        let (transition, profile, has_plan, rejections, _) = event_fields(&kind);
        assert_eq!(transition, "rejected");
        assert_eq!(
            profile, PLAN_PROFILE,
            "a rejected plan leaves the latch shut"
        );
        assert!(has_plan);
        assert_eq!(rejections, Some(2), "a loop has to be countable");
        clear(&id);
    }

    #[test]
    fn clearing_does_not_replay_a_stale_plan() {
        let id = run("event-clear");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        let kind = changed_event(&snapshot(&id).unwrap(), PlanTransition::Cleared, None);
        let (transition, _, has_plan, _, _) = event_fields(&kind);
        assert_eq!(transition, "cleared");
        assert!(!has_plan);
        clear(&id);
    }

    #[test]
    fn operations_on_an_unknown_run_fail_closed() {
        let id = run("unknown");
        assert_eq!(approve(&id).unwrap_err().code, "not_in_plan_mode");
        assert_eq!(reject(&id).unwrap_err().code, "not_in_plan_mode");
        assert!(record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).is_err());
    }
}
