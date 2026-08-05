//! Window registry (batch 5 CR-502).
//!
//! Maps Tauri WebView lifecycle to persistent WindowInstance records.
//! Each child WebView gets a WindowInstance row; close≠stop — closing a
//! window does not affect the runtime, and stopping a runtime does not
//! automatically close the window.

use super::browser;
use super::model::BrowserBounds;
use super::model::WindowInstance;
use super::surface_store;
use crate::{Error, Result};
use rusqlite::Connection;
use tauri::AppHandle;

/// Open a window for the given app: create a WindowInstance and show the
/// child WebView. Returns the window id.
pub fn open_window(
    app: &AppHandle,
    conn: &Connection,
    browser_state: &browser::BrowserStateHandle,
    application_id: &str,
    app_id: &str,
    url: &str,
    bounds: BrowserBounds,
) -> Result<String> {
    // Ensure surface exists
    let surface_id = surface_store::find_main_surface(conn, application_id)?
        .ok_or_else(|| {
            Error::Internal(format!("no main surface for app {application_id}"))
        })?;

    let label = browser::child_label(app_id);

    // Show the WebView
    browser::browser_show(app, browser_state, app_id, url, bounds)?;

    // Create or update the WindowInstance
    let window_id = if let Some(existing) = surface_store::find_window_by_label(conn, &label)? {
        surface_store::update_window_state(conn, &existing.id, WindowInstance::STATE_OPEN)?;
        existing.id
    } else {
        surface_store::create_window(conn, application_id, &surface_id, None, &label)?
    };

    Ok(window_id)
}

/// Close a window: close the WebView and persist the closed state.
/// Returns the app_id of the closed window, if any.
pub fn close_window(
    app: &AppHandle,
    conn: &Connection,
    browser_state: &browser::BrowserStateHandle,
    app_id: &str,
) -> Result<()> {
    let label = browser::child_label(app_id);

    // Close the WebView
    browser::browser_close(app, browser_state, app_id)?;

    // Update the WindowInstance
    if let Some(window) = surface_store::find_window_by_label(conn, &label)? {
        surface_store::update_window_state(conn, &window.id, WindowInstance::STATE_CLOSED)?;
    }

    Ok(())
}

/// Minimize a window (hide the WebView).
pub fn minimize_window(
    app: &AppHandle,
    conn: &Connection,
    app_id: &str,
) -> Result<()> {
    let label = browser::child_label(app_id);

    // Hide the WebView
    browser::browser_hide(app, app_id)?;

    // Update the WindowInstance
    if let Some(window) = surface_store::find_window_by_label(conn, &label)? {
        surface_store::update_window_state(conn, &window.id, WindowInstance::STATE_MINIMIZED)?;
    }

    Ok(())
}

/// Restore a window (show the WebView).
pub fn restore_window(
    _app: &AppHandle,
    _conn: &Connection,
    _app_id: &str,
) -> Result<()> {
    // Restore is handled by browser_show with an existing WebView:
    // the WebView is navigated to the URL and shown.
    // The state is updated by the caller.
    Ok(())
}

/// Close the WebView for a deleted app without updating the DB (the app
/// row is already gone, so the WindowInstance cascade will handle it).
pub fn cleanup_deleted_app_window(
    app: &AppHandle,
    browser_state: &browser::BrowserStateHandle,
    app_id: &str,
) -> Result<()> {
    // Close the WebView if it exists
    browser::browser_close(app, browser_state, app_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn child_label_generates_correct_window_label() {
        let label = browser::child_label("my-app");
        assert_eq!(label, "creative-app-my-app");
        assert!(!label.contains("main"));
    }

    #[test]
    fn window_state_constants() {
        assert_eq!(WindowInstance::STATE_OPEN, "open");
        assert_eq!(WindowInstance::STATE_CLOSED, "closed");
        assert_eq!(WindowInstance::STATE_MINIMIZED, "minimized");
        assert_eq!(WindowInstance::STATE_BACKGROUND, "background");
    }

    #[test]
    fn close_and_stop_are_distinct() {
        // Verify that the WindowInstance state constants are distinct from
        // RuntimeInstanceStatus values, so close≠stop is enforced at the type level.
        let close_state = WindowInstance::STATE_CLOSED;
        let stop_state = crate::creative_app::model::RuntimeInstanceStatus::Stopped.as_str();
        assert_ne!(
            close_state, stop_state,
            "window closed state must differ from runtime stopped state"
        );
    }
}