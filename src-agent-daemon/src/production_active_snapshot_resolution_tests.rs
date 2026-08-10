use super::*;

fn snapshot(text: &str) -> crate::conversation_store::ActiveContextSnapshot {
    crate::conversation_store::ActiveContextSnapshot {
        messages: vec![agent_core::AgentMessage::System(
            agent_core::SystemMessage {
                message_id: agent_core::MessageId::new(),
                text: text.to_string(),
            },
        )],
        input_message_ids: Default::default(),
    }
}

fn system_text(snapshot: &crate::conversation_store::ActiveContextSnapshot) -> String {
    match &snapshot.messages[0] {
        agent_core::AgentMessage::System(system) => system.text.clone(),
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn checkpoint_snapshot_wins_over_latest_without_consulting_it() {
    let mut consulted = false;
    let resolved = resolve_active_snapshot_for_start(true, Some(snapshot("checkpoint")), || {
        consulted = true;
        Ok(Some(snapshot("latest")))
    })
    .unwrap();
    assert!(
        !consulted,
        "latest snapshot must not be read when a checkpoint snapshot exists"
    );
    assert_eq!(system_text(&resolved.unwrap()), "checkpoint");
}

#[test]
fn continue_without_checkpoint_snapshot_fails_closed_even_with_latest() {
    let error =
        resolve_active_snapshot_for_start(true, None, || Ok(Some(snapshot("latest")))).unwrap_err();
    assert!(
        error.contains("active context snapshot"),
        "stable fail-closed error expected, got: {error}"
    );
}

#[test]
fn fresh_run_falls_back_to_latest_conversation_snapshot() {
    let resolved =
        resolve_active_snapshot_for_start(false, None, || Ok(Some(snapshot("latest")))).unwrap();
    assert_eq!(system_text(&resolved.unwrap()), "latest");
}
