//! The main window: layouts, always-on-top, content protection, tray behaviour (FR-93, FR-94).

use std::sync::atomic::{AtomicU8, Ordering};

use tauri::{AppHandle, LogicalSize, Manager, WebviewWindow};

use crate::events::{self, UiState};
use crate::settings::{Layout, Settings};
use crate::state::AppState;

pub const MAIN: &str = "main";
const COMPACT: (f64, f64) = (380.0, 170.0);
const COMPACT_MIN: (f64, f64) = (340.0, 150.0);
const EXPANDED: (f64, f64) = (420.0, 640.0);
const EXPANDED_MIN: (f64, f64) = (360.0, 420.0);

pub fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN)
}

pub fn show(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    sync_webview_visibility(app);
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let _ = w.hide();
    }
    sync_webview_visibility(app);
}

/// What the WebView was last told: 0 nothing yet, 1 visible, 2 hidden.
static WEBVIEW_SHOWN: AtomicU8 = AtomicU8::new(0);

/// WebView2 keeps a minimized or hidden window's page "visible", so the line goes on drawing
/// unseen. Telling it otherwise stops animation frames and throttles timers; events still
/// arrive, so the transcript keeps up. Called on every resize (minimizing is one) and hide/show.
pub fn sync_webview_visibility(app: &AppHandle) {
    let Some(w) = main_window(app) else { return };
    let visible = w.is_visible().unwrap_or(true) && !w.is_minimized().unwrap_or(false);
    let want = if visible { 1 } else { 2 };
    if WEBVIEW_SHOWN.swap(want, Ordering::Relaxed) == want {
        return;
    }
    #[cfg(windows)]
    {
        let result = w.with_webview(move |webview| {
            // SAFETY: `with_webview` runs this on the WebView's UI thread.
            if let Err(e) = unsafe { webview.controller().SetIsVisible(visible) } {
                tracing::warn!("WebView visibility: {e}");
            }
        });
        if let Err(e) = result {
            tracing::warn!("WebView visibility: {e}");
        }
    }
}

/// Whether the WebView was last told it is hidden (the window minimized or in the tray).
pub fn webview_hidden() -> bool {
    WEBVIEW_SHOWN.load(Ordering::Relaxed) == 2
}

pub fn toggle_visible(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
        if visible {
            hide(app);
        } else {
            show(app);
        }
    }
}

/// Always-on-top and screen-capture exclusion (WDA_EXCLUDEFROMCAPTURE through Tauri).
pub fn apply_settings(app: &AppHandle, s: &Settings) {
    if let Some(w) = main_window(app) {
        let _ = w.set_always_on_top(s.window.always_on_top);
        let _ = w.set_content_protected(s.screenshot.exclude_self);
    }
}

/// Resizes for a layout. `Expanded` is also used temporarily for Settings and the wizard.
pub fn apply_layout(app: &AppHandle, layout: Layout) {
    let Some(w) = main_window(app) else { return };
    let (size, min, resizable) = match layout {
        Layout::Compact => (COMPACT, COMPACT_MIN, false),
        Layout::Expanded => (EXPANDED, EXPANDED_MIN, true),
    };
    let _ = w.set_resizable(true);
    let _ = w.set_min_size(Some(LogicalSize::new(min.0, min.1)));
    let _ = w.set_size(LogicalSize::new(size.0, size.1));
    let _ = w.set_resizable(resizable);
}

fn busy(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|st| {
            matches!(st.recorder.lock().ui_state(), Some(UiState::Recording | UiState::Paused | UiState::Finishing))
        })
        .unwrap_or(false)
}

/// Closing while recording hides to the tray; otherwise the app quits.
pub fn on_close_requested(app: &AppHandle, window: &tauri::Window, api: &tauri::CloseRequestApi) {
    if window.label() != MAIN {
        return;
    }
    api.prevent_close();
    if busy(app) {
        hide(app);
        if let Some(st) = app.try_state::<AppState>() {
            crate::commands::os_notification(app, &st, "hiddenToTray", "");
        }
    } else {
        app.exit(0);
    }
}

/// Tray 終了: quitting while recording asks first (the UI shows the confirmation).
pub fn request_quit(app: &AppHandle) {
    if busy(app) {
        show(app);
        events::emit(app, events::UI_COMMAND, &serde_json::json!({ "command": "confirm_quit" }));
    } else {
        app.exit(0);
    }
}
