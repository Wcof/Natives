use crate::rpc::handlers::run::{cap_wire_replay, run_manager};
use crate::rpc::{send_error, send_success, write_stream_frame};
use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
use assistant_protocol::v2::ReplayRunRequest;

pub(crate) async fn handle_run_watch(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v2::V2Request,
) {
    // Persistent event stream (RunWatchStreamV2, STREAM-CONTRACT-V2):
    // 1) validate the run exists,
    // 2) establish the durable receiver AND the live receiver
    //    (subscribe_after with bounded live replay) BEFORE the ACK so no
    //    replay→subscribe window is lost,
    // 3) return a normal RPC ACK `{stream:"run.watch", streamVersion:2}`,
    // 4) replay durable events > after_durable_sequence,
    // 5) then tokio::select! multiplex durable / live / heartbeat.
    //
    // - durable receiver lag  → gap-fill by replaying from the cursor
    //   (durable facts are replayable).
    // - live receiver lag     → ResyncRequired(live); never fabricate a
    //   durable fact from the ephemeral lane.
    // - long idle             → Heartbeat so the 30s client frame
    //   timeout never fires on a healthy stream.
    // - terminal durable event → clean close.
    // - The two lanes never share a sequence namespace.
    let after_durable = request
        .params
        .get("after_durable_sequence")
        .or_else(|| request.params.get("after_sequence"))
        .or_else(|| request.params.get("last_sequence"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let after_live = request
        .params
        .get("after_live_sequence")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let run_id = request
        .params
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if run_id.is_empty() {
        send_error(
            writer,
            &request.request_id,
            &DaemonError::new(
                error_codes::INVALID_INPUT,
                ErrorCategory::Validation,
                false,
                "run.watch requires run_id".to_string(),
            ),
        )
        .await;
        return;
    }

    // 1) Run must exist before we establish any receiver or ACK.
    if run_manager().get_run(&run_id).is_none() {
        send_error(
            writer,
            &request.request_id,
            &DaemonError::new(
                error_codes::NOT_FOUND,
                ErrorCategory::NotFound,
                false,
                format!("run not found: {run_id}"),
            ),
        )
        .await;
        return;
    }

    // 2) Establish durable + live receivers BEFORE the ACK.
    let mut durable_rx = run_manager().events().subscribe(&run_id);

    // S1 frozen API: runtime.live_events() -> LiveEventBus. The bounded
    // ring replay arrives atomically with the live receiver.
    let live_bus = run_manager().runtime.live_events();
    let live_sub = live_bus.subscribe_after(&run_id, after_live);
    let mut live_rx = live_sub.receiver;
    let mut live_cursor = live_sub.last_sequence;
    let live_gap = live_sub.gap;
    let live_replay = live_sub.buffered;

    // 3) ACK — a normal RPC Response, then frames.
    send_success(
        writer,
        &request.request_id,
        &request.client_id,
        &request.session_token,
        crate::stream_protocol::stream_ack(),
    )
    .await;

    // 4) Replay durable events > after_durable_sequence.
    let replayed = match run_manager().replay_checked(ReplayRunRequest {
        run_id: run_id.clone(),
        after_sequence: after_durable,
    }) {
        Ok(events) => events,
        Err(error) => {
            // After the ACK the client is in frame mode; a clean close
            // (reconnect by cursor) is safer than a malformed error frame.
            let _ = error;
            return;
        }
    };
    let mut durable_seq = after_durable;
    for event in cap_wire_replay(replayed) {
        let seq = event.run_sequence;
        durable_seq = durable_seq.max(seq);
        if write_stream_frame(
            writer,
            &crate::stream_protocol::RunStreamFrameV2::durable_event(&run_id, &event),
        )
        .await
        .is_err()
        {
            return; // client dropped — clean exit, no orphan task
        }
    }

    // 5a) Replay the bounded live prefix (> after_live_sequence) after
    //     the durable replay. Never touches the durable cursor.
    for ev in live_replay {
        if ev.live_sequence <= after_live {
            continue;
        }
        live_cursor = live_cursor.max(ev.live_sequence);
        if write_stream_frame(
            writer,
            &crate::stream_protocol::RunStreamFrameV2::live_event(&run_id, &ev),
        )
        .await
        .is_err()
        {
            return;
        }
    }
    // If the requested live cursor fell before the bounded ring start,
    // some ephemeral deltas are unrecoverable → ResyncRequired(live).
    // This is never a durable-fact gap.
    if live_gap
        && write_stream_frame(
            writer,
            &crate::stream_protocol::RunStreamFrameV2::resync_required(
                &run_id,
                crate::stream_protocol::RunStreamLane::Live,
                "live_buffer_gap",
            ),
        )
        .await
        .is_err()
    {
        return;
    }

    // 5b) If the run already reached terminal, clean close — never hang
    //     waiting for a broadcast that will not come.
    if run_manager()
        .get_run(&run_id)
        .map(|r| r.status.is_terminal())
        .unwrap_or(false)
    {
        return;
    }

    // 5c) Live push phase: multiplex durable / live / heartbeat.
    use crate::stream_protocol::{RunStreamFrameV2, RunStreamLane};
    let heartbeat_interval = std::time::Duration::from_secs(15);
    let mut heartbeat = tokio::time::interval(heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await; // consume the immediate first tick
    let mut last_frame_at = std::time::Instant::now();

    loop {
        tokio::select! {
            result = durable_rx.recv() => {
                match result {
                    Ok(event) => {
                        let seq = event.run_sequence;
                        if seq <= durable_seq {
                            continue; // already forwarded
                        }
                        durable_seq = seq;
                        if write_stream_frame(writer, &RunStreamFrameV2::durable_event(&run_id, &event)).await.is_err() {
                            return; // client dropped
                        }
                        last_frame_at = std::time::Instant::now();
                        if event.payload.is_terminal() {
                            return; // terminal durable fact — clean close
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Durable facts are replayable: gap-fill by cursor.
                        let replay = match run_manager().replay_checked(ReplayRunRequest {
                            run_id: run_id.clone(),
                            after_sequence: durable_seq,
                        }) {
                            Ok(events) => events,
                            Err(_) => return,
                        };
                        for event in cap_wire_replay(replay) {
                            let seq = event.run_sequence;
                            if seq <= durable_seq {
                                continue;
                            }
                            durable_seq = seq;
                            if write_stream_frame(writer, &RunStreamFrameV2::durable_event(&run_id, &event)).await.is_err() {
                                return;
                            }
                            last_frame_at = std::time::Instant::now();
                            if event.payload.is_terminal() {
                                return;
                            }
                        }
                    }
                    Err(_) => return, // broadcast closed — clean close
                }
            }
            result = live_rx.recv() => {
                match result {
                    Ok(ev) => {
                        if ev.live_sequence <= live_cursor {
                            continue;
                        }
                        live_cursor = ev.live_sequence;
                        if write_stream_frame(writer, &RunStreamFrameV2::live_event(&run_id, &ev)).await.is_err() {
                            return;
                        }
                        last_frame_at = std::time::Instant::now();
                    }
                    Err(_) => {
                        // Live bus lag → ResyncRequired(live). Never
                        // fabricate a durable fact from the live lane.
                        if write_stream_frame(
                            writer,
                            &RunStreamFrameV2::resync_required(
                                &run_id,
                                RunStreamLane::Live,
                                "live_buffer_gap",
                            ),
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        last_frame_at = std::time::Instant::now();
                    }
                }
            }
            _ = heartbeat.tick() => {
                // Interval ticks can arrive a few microseconds before
                // `elapsed()` reaches its nominal duration. Accept that
                // scheduler jitter so a 15s heartbeat is not skipped,
                // which would create a 30s gap at the client timeout.
                if last_frame_at.elapsed() + std::time::Duration::from_millis(100) >= heartbeat_interval {
                    if write_stream_frame(
                        writer,
                        &RunStreamFrameV2::heartbeat(&run_id, durable_seq, live_cursor),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                    last_frame_at = std::time::Instant::now();
                }
            }
        }
    }
}
