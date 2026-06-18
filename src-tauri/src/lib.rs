use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;

mod commands;
mod db;
mod error;

pub use error::{Error, Result};

/// Application state shared across commands
pub struct AppState {
    pub db: Mutex<Option<rusqlite::Connection>>,
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
        .setup(|app| {
            // Initialize app state
            app.manage(AppState {
                db: Mutex::new(None),
            });
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
            // Clipboard
            commands::clipboard::clipboard_write,
            commands::clipboard::clipboard_read,
        ])
        .run(tauri::generate_context!())
        .expect("error while running natives");
}
