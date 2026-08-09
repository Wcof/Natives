//! macOS menubar persistence: tray toggle, lazily-created transparent popup,
//! hide-only close/blur and idempotent quit.
//!
//! Contract (docs/architecture/macos-menubar-personal-overview.md §3):
//! - Window label `menubar`, renderer route `?surface=menubar`.
//! - Popup is lazily created on the first tray click: transparent, borderless,
//!   non-resizable, hidden until `theme_ready_signal`.
//! - Popup blur / Escape / CloseRequested only hide. True quit is Cmd+Q (handled
//!   in `lib.rs` via `RunEvent::ExitRequested`/`Exit`) or the popup Quit item
//!   (`menubar_quit`), both funnel into the same Once-guarded teardown.
//! - Every command validates the invoking window label: only `main`/`menubar`
//!   may call these commands. The popup does not inherit main's shell/fs/dialog/
//!   credential/Workshop permissions (see `capabilities/menubar.json`).

use crate::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

/// Window label for the menubar popup (frozen contract).
pub const MENUBAR_LABEL: &str = "menubar";
/// Renderer route for the popup (frozen contract).
pub const MENUBAR_URL: &str = "index.html?surface=menubar";
/// Tray icon id — must match the id used in `lib.rs` setup.
pub const MENUBAR_TRAY_ID: &str = "natives-menubar";

/// Popup geometry in logical points (mirrors the retired widget skeleton).
const POPUP_WIDTH: f64 = 400.0;
const POPUP_HEIGHT: f64 = 600.0;
/// Gap between the tray icon edge and the popup edge (logical points).
const POPUP_GAP: f64 = 8.0;
/// Grace window for blur-hide: macOS emits the popup blur before the tray click
/// that caused it, so an immediate hide would be undone by the toggle.
const BLUR_HIDE_DELAY_MS: u64 = 150;

/// Set once the popup renderer has applied its theme CSS variables and signalled
/// readiness. The popup is created `visible(false)` and must not appear before
/// this (FOUC guard). Reset on every lazy (re)creation.
static MENUBAR_THEME_READY: AtomicBool = AtomicBool::new(false);

/// When set, a pending blur-hide task must not hide the popup (a toggle
/// show/hide has already handled the current state transition).
static BLUR_HIDE_CANCEL: AtomicBool = AtomicBool::new(false);

/// Mark the popup as theme-ready (called from `widget::theme_ready_signal`).
pub fn mark_theme_ready() {
    MENUBAR_THEME_READY.store(true, Ordering::Relaxed);
}

fn reset_theme_ready() {
    MENUBAR_THEME_READY.store(false, Ordering::Relaxed);
}

fn theme_ready() -> bool {
    MENUBAR_THEME_READY.load(Ordering::Relaxed)
}

fn cancel_pending_blur_hide() {
    BLUR_HIDE_CANCEL.store(true, Ordering::Relaxed);
}

/// Hide the popup shortly after it loses focus, unless a toggle cancelled it.
/// Public so `lib.rs` `.on_window_event(Focused(false))` can route here.
pub fn schedule_blur_hide(app: &tauri::AppHandle) {
    BLUR_HIDE_CANCEL.store(false, Ordering::Relaxed);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(BLUR_HIDE_DELAY_MS)).await;
        if !BLUR_HIDE_CANCEL.load(Ordering::Relaxed) {
            if let Some(popup) = app.get_webview_window(MENUBAR_LABEL) {
                let _ = popup.hide();
            }
        }
    });
}

/// Every menubar command must be invoked from the `main` or `menubar` window
/// (contract §3). Other surfaces are rejected so the popup never becomes a
/// reach-through for unrelated webviews.
fn ensure_menubar_caller(window: &tauri::Window) -> Result<()> {
    match window.label() {
        "main" | MENUBAR_LABEL => Ok(()),
        other => Err(Error::InvalidInput(format!(
            "menubar command called from unsupported window '{other}' (only main/menubar allowed)"
        ))),
    }
}

