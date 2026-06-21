use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

mod agent;
mod archive;
mod commands;
mod db;
mod disk_usage;
mod env_manager;
mod error;
mod file_manager;
mod fs_watch;
#[cfg(feature = "ghostty-vt")]
mod ghostty_vt;
mod ghostty_config;
mod git;
mod html_preview;
mod http_server;
mod lid_guard;
mod module_manager;
mod permission_center;
mod release_wizard;
mod screenshot;
mod search;
mod terminal;
mod terminal_recorder;
mod thumbnail;
mod token_manager;
pub mod update_checker;
mod wechat;

pub use error::{Error, Result};

// Feature-gated re-exports
#[cfg(feature = "ghostty-vt")]
pub use ghostty_vt::GhosttyTerminal;

/// Emit a `db-state-changed` event so the frontend can react to state changes
/// without polling.  All write commands (theme, locale, module, notification, env)
/// must call this helper to satisfy the "unidirectional bus" constraint.
#[allow(dead_code)]
pub fn emit_db_state_changed(
    app_handle: &tauri::AppHandle,
    channel: &str,
    data: serde_json::Value,
) {
    #[derive(Clone, serde::Serialize)]
    struct Payload {
        channel: String,
        data: serde_json::Value,
    }
    let _ = app_handle.emit(
        "db-state-changed",
        Payload {
            channel: channel.to_string(),
            data,
        },
    );
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
    pub usage_cache: Mutex<Option<(serde_json::Value, u64)>>,
    pub fs_watcher: fs_watch::FsWatcher,
    pub lid_guard: lid_guard::LidGuard,
    pub wechat_bridge: Mutex<Option<wechat::bridge::Bridge>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonValue {
    pub value: serde_json::Value,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
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
            let pool = db::init_db_pool(&db_path)
                .map_err(|e| format!("failed to init database pool: {e}"))?;

            // ── Window Vibrancy (macOS Liquid Glass) ──
            // CSS backdrop-filter blur() is configured in globals.css for cross-platform glass effect.
            // Native NSVisualEffectView vibrancy can be added here when a Tauri v2 plugin becomes
            // available (e.g. tauri-plugin-vibrancy). The window is already "transparent": true in
            // tauri.conf.json, so applying a native vibrancy view will work out of the box.
            // On Tauri v2, use: window.set_background_color(Color::TRANSPARENT) and then apply
            // NSVisualEffectView via the raw window handle (app.get_webview_window("main").unwrap()).

            // Initialize token manager (needs a connection from the pool)
            let init_conn = pool.get()
                .map_err(|e| format!("failed to get DB connection: {e}"))?;
            let tm = std::sync::Arc::new(token_manager::TokenManager::new(&init_conn));
            drop(init_conn); // return connection to pool

            // Initialize modules directory
            let modules_dir = data_dir.join("modules");
            std::fs::create_dir_all(&modules_dir)
                .map_err(|e| format!("failed to create modules dir: {e}"))?;

            // Start local HTTP server for module assets and bridge API
            let mut server = http_server::HttpServer::new(modules_dir, tm.clone(), db_path.clone());
            let port = server.start(0).unwrap_or_else(|e| {
                eprintln!("failed to start HTTP server: {e}");
                0
            });

            // Ensure builtin tools have DB rows (INSERT OR IGNORE — idempotent)
            {
                let seed_conn = pool.get()
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
                screenshot_stop_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                usage_cache: Mutex::new(None),
                fs_watcher: fs_watch::FsWatcher::new(app.handle().clone()),
                lid_guard: lid_guard::LidGuard::new(),
                wechat_bridge: Mutex::new(Some(wechat::bridge::Bridge::new())),
            });

            // FOUC guard: window starts hidden (tauri.conf.json has visible: false)
            // It will be shown by theme_ready_signal command from frontend
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if let Some(state) = window.try_state::<AppState>() {
                    state.ghostty_manager.kill_all();
                    state.terminal_manager.kill_all();
                }
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
            commands::window::window_close,
            commands::window::window_is_maximized,
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
            commands::env::env_decrypt,
            // Notifications
            commands::notification::notification_send,
            commands::notification::notification_list,
            commands::notification::notification_mark_read,
            commands::notification::notification_mark_all_read,
            // File System
            commands::fs::fs_list_dir,
            commands::fs::fs_read_file,
            commands::fs::fs_write_file_atomic,
            commands::fs::fs_create_entry,
            commands::fs::fs_rename_entry,
            commands::fs::fs_trash_entry,
            commands::fs::fs_move_entry,
            commands::fs::fs_import_files,
            commands::fs::fs_recent_files,
            commands::fs::fs_save_blob,
            // Archive
            commands::archive::archive_list,
            // Search
            commands::search::search_grep,
            commands::search::search_files,
            commands::search::search_spotlight,
            // State
            commands::state::state_save,
            commands::state::state_load,
            commands::state::state_clear,
            // Git
            commands::git::git_status,
            commands::git::git_diff,
            // Disk
            commands::disk::disk_usage,
            commands::disk::disk_system_info,
            // Thumbnail
            commands::thumbnail::thumbnail_generate,
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
            commands::usage::usage_refresh,
            // CodeGraph
            commands::codegraph::read_codegraph,
            commands::codegraph::rtk_gain,
            // Provider
            commands::provider::list_providers,
            commands::provider::add_provider,
            commands::provider::delete_provider,
            commands::provider::add_provider_key,
            commands::provider::delete_provider_key,
            // Widget
            commands::widget::open_widget_window,
            commands::widget::theme_ready_signal,
            // WeChat ClawBot
            commands::wechat::wechat_env,
            commands::wechat::wechat_login,
            commands::wechat::wechat_disconnect,
            commands::wechat::wechat_check,
            commands::wechat::wechat_send,
            commands::wechat::wechat_set_target,
            commands::wechat::wechat_set_cwd,
            commands::wechat::wechat_set_persona,
            commands::wechat::wechat_detect_agents,
            commands::wechat::wechat_status,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running natives");
}
