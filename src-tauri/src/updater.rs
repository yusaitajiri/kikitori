//! Daily update check through the Tauri updater (FR-95, section 17).
//!
//! The plugin needs a public key in `plugins.updater`, which only exists once the maintainer
//! has generated updater keys (scripts/enable-updater.mjs). Until then the plugin is not
//! registered and every function here is a no-op, so builds without keys still run.

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{self, Notice, NoticeLevel};
use crate::recorder::Phase;
use crate::state::AppState;

const DAY_SECS: i64 = 24 * 60 * 60;
const LAUNCH_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

pub fn configured(app: &AppHandle) -> bool {
    app.config().plugins.0.contains_key("updater")
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    if configured(app) {
        app.plugin(tauri_plugin_updater::Builder::new().build())?;
    }
    Ok(())
}

/// Checks at launch when the setting is on and the last check is a day old. A found update
/// becomes a banner whose button calls `install_update`; nothing installs without a click.
pub fn check_on_launch(app: &AppHandle) {
    let st = app.state::<AppState>();
    if !configured(app) || !st.settings.read().updates.check {
        return;
    }
    if !due(st.internal.lock().last_update_check, chrono::Utc::now().timestamp()) {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let the window load first: it must be listening to show the banner, and launch
        // stays free of network calls.
        tokio::time::sleep(LAUNCH_DELAY).await;
        let st = app.state::<AppState>();
        st.internal.lock().last_update_check = Some(chrono::Utc::now().timestamp());
        st.persist_internal(&app);
        let found = match app.updater() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        match found {
            Ok(Some(update)) => {
                tracing::info!("update {} available", update.version);
                events::notice(
                    &app,
                    Notice::new(NoticeLevel::Info, "update_available", "updateAvailable")
                        .param("version", update.version)
                        .action("updateNow", "install_update"),
                );
            }
            Ok(None) => tracing::info!("no update available"),
            Err(e) => tracing::warn!("update check failed: {e}"),
        }
    });
}

/// A check is due a day after the last one, or when the clock went backwards.
fn due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|t| now - t >= DAY_SECS || now < t)
}

/// Downloads the update, then runs the installer in passive mode. On Windows the installer
/// closes the app and starts it again afterwards (`/R`).
pub async fn install(app: &AppHandle) -> AppResult<()> {
    if !configured(app) {
        return Err(AppError::internal("updater is not configured"));
    }
    if !is_idle(app) {
        return Err(AppError::new(ErrorCode::Internal, "updateWhileRecording"));
    }
    let save_windows = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            use tauri_plugin_window_state::{AppHandleExt, StateFlags};
            let _ = save_windows.save_window_state(StateFlags::POSITION);
        })
        .build()
        .map_err(update_error)?;
    let Some(update) = updater.check().await.map_err(update_error)? else {
        return Ok(());
    };
    tracing::info!("downloading update {}", update.version);
    let bytes = update.download(|_, _| {}, || {}).await.map_err(update_error)?;

    // Hold the recorder lock from the last idle check until the installer takes over, so a
    // recording can't start in between.
    let st = app.state::<AppState>();
    let recorder = st.recorder.lock();
    if !matches!(*recorder, Phase::Idle) {
        return Err(AppError::new(ErrorCode::Internal, "updateWhileRecording"));
    }
    update.install(bytes).map_err(update_error)?;
    drop(recorder);
    // Only reached where installing doesn't exit the process (not Windows).
    app.restart();
}

fn is_idle(app: &AppHandle) -> bool {
    matches!(*app.state::<AppState>().recorder.lock(), Phase::Idle)
}

fn update_error(e: tauri_plugin_updater::Error) -> AppError {
    tracing::warn!("update failed: {e}");
    match e {
        tauri_plugin_updater::Error::Reqwest(_) | tauri_plugin_updater::Error::Network(_) => {
            AppError::new(ErrorCode::Network, e.to_string())
        }
        _ => AppError::internal(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_at_most_once_a_day() {
        let now = 1_000_000;
        assert!(due(None, now));
        assert!(!due(Some(now - 60), now));
        assert!(!due(Some(now - DAY_SECS + 1), now));
        assert!(due(Some(now - DAY_SECS), now));
        // The clock went backwards (for example a fixed CMOS battery): check again.
        assert!(due(Some(now + 3600), now));
    }
}
