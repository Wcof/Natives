use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as TokioMutex;

mod agent;
mod archive;
mod archive_ops;
pub mod assistant_service;
pub mod commands;
pub mod context_window;
/// KI-3 lint rules live in the `contract-linter` crate so the Agent Daemon runs
/// the identical ruleset on drafts (ADR-0014 section 8.1). Re-exported here to keep the
/// existing `natives_lib::contract_linter::…` call sites intact.
pub use contract_linter;
pub mod creative_app;
pub mod creative_draft;
pub mod credential_broker;
pub mod daemon;
pub mod daemon_authority;
pub mod db;
mod disk_usage;
mod env_manager;
mod error;
pub mod executor_catalog;
pub mod file_manager;
mod fs_watch;
mod ghostty_config;
#[cfg(feature = "ghostty-vt")]
mod ghostty_vt;
mod git;
mod html_preview;
mod http_server;
mod image_convert;
pub mod jobs;
pub mod key_lease;
mod lid_guard;
mod locate;
pub mod log_sanitizer;
mod module_manager;
mod permission_center;
pub mod provider_accounts;
pub mod provider_key_manager;
mod release_wizard;
mod runtime;
mod screenshot;
mod search;
pub mod sequence_id;
pub mod sidecar_supervisor;
mod terminal;
mod terminal_recorder;
mod thumbnail;
mod token_manager;
pub mod update_checker;
pub mod usage;
pub mod vendor_whitelist;
mod wechat;

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
pub fn emit_db_state_changed(
    app_handle: &tauri::AppHandle,
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
            // Focus the existing window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
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
            // Embedded credential inject (when mode falls back to embedded for tests).
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
                        .unwrap_or_else(|_| "uds".into())
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
            db::init_assistant_db()
                .map_err(|e| format!("failed to init assistant database: {e}"))?;

            // Scheduler 双权威收敛：Host Job runner 启动前，一次性把旧 Daemon
            // scheduler/jobs.json 事务性导入 scheduled_tasks。源文件保留，成功后
            // 另存只读备份与完成 marker；冲突 fail-closed，禁止两份定义并跑。
            {
                let mut assistant_conn = db::get_assistant_db_conn()
                    .map_err(|e| format!("failed to open assistant database for jobs: {e}"))?;
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
            // init_assistant_db 内经 jobs::store::ensure_schema 补齐）
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
            });

            // Creative App: global mutation lock + child browser state (ADR-0013)
            // + local project runtime supervisor (process tree / logs)
            app.manage(creative_app::service::new_mutation_lock());
            app.manage(std::sync::Mutex::new(
                creative_app::browser::BrowserState::new(),
            ));
            app.manage(creative_app::local::new_runtime_manager());

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

            // ── Initialize Assistant Store (in-process, no sidecar) ──
            // The assistant database (~/.natives/assistant.db) is managed directly
            // through the DataStore, which handles its own migrations and WAL setup.
            let assistant_db_path = data_dir.join("assistant.db");
            let assistant_data_store = std::sync::Arc::new(
                daemon::data::DataStore::new(&assistant_db_path.to_string_lossy())
                    .map_err(|e| format!("failed to init assistant store: {e}"))?,
            );
            app.manage(TokioMutex::new(assistant_service::AssistantStore::new(
                assistant_data_store,
            )));

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
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if let Some(state) = window.try_state::<AppState>() {
                    state.ghostty_manager.kill_all();
                    state.terminal_manager.kill_all();
                }
                // Stop all local creative process trees on normal exit.
                if let Some(local_rt) =
                    window.try_state::<creative_app::local::LocalRuntimeHandle>()
                {
                    let handle = window.app_handle().clone();
                    let rt = local_rt.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        creative_app::local::shutdown_all(rt.as_ref(), Some(&handle)).await;
                    });
                }
                // Graceful agent-daemon sidecar shutdown (wipe bootstrap file).
                let _ = sidecar_supervisor::global_supervisor().shutdown();
            }
        })
        .invoke_handler(tauri::generate_handler![
            // App
            commands::app::app_version,
            // DB
            commands::db::db_get,
            commands::db::db_set,
            commands::db::db_delete,
            commands::db::db_list,
            // Theme
            commands::theme::get_theme,
            commands::theme::set_theme,
            commands::theme::builtin_tool_ghostty_sync_theme,
            // Locale
            commands::locale::get_locale,
            commands::locale::set_locale,
            // Shell
            commands::shell::show_item_in_folder,
            commands::shell::open_path,
            // Window
            commands::window::window_minimize,
            commands::window::window_maximize,
            commands::window::window_toggle_fullscreen,
            commands::window::window_close,
            commands::window::window_is_maximized,
            commands::window::window_is_fullscreen,
            commands::window::window_tile,
            // Clipboard
            commands::clipboard::clipboard_write,
            commands::clipboard::clipboard_read,
            // Terminal
            commands::terminal::terminal_create,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_kill,
            commands::terminal::terminal_cwd,
            commands::terminal::terminal_proc,
            commands::terminal::terminal_session_state,
            commands::terminal::terminal_list_sessions,
            #[cfg(feature = "ghostty-vt")]
            commands::terminal::terminal_render_state,
            // Plugins
            commands::plugins::plugin_detect,
            commands::plugins::plugin_install,
            commands::plugins::plugin_uninstall,
            // Builtin Tool Registry
            commands::terminal::builtin_tool_detect,
            commands::terminal::builtin_tool_launch,
            commands::terminal::builtin_tool_list,
            commands::terminal::builtin_tool_update,
            commands::terminal::builtin_tool_seed,
            commands::terminal::builtin_tool_ghostty_is_running,
            commands::terminal::builtin_tool_ghostty_focus,
            commands::terminal::builtin_tool_ghostty_launch,
            commands::terminal::ghostty_vt_available,
            // Terminal recording
            commands::terminal::terminal_record_start,
            commands::terminal::terminal_record_stop,
            commands::terminal::terminal_record_list,
            commands::terminal::terminal_record_play,
            commands::terminal::terminal_record_export,
            commands::terminal::terminal_record_prune,
            // Module
            commands::module::module_scan,
            commands::module::module_install,
            commands::module::module_read_manifest,
            commands::module::module_grant_permission,
            commands::module::module_revoke_permission,
            commands::module::module_list_permissions,
            commands::module::module_get_audit_log,
            commands::module::module_approve_all_permissions,
            commands::module::module_uninstall,
            commands::module::module_list,
            commands::module::module_enable,
            commands::module::module_disable,
            commands::module::module_update,
            commands::module::write_generated_module,
            commands::creative_draft::create_creative_draft,
            commands::creative_draft::list_creative_drafts,
            commands::creative_draft::get_creative_draft,
            commands::creative_draft::read_creative_draft,
            commands::creative_draft::bind_creative_draft_conversation,
            commands::creative_draft::rollback_creative_draft,
            commands::creative_draft::publish_creative_draft,
            commands::creative_draft::delete_creative_draft,
            // Creative App (multi-source: workshop + GitHub container + local project)
            commands::creative_app::creative_app_list,
            commands::creative_app::creative_app_start,
            commands::creative_app::creative_app_stop,
            commands::creative_app::creative_app_delete,
            commands::creative_app::creative_app_get_open_target,
            commands::creative_app::creative_app_inspect_github,
            commands::creative_app::creative_app_install_github,
            commands::creative_app::creative_app_logs,
            commands::creative_app::creative_app_reconcile,
            commands::creative_app::creative_app_github_token_status,
            commands::creative_app::creative_app_github_token_set,
            commands::creative_app::creative_app_github_token_clear,
            commands::creative_app::creative_app_docker_status,
            commands::creative_app::creative_app_browser_show,
            commands::creative_app::creative_app_browser_set_bounds,
            commands::creative_app::creative_app_browser_back,
            commands::creative_app::creative_app_browser_forward,
            commands::creative_app::creative_app_browser_reload,
            commands::creative_app::creative_app_browser_hide,
            commands::creative_app::creative_app_browser_close,
            commands::creative_app::creative_app_browser_current,
            // CR-501: Surface / Endpoint / Window
            commands::creative_app::creative_app_surface_list,
            commands::creative_app::creative_app_window_list,
            commands::creative_app::creative_app_window_open,
            commands::creative_app::creative_app_window_close,
            commands::creative_app::creative_app_window_minimize,
            commands::creative_app::creative_app_window_restore,
            // CR-1001/1002: Agent proposal gate
            commands::creative_app::creative_app_proposal_validate,
            commands::creative_app::creative_app_proposal_reject,
            commands::creative_app::creative_app_proposal_approve,
            commands::creative_app::creative_app_proposal_list,
            commands::creative_app::creative_app_inspect_local,
            commands::creative_app::creative_app_create_local,
            commands::creative_app::creative_app_update_local,
            commands::creative_app::creative_app_rescan_local,
            commands::creative_app::creative_app_restart,
            commands::creative_app::creative_app_resolve_orphan,
            commands::creative_app::creative_app_get_local_logs,
            commands::creative_app::creative_app_install_local_dependencies,
            commands::creative_app::creative_app_preview_local_dependency_install,
            commands::creative_app::creative_app_get_local_ai_settings,
            commands::creative_app::creative_app_save_local_ai_settings,
            commands::creative_app::creative_app_preview_local_ai,
            commands::creative_app::creative_app_analyze_local_with_ai,
            commands::creative_app::creative_app_diagnose_local_with_ai,
            commands::creative_app::creative_app_get_local_config,
            commands::creative_app::creative_app_poll_local_exits,
            // Environment
            commands::env::env_get_variables,
            commands::env::env_get_default_profile,
            commands::env::env_list_profiles,
            commands::env::env_create_profile,
            commands::env::env_delete_profile,
            commands::env::env_set_default_profile,
            commands::env::env_set_variable,
            commands::env::env_delete_variable,
            commands::env::env_encrypt,
            // Notifications
            commands::notification::notification_send,
            commands::notification::notification_list,
            commands::notification::notification_mark_read,
            commands::notification::notification_mark_all_read,
            // File System
            commands::fs::fs_list_dir,
            commands::fs::fs_list_dir_detailed,
            commands::fs::fs_read_file,
            commands::fs::fs_write_file_atomic,
            commands::fs::fs_create_entry,
            commands::fs::fs_rename_entry,
            commands::fs::fs_trash_entry,
            commands::fs::fs_trash_entries,
            commands::fs::fs_move_entry,
            commands::fs::fs_move_entries,
            commands::fs::fs_copy_entry,
            commands::fs::fs_copy_entries,
            commands::fs::fs_duplicate_entry,
            commands::fs::fs_stat,
            commands::fs::fs_import_files,
            commands::fs::fs_recent_files,
            commands::fs::fs_save_blob,
            commands::fs::fs_roots,
            commands::fs::fs_open_with,
            commands::fs::fs_clipboard_copy_files,
            commands::fs::fs_clipboard_copy_image,
            // Archive
            commands::archive::archive_list,
            commands::archive::fs_extract_archive,
            commands::archive::fs_compress_entries,
            // Search
            commands::search::search_grep,
            commands::search::search_files,
            commands::search::search_spotlight,
            // Locate（终端路径定位链）
            commands::locate::fs_verify_paths,
            commands::locate::fs_locate,
            // State
            commands::state::state_save,
            commands::state::state_load,
            commands::state::state_clear,
            // Git
            commands::git::git_status,
            commands::git::git_diff,
            commands::git::git_commit,
            commands::git::git_push,
            // Disk
            commands::disk::disk_usage,
            commands::disk::disk_system_info,
            commands::disk::system_metrics,
            // Thumbnail
            commands::thumbnail::thumbnail_generate,
            commands::thumbnail::fs_convert_image_preview,
            // Agent
            commands::agent::agent_scan_projects,
            commands::agent::agent_get_sessions,
            commands::agent::agent_scan_skills,
            commands::agent::agent_detect_status,
            // Skills
            commands::skills::skills_enable,
            commands::skills::skills_disable,
            commands::skills::skills_get_deactivated_path,
            commands::skills::skills_uninstall,
            // Screenshot
            commands::screenshot::screenshot_start_watching,
            commands::screenshot::screenshot_stop_watching,
            commands::screenshot::screenshot_save_annotated,
            // Release
            commands::release::release_inspect,
            commands::release::release_prepare,
            commands::release::release_get_sequence,
            commands::release::release_execute,
            // Update
            commands::update::update_check,
            commands::update::update_mute,
            commands::update::update_dismiss,
            commands::update::update_get_muted,
            commands::update::update_get_dismissed,
            // Usage
            commands::usage::usage_get_cached,
            commands::usage::usage_sync,
            commands::usage::usage_get_ccusage_enabled,
            commands::usage::usage_set_ccusage_enabled,
            commands::usage::usage_detect_ccusage,
            // CodeGraph
            commands::codegraph::read_codegraph,
            commands::codegraph::rtk_gain,
            // Provider
            commands::provider::list_providers,
            commands::provider::add_provider,
            commands::provider::delete_provider,
            commands::provider::add_provider_key,
            commands::provider::delete_provider_key,
            commands::provider::provider_update_defaults,
            commands::provider::provider_set_primary_key,
            commands::provider::provider_discover_models,
            commands::provider::provider_discover_models_saved,
            // Project （统一项目目录 API）
            commands::project::project_list,
            commands::project::project_register,
            commands::project::project_open,
            commands::project::project_rename,
            commands::project::project_remove,
            // Widget
            commands::widget::open_widget_window,
            commands::widget::theme_ready_signal,
            // WeChat ClawBot
            commands::wechat::wechat_env,
            commands::wechat::wechat_login,
            commands::wechat::wechat_poll_login,
            commands::wechat::wechat_disconnect,
            commands::wechat::wechat_check,
            commands::wechat::wechat_send,
            commands::wechat::wechat_set_target,
            commands::wechat::wechat_set_cwd,
            commands::wechat::wechat_set_persona,
            commands::wechat::wechat_detect_agents,
            commands::wechat::wechat_status,
            // Runtime abstraction (Slice B)
            commands::runtime::runtime_list_available,
            commands::runtime::runtime_detect_cli,
            commands::runtime::runtime_set_capability_enabled,
            // Jobs（任务模块，Job Module — 契约第 5 节）
            commands::jobs::job_list,
            commands::jobs::job_get,
            commands::jobs::job_create,
            commands::jobs::job_update,
            commands::jobs::job_delete,
            commands::jobs::job_set_enabled,
            commands::jobs::job_run_now,
            commands::jobs::job_runs_list,
            commands::assistant::assistant_list_sessions,
            commands::assistant::assistant_get_messages,
            commands::assistant::assistant_create_session,
            commands::assistant::assistant_delete_session,
            commands::assistant::assistant_save_message,
            commands::assistant::assistant_update_message_status,
            commands::assistant::assistant_update_session_title,
            commands::assistant::assistant_update_session_model,
            commands::provider::provider_test,
            commands::provider::test_provider_raw,
            // Provider routing / Sub2API account pool (Host-owned configuration)
            provider_accounts::provider_accounts_list,
            provider_accounts::provider_accounts_preview_import,
            provider_accounts::provider_accounts_commit_import,
            provider_accounts::provider_accounts_batch_delete,
            provider_accounts::provider_accounts_create_pool,
            provider_accounts::provider_routing_get_settings,
            provider_accounts::provider_routing_update_settings,
            provider_accounts::provider_routing_rotate_local_token,
            provider_accounts::provider_routing_list_bindings,
            provider_accounts::provider_routing_update_bindings,
            // Library (fanbox clone — G4)
            commands::library::library_list_folders,
            commands::library::library_create_folder,
            commands::library::library_update_folder,
            commands::library::library_delete_folder,
            commands::library::library_list_tags,
            commands::library::library_create_tag,
            commands::library::library_delete_tag,
            commands::library::library_list_items,
            commands::library::library_get_item,
            commands::library::library_create_item,
            commands::library::library_update_item,
            commands::library::library_delete_item,
            commands::library::library_batch_tag,
            commands::library::library_batch_move,
            commands::library::library_batch_delete,
            commands::library::library_get_stats,
            // Subagent (G8)
            commands::subagent::subagent_list,
            commands::subagent::subagent_get,
            commands::subagent::subagent_create,
            commands::subagent::subagent_update,
            commands::subagent::subagent_delete,
            commands::subagent::subagent_run,
            commands::subagent::subagent_list_runs,
            commands::subagent::subagent_resolve_binding,
            // Capability secrets (ADR-0016 决策 7) — Host 侧加密存储
            commands::capability_secret::capability_secret_set,
            commands::capability_secret::capability_secret_delete,
            commands::capability_secret::capability_secret_list,
            // MCP OAuth 浏览器流 (ADR-0016 决策 7) — Host 侧 loopback + PKCE
            commands::mcp_oauth::mcp_oauth_start,
            // Execution Engine settings（PRD 3.4）
            commands::executor_settings::executor_get_settings,
            commands::executor_settings::executor_save_settings,
            // Module rollback（US-6 一键回滚）
            commands::module::rollback_module,
            // Bridge / Security
            commands::bridge::get_http_port,
            commands::bridge::generate_token,
            commands::bridge::validate_token,
            // FsWatch
            commands::watch_preview::fs_watch_start,
            commands::watch_preview::fs_watch_stop,
            commands::watch_preview::fs_watch_stop_all,
            commands::watch_preview::fs_watch_list,
            // HtmlPreview
            commands::watch_preview::html_preview_prepare,
            // LidGuard
            commands::watch_preview::lid_guard_set,
            commands::watch_preview::lid_guard_status,
            // Assistant Service (in-process RPC, no sidecar)
            crate::assistant_service::assistant_rpc_request,
            crate::assistant_service::assistant_status,
            // Credential Broker — daemon requests decrypted keys for a single run
            crate::credential_broker::credential_broker_resolve,
            // Sidecar Supervisor — production UDS lifecycle (no silent embedded fallback)
            crate::sidecar_supervisor::daemon_supervisor_status,
            crate::sidecar_supervisor::daemon_supervisor_ensure,
            crate::sidecar_supervisor::daemon_supervisor_poll,
            crate::sidecar_supervisor::daemon_supervisor_shutdown,
        ])
        .build(tauri::generate_context!())
        .expect("error while building natives")
        .run(|_app_handle, event| {
            // Window CloseRequested is not the only exit path (menu quit, app exit, etc.).
            match event {
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                    let _ = crate::sidecar_supervisor::global_supervisor().shutdown();
                }
                _ => {}
            }
        });
}
