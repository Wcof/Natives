use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

mod agent;
mod agent_skill_stats;
pub mod ai;
pub mod apps;
mod archive;
mod archive_ops;
pub mod assistant_service;
pub mod commands;
pub mod context_window;
/// KI-3 lint rules live in the `contract-linter` crate so the Agent Daemon runs
/// the identical ruleset on drafts (ADR-0014 section 8.1). Re-exported here to keep the
/// existing `natives_lib::contract_linter::…` call sites intact.
pub mod contract_linter;
pub mod creative_app;
pub mod creative_draft;
pub mod credential_broker;
mod credential_broker_lease;
pub mod daemon;
pub mod daemon_authority;
mod daemon_watchdog;
pub mod db;
mod disk_usage;
mod env_manager;
mod error;
pub mod execution_engine_settings;
pub mod executor_catalog;
pub mod file_indexer;
pub mod file_manager;
mod fs_watch;
mod ghostty_config;
#[cfg(feature = "ghostty-vt")]
mod ghostty_vt;
mod git;
mod handler_registration;
mod html_preview;
mod http_server;
mod image_convert;
pub mod integrations;
pub mod jobs;
pub mod key_lease;
pub mod key_pool;
mod lid_guard;
mod locate;
pub mod log_sanitizer;
mod module_manager;
mod permission_center;
pub mod provider_accounts;
mod provider_accounts_parse;
pub mod provider_key_manager;
pub mod proxy;
mod release_wizard;
mod runtime;
mod screenshot;
mod search;
pub mod secrets;
pub mod sequence_id;
pub mod sidecar_supervisor;
mod terminal;
mod terminal_ghostty;
mod terminal_process;
mod terminal_recorder;
mod thumbnail;
mod token_manager;
pub mod update_checker;
pub mod usage;
pub mod vendor_whitelist;
mod wechat;
mod workspace;

pub use error::{Error, Result};

// Feature-gated re-exports
#[cfg(feature = "ghostty-vt")]
pub use ghostty_vt::GhosttyTerminal;

/// Emit a `db-state-changed` event so the frontend can react to state changes
/// without polling.  All write commands (theme, locale, module, notification, env)
/// must call this helper to satisfy the "unidirectional bus" constraint (KI-4).
///
/// The payload carries `version + sequence_id` so consumers can reconcile state
/// and discard out-of-order events.
pub fn emit_db_state_changed<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    channel: &str,
    data: serde_json::Value,
) {
    let payload = crate::sequence_id::envelope(channel, data);
    let _ = app_handle.emit("db-state-changed", payload);
}

/// Application state shared across commands
pub struct AppState {
    pub db: db::DbPool,
    pub token_manager: std::sync::Arc<token_manager::TokenManager>,
    pub http_port: Mutex<u16>,
    pub terminal_manager: terminal::TerminalManager,
    pub ghostty_manager: terminal::GhosttyManager,
    pub terminal_recorder: std::sync::Arc<terminal_recorder::Recorder>,
    pub screenshot_stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub usage_cache: usage::UsageCache,
    pub fs_watcher: fs_watch::FsWatcher,
    pub lid_guard: lid_guard::LidGuard,
    pub wechat_bridge: Mutex<Option<wechat::bridge::Bridge>>,
    pub skills_trash_slots: tokio::sync::Semaphore,
    pub proxy_runtime: std::sync::Arc<proxy::ProxyRuntime>,
    pub oauth_sessions: std::sync::Arc<ai::oauth::session::OauthSessionManager>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonValue {
    pub value: serde_json::Value,
}

/// Ensure macOS system traffic lights (close/minimize/zoom) are visible.
/// Requires decorations=true + TitleBarStyle::Overlay (tauri.macos.conf.json).
#[cfg(target_os = "macos")]
fn apply_macos_traffic_lights(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_decorations(true);
        let _ = window.set_title_bar_style(tauri::TitleBarStyle::Overlay);
    }
}

