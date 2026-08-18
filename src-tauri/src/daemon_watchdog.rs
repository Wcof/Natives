//! Host-owned Agent Daemon watchdog.

use std::time::Duration;

/// Drive the existing supervisor without owning any process state.
pub(crate) fn start() {
    tauri::async_runtime::spawn(async move {
        let supervisor = crate::sidecar_supervisor::global_supervisor();
        let mut previous = supervisor.status();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if !supervisor.watchdog_should_run() {
                break;
            }
            let polled =
                match tokio::task::spawn_blocking(move || supervisor.poll_child_health()).await {
                    Ok(status) => status,
                    Err(error) => {
                        eprintln!("[natives] daemon watchdog poll worker failed: {error}");
                        continue;
                    }
                };
            if previous.pid != polled.pid || previous.production_ready != polled.production_ready {
                crate::daemon_authority::reset_authority_cache().await;
            }
            let outcome =
                tokio::task::spawn_blocking(move || supervisor.ensure_healthy_or_restart()).await;
            let current = supervisor.status();
            if polled.pid != current.pid || polled.production_ready != current.production_ready {
                crate::daemon_authority::reset_authority_cache().await;
            }
            if previous.state != current.state {
                match &outcome {
                    Ok(Ok(_)) => eprintln!(
                        "[natives] daemon watchdog state={:?} production_ready={}",
                        current.state, current.production_ready
                    ),
                    Ok(Err(error)) => eprintln!("[natives] daemon watchdog fault: {error}"),
                    Err(error) => {
                        eprintln!("[natives] daemon watchdog restart worker failed: {error}")
                    }
                }
            }
            previous = current;
        }
    });
}