/// Shared tray-toggle entry point (used by the tray click callback and the
/// `menubar_toggle` command). Creates the popup lazily on first call; afterwards
/// toggles hide/show.
pub fn toggle_menubar(app: &tauri::AppHandle) -> Result<()> {
    if let Some(popup) = app.get_webview_window(MENUBAR_LABEL) {
        if popup.is_visible().unwrap_or(false) {
            cancel_pending_blur_hide();
            popup.hide().map_err(|e| Error::Internal(e.to_string()))?;
        } else if theme_ready() {
            cancel_pending_blur_hide();
            position_popup(app, &popup)?;
            popup.show().map_err(|e| Error::Internal(e.to_string()))?;
            popup
                .set_focus()
                .map_err(|e| Error::Internal(e.to_string()))?;
        } else {
            // Theme not ready yet (first load in flight): keep it invisible;
            // `theme_ready_signal` will show it once CSS variables are applied.
            position_popup(app, &popup)?;
        }
        return Ok(());
    }

    // Lazy creation on first tray click. Stays hidden until theme_ready_signal.
    reset_theme_ready();
    let popup = WebviewWindowBuilder::new(app, MENUBAR_LABEL, WebviewUrl::App(MENUBAR_URL.into()))
        .title("Natives")
        .inner_size(POPUP_WIDTH, POPUP_HEIGHT)
        .transparent(true)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .visible(false)
        .build()
        .map_err(|e| Error::Internal(e.to_string()))?;
    position_popup(app, &popup)?;
    Ok(())
}

/// Clamp an axis to `[min, max]` without panicking when `max < min` (a popup
/// can be wider/taller than a very small work area).
fn clamp_axis(v: f64, min: f64, max: f64) -> f64 {
    if max < min {
        min
    } else {
        v.max(min).min(max)
    }
}

/// Position the popup under the tray icon, falling back to the primary monitor
/// top-right, and clamp it inside the containing monitor's work area. Handles
/// multi-monitor, negative coordinates, the notch and auto-hidden menu bars.
fn position_popup(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) -> Result<()> {
    let scale = popup
        .scale_factor()
        .map_err(|e| Error::Internal(e.to_string()))?;

    // 1. Tray icon rect (macOS supported) → physical anchor.
    let tray_rect = app
        .tray_by_id(MENUBAR_TRAY_ID)
        .and_then(|tray| tray.rect().ok().flatten());
    let anchor = tray_rect.map(|rect| {
        let pos = rect.position.to_physical(scale);
        let size = rect.size.to_physical(scale);
        (pos.x, pos.y, size.width, size.height)
    });

    let primary = popup
        .primary_monitor()
        .map_err(|e| Error::Internal(e.to_string()))?;
    let monitors = popup
        .available_monitors()
        .map_err(|e| Error::Internal(e.to_string()))?;

    let popup_w = POPUP_WIDTH * scale;
    let popup_h = POPUP_HEIGHT * scale;
    let gap = POPUP_GAP * scale;

    // Default anchor when the tray rect is missing or zero-sized.
    let default_anchor = || -> (f64, f64, f64, f64) {
        match &primary {
            Some(m) => {
                let wa = m.work_area();
                (
                    wa.position.x as f64 + wa.size.width as f64 - popup_w,
                    wa.position.y as f64,
                    0.0,
                    0.0,
                )
            }
            None => (0.0, 0.0, 0.0, 0.0),
        }
    };

    let (ax, ay, aw, ah) = anchor.unwrap_or_else(default_anchor);
    // Zero-size rect (menu bar hidden / autohide) → fall back to the primary
    // monitor top-right.
    let (ax, ay, aw, ah) = if aw <= 0.0 || ah <= 0.0 {
        default_anchor()
    } else {
        (ax, ay, aw, ah)
    };

    // 2. Choose the monitor containing the anchor; prefer primary as fallback.
    let target_monitor = monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            ax >= p.x as f64
                && ax <= p.x as f64 + s.width as f64
                && ay >= p.y as f64
                && ay <= p.y as f64 + s.height as f64
        })
        .or(primary.as_ref())
        .ok_or_else(|| Error::NotFound("monitor".into()))?;

    let wa = target_monitor.work_area();
    let wa_x = wa.position.x as f64;
    let wa_y = wa.position.y as f64;
    let wa_w = wa.size.width as f64;
    let wa_h = wa.size.height as f64;

    // 3. Candidate: centered below the tray icon; flip above when no room below.
    let mut x = ax + aw / 2.0 - popup_w / 2.0;
    let mut y = ay + ah + gap;
    if y + popup_h > wa_y + wa_h {
        y = ay - gap - popup_h;
    }
    x = clamp_axis(x, wa_x, wa_x + wa_w - popup_w);
    y = clamp_axis(y, wa_y, wa_y + wa_h - popup_h);

    popup
        .set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(|e| Error::Internal(e.to_string()))
}

