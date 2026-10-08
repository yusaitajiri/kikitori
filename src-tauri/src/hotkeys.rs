//! Global hotkeys (FR-01, FR-06, FR-07, FR-30, FR-92). Off until set; a failure is reported in
//! Settings only.

use std::collections::BTreeMap;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::events::{self, Notice, NoticeLevel};
use crate::state::AppState;

/// What a hotkey does; it runs off the shortcut thread.
type Action = fn(&AppHandle, &AppState);

fn toggle(app: &AppHandle, st: &AppState) {
    #[cfg(windows)]
    crate::recorder::toggle(app, st);
    #[cfg(not(windows))]
    let _ = (app, st);
}

fn screenshot(app: &AppHandle, st: &AppState) {
    let _ = crate::recorder::take_screenshot(app, st);
}

/// The button shows its own toast; a hotkey, pressed with another app in front, says what it did.
fn mark(app: &AppHandle, st: &AppState) {
    let message = match crate::recorder::mark_current(st) {
        Ok(true) => "markAdded",
        Ok(false) => "nothingToMark",
        Err(_) => "shotOnlyWhileRecording",
    };
    events::notice(app, Notice::new(NoticeLevel::Info, "mark", message).toast());
}

fn cut(app: &AppHandle, st: &AppState) {
    let message = if crate::recorder::add_cut(st).is_ok() { "cutAdded" } else { "shotOnlyWhileRecording" };
    events::notice(app, Notice::new(NoticeLevel::Info, "cut", message).toast());
}

/// (Re)registers the hotkeys that are set (an empty one is off).
pub fn register_all(app: &AppHandle) {
    let st = app.state::<AppState>();
    let hotkeys = st.settings.read().hotkeys.clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errors = BTreeMap::new();
    let table: [(&str, &str, Action); 4] = [
        ("toggle", &hotkeys.toggle, toggle),
        ("screenshot", &hotkeys.screenshot, screenshot),
        ("mark", &hotkeys.mark, mark),
        ("cut", &hotkeys.cut, cut),
    ];
    let mut taken: Vec<&str> = Vec::new();
    for (name, accelerator, action) in table {
        if accelerator.is_empty() {
            continue; // Off.
        }
        if taken.iter().any(|t| t.eq_ignore_ascii_case(accelerator)) {
            errors.insert(name.to_string(), "same as another shortcut".to_string());
            continue;
        }
        let registered = gs.on_shortcut(accelerator, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                std::thread::spawn(move || action(&app, &app.state::<AppState>()));
            }
        });
        match registered {
            Ok(()) => taken.push(accelerator),
            Err(e) => {
                tracing::warn!("{name} hotkey {accelerator} failed: {e}");
                errors.insert(name.to_string(), e.to_string());
            }
        }
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
