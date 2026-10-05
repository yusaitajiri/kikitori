//! Global hotkeys (FR-01, FR-30, FR-92). Off until set; a failure is reported in Settings only.

use std::collections::BTreeMap;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::state::AppState;

/// (Re)registers the hotkeys that are set (an empty one is off).
pub fn register_all(app: &AppHandle) {
    let st = app.state::<AppState>();
    let hotkeys = st.settings.read().hotkeys.clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errors = BTreeMap::new();

    if hotkeys.toggle.is_empty() {
        // Off.
    } else if let Err(e) = gs.on_shortcut(hotkeys.toggle.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let app = app.clone();
            std::thread::spawn(move || {
                let st = app.state::<AppState>();
                #[cfg(windows)]
                crate::recorder::toggle(&app, &st);
            });
        }
    }) {
        tracing::warn!("toggle hotkey {} failed: {e}", hotkeys.toggle);
        errors.insert("toggle".to_string(), e.to_string());
    }

    if hotkeys.screenshot.is_empty() {
        // Off.
    } else if hotkeys.screenshot.eq_ignore_ascii_case(&hotkeys.toggle) {
        errors.insert("screenshot".to_string(), "same as Start/Stop".to_string());
    } else if let Err(e) = gs.on_shortcut(hotkeys.screenshot.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let app = app.clone();
            std::thread::spawn(move || {
                let st = app.state::<AppState>();
                let _ = crate::recorder::take_screenshot(&app, &st);
            });
        }
    }) {
        tracing::warn!("screenshot hotkey {} failed: {e}", hotkeys.screenshot);
        errors.insert("screenshot".to_string(), e.to_string());
    }

    *st.hotkey_errors.lock() = errors;
}

/// Checks that an accelerator string parses, without registering it.
pub fn validate(accelerator: &str) -> Result<(), String> {
    use std::str::FromStr;
    tauri_plugin_global_shortcut::Shortcut::from_str(accelerator).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_accelerators_parse() {
        assert!(super::validate("Ctrl+Alt+R").is_ok());
        assert!(super::validate("Ctrl+Alt+S").is_ok());
        assert!(super::validate("Ctrl+Shift+F9").is_ok());
        assert!(super::validate("NotAKey+Q").is_err());
    }
}
