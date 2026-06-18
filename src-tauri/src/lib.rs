use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;

mod agent;
mod archive;
mod commands;
mod db;
mod disk_usage;
mod env_manager;
mod error;
mod file_manager;
mod git;
mod http_server;
mod module_manager;
mod permission_center;
mod search;
mod terminal;
mod thumbnail;
mod token_manager;

pub use error::{Error, Result};

/// Application state shared across commands
pub struct AppState {
    pub db: Mutex<Option<rusqlite::Connection>>,
    pub token_manager: std::sync::Arc<token_manager::TokenManager>,
    pub http_port: Mutex<u16>,
    pub terminal_manager: terminal::TerminalManager,
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
                .map_err(|e| format!("failed to get home dir: {e}"))
                .unwrap()
                .join(".natives");
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("failed to create .natives dir: {e}"))
                .unwrap();
            let db_path = data_dir.join("natives.db");
            let conn = db::init_db(&db_path)
                .map_err(|e| format!("failed to init database: {e}"))
                .unwrap();

            // Initialize token manager
            let tm = std::sync::Arc::new(token_manager::TokenManager::new(&conn));

            // Initialize modules directory
            let modules_dir = data_dir.join("modules");
            std::fs::create_dir_all(&modules_dir)
                .map_err(|e| format!("failed to create modules dir: {e}"))
                .unwrap();

            // Start local HTTP server for module assets and bridge API
            let mut server = http_server::HttpServer::new(modules_dir, tm.clone());
            let port = server.start(0).unwrap_or_else(|e| {
                eprintln!("failed to start HTTP server: {e}");
                0
            });

            app.manage(AppState {
                db: Mutex::new(Some(conn)),
                token_manager: tm,
                http_port: Mutex::new(port),
                terminal_manager: terminal::TerminalManager::new(),
            });

            // FOUC guard: window starts hidden (tauri.conf.json has visible: false)
            // It will be shown by theme_ready_signal command from frontend
            Ok(())
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
            commands::update::update_get_muted,
            // Usage
            commands::usage::usage_refresh,
            // Widget
            commands::widget::open_widget_window,
            commands::widget::theme_ready_signal,
            // Bridge / Security
            commands::bridge::get_http_port,
            commands::bridge::generate_token,
            commands::bridge::validate_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running natives");
}