/// Show, unminimize and focus the main window. Shared by single-instance, Dock
/// Reopen, `menubar_open_main` and `menubar_open_personal_overview`.
pub fn open_main_window(app: &tauri::AppHandle) -> Result<()> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))?;
    if main.is_minimized().unwrap_or(false) {
        main.unminimize()
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    main.show().map_err(|e| Error::Internal(e.to_string()))?;
    main.set_focus()
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Toggle the menubar popup (lazily create on first call, then show/hide).
#[tauri::command]
pub fn menubar_toggle(window: tauri::Window) -> Result<()> {
    ensure_menubar_caller(&window)?;
    toggle_menubar(window.app_handle())
}

/// Hide the menubar popup (Escape / explicit close from the renderer).
#[tauri::command]
pub fn menubar_hide(window: tauri::Window) -> Result<()> {
    ensure_menubar_caller(&window)?;
    if let Some(popup) = window.app_handle().get_webview_window(MENUBAR_LABEL) {
        cancel_pending_blur_hide();
        popup.hide().map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}

/// True quit: idempotent process cleanup (terminals, local creative processes,
/// Agent Daemon, supervised children) followed by `exit(0)`. Cmd+Q reaches the
/// same cleanup via `RunEvent::ExitRequested`/`Exit`.
#[tauri::command]
pub fn menubar_quit(window: tauri::Window) -> Result<()> {
    ensure_menubar_caller(&window)?;
    let app = window.app_handle().clone();
    crate::shutdown_all_processes(&app);
    app.exit(0);
    Ok(())
}

/// Open Natives: hide the popup, then show/unminimize/focus the main window.
#[tauri::command]
pub fn menubar_open_main(window: tauri::Window) -> Result<()> {
    ensure_menubar_caller(&window)?;
    if let Some(popup) = window.app_handle().get_webview_window(MENUBAR_LABEL) {
        cancel_pending_blur_hide();
        popup.hide().map_err(|e| Error::Internal(e.to_string()))?;
    }
    open_main_window(window.app_handle())
}

/// Open Natives at Settings → Personal Overview: restore the main window and
/// navigate there via the existing `navigate` CustomEvent convention
/// (`ShellLayout`/`useModuleEvents` maps `__settings__` → `settings:personal`).
#[tauri::command]
pub fn menubar_open_personal_overview(window: tauri::Window) -> Result<()> {
    ensure_menubar_caller(&window)?;
    if let Some(popup) = window.app_handle().get_webview_window(MENUBAR_LABEL) {
        cancel_pending_blur_hide();
        popup.hide().map_err(|e| Error::Internal(e.to_string()))?;
    }
    open_main_window(window.app_handle())?;
    if let Some(main) = window.app_handle().get_webview_window("main") {
        main.eval("window.dispatchEvent(new CustomEvent('navigate', { detail: '__settings__' }));")
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}