/// Idempotent process teardown shared by every true-exit path
/// (`RunEvent::ExitRequested`/`Exit` and the `menubar_quit` command).
///
/// MB-P0-01: window close only hides — it never reaches here. A `Once` guard
/// guarantees PTY terminals, Ghostty sessions, local creative process trees and
/// the Agent Daemon are reaped exactly once per process even if several exit
/// paths fire back-to-back.
pub(crate) fn shutdown_all_processes(app: &tauri::AppHandle) {
    static CLEANUP: std::sync::Once = std::sync::Once::new();
    CLEANUP.call_once(|| {
        // 1. PTY terminals + Ghostty sessions.
        if let Some(state) = app.try_state::<AppState>() {
            state.ghostty_manager.kill_all();
            state.terminal_manager.kill_all();
        }
        // 2. Local creative process trees (supervised dev servers).
        if let Some(local_rt) = app.try_state::<creative_app::local::LocalRuntimeHandle>() {
            let handle = app.clone();
            let rt = local_rt.inner().clone();
            tauri::async_runtime::block_on(async move {
                creative_app::local::shutdown_all(rt.as_ref(), Some(&handle)).await;
            });
        }
        // 3. Agent Daemon (UDS sidecar) — graceful, idempotent under concurrency.
        let _ = sidecar_supervisor::global_supervisor().shutdown();
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // Persist size/position/maximized only.
        // DECORATIONS must not be restored: an older frameless session would
        // permanently hide macOS traffic lights. VISIBLE is also excluded so
        // the FOUC guard (visible:false → theme_ready_signal → show) stays
        // authoritative.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch reuses the existing Host (contract §4): hide the
            // menubar popup, then show/unminimize/focus the main window. Close
            // is never a teardown path — cleanup only happens on true exit.
            if let Some(popup) = app.get_webview_window(commands::menubar::MENUBAR_LABEL) {
                let _ = popup.hide();
            }
            let _ = commands::menubar::open_main_window(app);
        }))
        .setup(|app| {
            // Initialize SQLite database at ~/.natives/natives.db
            let data_dir = app
                .path()
                .home_dir()
                .map_err(|e| format!("failed to get home dir: {e}"))?
                .join(".natives");
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("failed to create .natives dir: {e}"))?;
            let db_path = data_dir.join("natives.db");
            // Production credential path contract (must be absolute).
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            let runtime_dir = data_dir.join("runtime");
            let _ = std::fs::create_dir_all(&runtime_dir);
            std::env::set_var("NATIVES_RUNTIME_DIR", &runtime_dir);
            // Phase 0: daemon authority store is assistant.db (credentials stay natives.db).
            let assistant_db_path = data_dir.join("assistant.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &assistant_db_path);
            // Production default: UDS. Tests/dev may override with embedded/auto.
            if std::env::var("NATIVES_DAEMON_MODE").is_err() {
                std::env::set_var("NATIVES_DAEMON_MODE", "uds");
            }
            let pool = db::init_db_pool(&db_path)
                .map_err(|e| format!("failed to init database pool: {e}"))?;
            {
                let conn = pool
                    .get()
                    .map_err(|e| format!("failed to initialize subagent tables: {e}"))?;
                commands::subagent::ensure_tables(&conn)
                    .map_err(|e| format!("failed to initialize subagent tables: {e}"))?;
            }
            // 注册主 pool 到全局，供 runtime 等无 State 上下文模块访问
            db::register_main_pool(pool.clone());
            // Host Credential Broker (W3 P0-04): bind the private mode-0600
            // UDS listener BEFORE spawning the daemon so the sidecar's lease
            // client can reach it. Production UDS mode always starts it;
            // Embedded only exists under test/diagnostic (architecture #6).
            {
                let broker_path =
                    crate::credential_broker::credential_broker_uds::broker_socket_path()
                        .map_err(|e| format!("broker socket path: {e}"))?;
                crate::credential_broker::credential_broker_uds::spawn_broker_uds_listener(
                    &broker_path,
                )
                .map_err(|e| format!("broker listener: {e}"))?;
            }
            // Embedded credential inject (when mode falls back to embedded for tests).
            // Embedded 仅存在于 test/diagnostic（架构目标 #6），随 cfg 隔离。
            #[cfg(any(test, feature = "diagnostic"))]
            {
                use std::sync::Once;
                static BROKER: Once = std::sync::Once::new();
                BROKER.call_once(|| {
                    natives_agent_daemon::install_credential_broker(std::sync::Arc::new(
                        |provider_id: &str, key_id: Option<&str>, run_id: &str| {
                            credential_broker::resolve_for_daemon(provider_id, key_id, run_id)
                        },
                    ));
                });
            }
            // Sidecar supervisor: try start when UDS required; never silent embedded.
            {
                let status = sidecar_supervisor::global_supervisor().status();
                let require = matches!(
                    std::env::var("NATIVES_DAEMON_MODE")
                        .unwrap_or_else(|_| "host".into())
                        .to_ascii_lowercase()
                        .as_str(),
                    "uds" | "sidecar" | "remote"
                );
                if require {
                    match sidecar_supervisor::global_supervisor().ensure_started() {
                        Ok(s) => {
                            eprintln!(
                                "[natives] sidecar supervisor state={:?} production_ready={}",
                                s.state, s.production_ready
                            );
                            if !s.production_ready {
                                return Err(format!(
                                    "UDS required but supervisor is not production-ready: {:?}",
                                    s.last_error
                                )
                                .into());
                            }

                            daemon_watchdog::start();
                        }
                        Err(e) => {
                            return Err(format!(
                                "UDS daemon unavailable (no embedded fallback): {e}"
                            )
                            .into());
                        }
                    }
                } else {
                    let _ = status;
                }
            }

            // ── Window Vibrancy (macOS Liquid Glass) ──
            // CSS backdrop-filter blur() is configured in globals.css for cross-platform glass effect.
            // Native NSVisualEffectView vibrancy can be added here when a Tauri v2 plugin becomes
            // available (e.g. tauri-plugin-vibrancy). The window is already "transparent": true in
            // tauri.conf.json, so applying a native vibrancy view will work out of the box.
            // On Tauri v2, use: window.set_background_color(Color::TRANSPARENT) and then apply
            // NSVisualEffectView via the raw window handle (app.get_webview_window("main").unwrap()).

            // Initialize token manager (needs a connection from the pool)
            let init_conn = pool
                .get()
                .map_err(|e| format!("failed to get DB connection: {e}"))?;
            let tm = std::sync::Arc::new(token_manager::TokenManager::new(&init_conn));
            drop(init_conn); // return connection to pool

            // Initialize modules directory
            let modules_dir = data_dir.join("modules");
            std::fs::create_dir_all(&modules_dir)
                .map_err(|e| format!("failed to create modules dir: {e}"))?;

            // Initialize assistant database (isolated from core natives.db)
            // W1: Host-owned tables (jobs + provider mirror) live in natives.db
            // (see db::ensure_host_owned_tables inside init_db_pool); the Host
            // no longer opens the Daemon's assistant.db.

            // Scheduler 双权威收敛：Host Job runner 启动前，一次性把旧 Daemon
            // scheduler/jobs.json 事务性导入 scheduled_tasks。源文件保留，成功后
            // 另存只读备份与完成 marker；冲突 fail-closed，禁止两份定义并跑。
            {
                let mut assistant_conn = db::get_main_conn()
                    .map_err(|e| format!("failed to open main database for jobs: {e}"))?;
                let legacy_jobs = runtime_dir.join("scheduler").join("jobs.json");
                let report = jobs::migration::migrate_legacy_scheduler_jobs(
                    &mut assistant_conn,
                    &legacy_jobs,
                    chrono::Utc::now(),
                )
                .map_err(|e| format!("legacy scheduler migration failed: {e}"))?;
                if report.source_found && !report.already_migrated {
                    eprintln!(
                        "[jobs] migrated {} legacy scheduler job(s); marker={}",
                        report.imported,
                        report.marker_path.display()
                    );
                }
            }

            // Job 任务模块：常驻 30s tick 循环（Once 幂等；列迁移已在
            // natives.db 初始化内经 jobs::store::ensure_schema 补齐）
            jobs::runner::start();

            // Pre-warm env encryption key cache from SQLite settings.
            {
                let pool_conn = pool
                    .get()
                    .map_err(|e| format!("failed to get DB connection for env key init: {e}"))?;
                let conn: &rusqlite::Connection = &pool_conn;
                env_manager::init_env_encryption_key(conn)
                    .map_err(|e| format!("failed to init env encryption key: {e}"))?;
            }

            // Start local HTTP server for module assets and bridge API
            let mut server = http_server::HttpServer::new(modules_dir, tm.clone(), db_path.clone());
            let port = server.start(0).unwrap_or_else(|e| {
                eprintln!("failed to start HTTP server: {e}");
                0
            });

            // Ensure builtin tools have DB rows (INSERT OR IGNORE — idempotent)
            {
                let seed_conn = pool
                    .get()
                    .map_err(|e| format!("failed to get DB connection: {e}"))?;
                let _ = db::seed_builtin_tool(&seed_conn, "terminal", "native");
                let _ = db::seed_builtin_tool(&seed_conn, "editor", "native");
                let _ = db::seed_builtin_tool(&seed_conn, "browser", "native");
            }

            let terminal_recorder = std::sync::Arc::new(terminal_recorder::Recorder::new());
            let mut terminal_manager = terminal::TerminalManager::new();
            terminal_manager.set_recorder(terminal_recorder.clone());

            app.manage(AppState {
                db: pool,
                token_manager: tm,
                http_port: Mutex::new(port),
                terminal_manager,
                ghostty_manager: terminal::GhosttyManager::new(),
                terminal_recorder,
                screenshot_stop_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
                usage_cache: usage::UsageCache::new(),
                fs_watcher: fs_watch::FsWatcher::new(app.handle().clone()),
                lid_guard: lid_guard::LidGuard::new(),
                wechat_bridge: Mutex::new(Some(wechat::bridge::Bridge::new())),
                skills_trash_slots: tokio::sync::Semaphore::new(2),
                proxy_runtime: std::sync::Arc::new(proxy::ProxyRuntime::new()),
                oauth_sessions: std::sync::Arc::new(ai::oauth::session::OauthSessionManager::new()),
            });

            // RunWatchStreamV2 host watch bridge (persistent run.watch —
            // NE-P0-01 §19.1): Renderer→Host→UDS→Daemon single stream. Host
            // previously did not manage WatchBridgeState nor register the
            // run_watch_start/stop/state commands; this is the only wiring.
            app.manage(Arc::new(Mutex::new(
                daemon::watch_bridge::WatchBridgeState::default(),
            )));

            // Creative App: global mutation lock + child browser state (ADR-0013)
            // + local project runtime supervisor (process tree / logs)
            app.manage(creative_app::service::new_mutation_lock());
            app.manage(std::sync::Mutex::new(
                creative_app::browser::BrowserState::new(),
            ));
            app.manage(creative_app::local::new_runtime_manager());
            // T08: OAuth temp-surface flow registry (shareable Arc for handlers).
            app.manage(std::sync::Arc::new(
                creative_app::oauth::OAuthFlowRegistry::default(),
            ));

            // Converge leftover installing/starting/stopping/deleting vs Docker labels
            {
                if let Ok(conn) = db::get_main_conn() {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build();
                        if let Ok(rt) = rt {
                            rt.block_on(async {
                                // Any operation still in flight when the previous
                                // Host process died is stale; settle it so the
                                // Renderer never sees an eternally-busy app (CR-201).
                                let _ =
                                    crate::creative_app::operation::settle_stale_on_startup(&conn);
                                // T07: reconcile DB windows vs real child WebViews —
                                // a window whose WebView died with the old process is
                                // explicitly marked missing/closed; a WebView with no
                                // DB row is closed as an orphan. Never fabricates an
                                // open state.
                                let _ = crate::creative_app::window::reconcile_all(&handle, &conn);
                                let _ = creative_app::install::reconcile_all(&conn, Some(&handle))
                                    .await;
                                let _ = creative_app::local::lifecycle::reconcile_local_apps(
                                    &conn,
                                    Some(&handle),
                                );
                            });
                        }
                    });
                }
            }

            // The local runtime owns Child handles only while this process is alive.
            // Poll independently of the renderer so an exited dev server cannot
            // remain shown as running until the user revisits the Workshop page.
            {
                let watchdog_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
                    loop {
                        tick.tick().await;
                        let pool = watchdog_handle.state::<AppState>().db.clone();
                        let runtime = watchdog_handle
                            .state::<creative_app::local::LocalRuntimeHandle>()
                            .inner()
                            .clone();
                        let handle = watchdog_handle.clone();
                        // No mutation lock: exit polling writes are DB-CAS-guarded
                        // (batch 1 CR-102), so a long lifecycle op on one app must
                        // not stall the watchdog (batch 2 CR-202 #04).
                        let _ = tokio::task::spawn_blocking(move || {
                            let c = pool
                                .get()
                                .map_err(|e| Error::Internal(format!("local watchdog db: {e}")))?;
                            let rt = tokio::runtime::Handle::current();
                            rt.block_on(creative_app::local::lifecycle::poll_and_reconcile_exits(
                                &c,
                                Some(&handle),
                                runtime.as_ref(),
                            ))
                        })
                        .await;
                    }
                });
            }

            // ── Daemon DB authority ──
            // The Agent Daemon owns assistant.db exclusively. The Host never
            // opens, writes, ATTACHes or migrates it (MODULAR §10.3 /
            // §11.3.2: cross_db_host = 0). Provider mirror / projects /
            // settings are Host-owned and live in the Host's own natives.db
            // (db::ensure_host_owned_tables at init_db_pool time). Legacy
            // assistant_* session migration is owned by the Daemon's
            // host_authority_migration, not by the Host.

            // ── P1 Runtime 抽象层：仅注册独立 CLI runtime ──
            // Native Assistant 的生产执行入口是 Protocol v2 Agent Daemon；
            // 不再把已退役的协调器注册为第二个执行权威。
            {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| format!("failed to create tokio runtime: {e}"))?;
                // 注册 Claude CLI + Codex CLI runtime（独立产品能力，不承载 Native Assistant）。
                rt.block_on(runtime::registry::register(std::sync::Arc::new(
                    runtime::claude_cli::ClaudeCliRuntime::new(),
                )));
                rt.block_on(runtime::registry::register(std::sync::Arc::new(
                    runtime::codex_cli::CodexCliRuntime::new(),
                )));
            }

            // ── macOS menubar: single Tray (Tauri 2 core, no new plugin) ──
            // icons/ has no dedicated `Template`-suffixed asset, so the existing
            // icon.png is reused and marked as a template on macOS (black+alpha
            // rendering in both light/dark menu bars). Left click toggles the
            // lazily-created menubar popup (contract §3 / MB-P0-04).
            {
                let mut tray_builder =
                    tauri::tray::TrayIconBuilder::with_id(commands::menubar::MENUBAR_TRAY_ID)
                        .tooltip("Natives")
                        .icon(
                            tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
                                .map_err(|e| format!("failed to load tray icon: {e}"))?,
                        );
                #[cfg(target_os = "macos")]
                {
                    tray_builder = tray_builder.icon_as_template(true);
                }
                tray_builder
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let _ = commands::menubar::toggle_menubar(tray.app_handle());
                        }
                    })
                    .build(app)
                    .map_err(|e| format!("failed to build tray icon: {e}"))?;
            }

            // FOUC guard: window starts hidden (tauri.conf.json has visible: false)
            // It will be shown by theme_ready_signal command from frontend
            //
            // macOS traffic lights require decorations + Overlay title bar
            // (see tauri.macos.conf.json). Re-assert after plugins so an old
            // window-state file or race cannot leave us frameless.
            #[cfg(target_os = "macos")]
            apply_macos_traffic_lights(app.handle());

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                // MB-P0-01: window close is never a teardown path. Main red
                // close and the menubar popup close only hide — Host, Daemon,
                // Jobs, terminals and supervised processes keep running. Real
                // cleanup happens exactly once on true exit: RunEvent
                // ExitRequested/Exit or the `menubar_quit` command.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let label = window.label();
                    if label == "main" || label == commands::menubar::MENUBAR_LABEL {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                // Popup blur (focus lost) → hide only, debounced so a tray click
                // that causes the blur can still toggle the popup (contract §3).
                tauri::WindowEvent::Focused(false) => {
                    if window.label() == commands::menubar::MENUBAR_LABEL {
                        commands::menubar::schedule_blur_hide(window.app_handle());
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(handler_registration::register_invoke_handlers())
        .build(tauri::generate_context!())
        .expect("error while building natives")
        .run(|app_handle, event| {
            // Window CloseRequested is no longer an exit path (close → hide).
            // True exit converges here and in `menubar_quit`:
            //   Cmd+Q / App::exit / menu Quit → ExitRequested → Exit.
            match event {
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                    crate::shutdown_all_processes(app_handle);
                }
                // macOS Dock "Reopen": restore the main window when no window
                // is visible (contract §4).
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen {
                    has_visible_windows,
                    ..
                } => {
                    if !has_visible_windows {
                        let _ = commands::menubar::open_main_window(app_handle);
                    }
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod env_manager_tests;
