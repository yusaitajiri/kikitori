//! Tauri commands (section 14). Argument and field names are camelCase on both sides; every
//! error is `{ code, message }`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::asr::worker::EngineStatus;
use crate::audio::source::{AudioApp, Device};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{self, StatePayload};
use crate::export::markdown;
use crate::export::{archive, html, plaintext, saving, typst_pdf};
use crate::models::{benchmark, manager};
use crate::recorder::{self, Phase, ShotResponse, SourceConfig, StartResponse};
use crate::session::log::{LogEvent, SessionLog};
use crate::session::model::{Segment, Session, TimelineItem};
use crate::session::recovery::{self, SessionSummary};
use crate::settings::{Layout, Locale, Settings};
use crate::state::AppState;

/// Runs blocking work off the async runtime's core threads.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppHandle, &AppState) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        f(&app, &st)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

pub fn write_clipboard(app: &AppHandle, text: &str) -> AppResult<()> {
    app.clipboard().write_text(text.to_string()).map_err(|e| AppError::internal(e.to_string()))
}

fn os_text(locale: Locale, key: &str, param: &str) -> String {
    match (locale, key) {
        (Locale::Ja, "shotAdded") => format!("スクショを挿入しました {param}"),
        (Locale::En, "shotAdded") => format!("Screenshot added {param}"),
        (Locale::Ja, "hiddenToTray") => "トレイで録音を続けています".to_string(),
        (Locale::En, "hiddenToTray") => "Still recording in the tray".to_string(),
        (_, other) => other.to_string(),
    }
}

/// A silent Windows notification (used when the window is hidden).
pub fn os_notification(app: &AppHandle, st: &AppState, key: &str, param: &str) {
    let locale = st.settings.read().locale;
    let result = app.notification().builder().title("Kikitori").body(os_text(locale, key, param)).silent().show();
    if let Err(e) = result {
        tracing::debug!("notification failed: {e}");
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub os_build: u32,
    pub app_loopback_supported: bool,
    /// "gpu" | "cpu" | "loading"
    pub device: &'static str,
    pub device_name: Option<String>,
    pub engine: EngineStatus,
    pub gpu_compiled: bool,
    pub hotkey_errors: std::collections::BTreeMap<String, String>,
    pub state: StatePayload,
    pub models_dir: String,
    pub log_dir: Option<String>,
    pub recommended_model: String,
    pub total_ram_bytes: u64,
    pub setup_done: bool,
    pub gpu_failed: bool,
}

#[tauri::command]
pub async fn get_app_info(app: AppHandle) -> AppResult<AppInfo> {
    blocking(app, |app, st| {
        let engine = st.worker.status();
        let device = match engine.state {
            crate::asr::worker::EngineState::Ready if engine.gpu => "gpu",
            crate::asr::worker::EngineState::Ready => "cpu",
            _ => "loading",
        };
        let internal = st.internal.lock().clone();
        Ok(AppInfo {
            version: app.package_info().version.to_string(),
            os_build: crate::platform::os_build(),
            app_loopback_supported: crate::platform::app_loopback_supported(),
            device,
            device_name: engine.device_name.clone(),
            engine,
            gpu_compiled: crate::asr::engine::gpu_compiled(),
            hotkey_errors: st.hotkey_errors.lock().clone(),
            state: recorder::state_payload(st),
            models_dir: st.models_dir.to_string_lossy().into_owned(),
            log_dir: app.path().app_log_dir().ok().map(|p| p.to_string_lossy().into_owned()),
            recommended_model: crate::models::catalog::recommend(crate::platform::total_memory()).to_string(),
            total_ram_bytes: crate::platform::total_memory(),
            setup_done: internal.setup_done,
            gpu_failed: internal.gpu_failure.is_some(),
        })
    })
    .await
}

#[tauri::command]
pub fn get_settings(st: State<'_, AppState>) -> Settings {
    st.settings.read().clone()
}

#[tauri::command]
pub async fn set_settings(app: AppHandle, partial: Value) -> AppResult<Settings> {
    blocking(app, move |app, st| {
        let old = st.settings.read().clone();
        if let Some(hk) = partial.get("hotkeys") {
            for k in ["toggle", "screenshot"] {
                // An empty shortcut is off.
                if let Some(acc) = hk.get(k).and_then(Value::as_str).filter(|a| !a.trim().is_empty()) {
                    crate::hotkeys::validate(acc).map_err(|e| AppError::new(ErrorCode::HotkeyTaken, e))?;
                }
            }
        }
        let new = crate::settings::merge_partial(&old, &partial, &st.default_output_root);
        *st.settings.write() = new.clone();
        st.persist_settings(app);
        if new.hotkeys != old.hotkeys {
            crate::hotkeys::register_all(app);
        }
        if new.window.always_on_top != old.window.always_on_top
            || new.screenshot.exclude_self != old.screenshot.exclude_self
        {
            crate::window::apply_settings(app, &new);
        }
        if (new.model_id != old.model_id || new.use_gpu != old.use_gpu) && matches!(&*st.recorder.lock(), Phase::Idle) {
            st.load_selected_model();
        }
        if new.output.root != old.output.root {
            allow_asset_dir(app, Path::new(&new.output.root));
        }
        if new.locale != old.locale {
            recorder::emit_state(app, st);
        }
        Ok(new)
    })
    .await
}

pub fn allow_asset_dir(app: &AppHandle, dir: &Path) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        tracing::warn!("asset scope for {}: {e}", dir.display());
    }
}

#[tauri::command]
pub async fn list_audio_apps(app: AppHandle) -> AppResult<Vec<AudioApp>> {
    blocking(app, |_, _| {
        #[cfg(windows)]
        {
            use crate::audio::source::AudioAppLister;
            crate::audio::win::sessions::WinAudioAppLister.list().map_err(AppError::from)
        }
        #[cfg(not(windows))]
        Ok(Vec::new())
    })
    .await
}

#[tauri::command]
pub async fn list_mic_devices(app: AppHandle) -> AppResult<Vec<Device>> {
    blocking(app, |_, _| {
        #[cfg(windows)]
        {
            crate::audio::win::devices::list_mic_devices().map_err(AppError::from)
        }
        #[cfg(not(windows))]
        Ok(Vec::new())
    })
    .await
}

/// The apps whose loudness the source picker shows, by root PID; none stops reading their meters.
#[tauri::command]
pub fn watch_app_levels(app: AppHandle, st: State<'_, AppState>, root_pids: Vec<u32>) {
    #[cfg(windows)]
    st.meters.watch(std::sync::Arc::new(app), root_pids);
    #[cfg(not(windows))]
    let _ = (app, st, root_pids);
}

#[tauri::command]
pub async fn start_recording(app: AppHandle, source: SourceConfig, title: Option<String>) -> AppResult<StartResponse> {
    blocking(app, move |app, st| {
        #[cfg(windows)]
        {
            recorder::start(app, st, source, title)
        }
        #[cfg(not(windows))]
        {
            let _ = (app, st, source, title);
            Err(AppError::internal("unsupported platform"))
        }
    })
    .await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> AppResult<()> {
    blocking(app, recorder::stop).await
}

#[tauri::command]
pub fn cancel_finishing(st: State<'_, AppState>) {
    recorder::cancel_finishing(&st);
}

#[tauri::command]
pub async fn pause_recording(app: AppHandle) -> AppResult<()> {
    blocking(app, recorder::pause).await
}

#[tauri::command]
pub async fn resume_recording(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, st| {
        #[cfg(windows)]
        {
            recorder::resume(app, st)
        }
        #[cfg(not(windows))]
        {
            let _ = (app, st);
            Ok(())
        }
    })
    .await
}

#[tauri::command]
pub async fn switch_to_system(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, st| {
        #[cfg(windows)]
        {
            recorder::switch_to_system(app, st)
        }
        #[cfg(not(windows))]
        {
            let _ = (app, st);
            Ok(())
        }
    })
    .await
}

#[tauri::command]
pub async fn take_screenshot(app: AppHandle) -> AppResult<ShotResponse> {
    blocking(app, recorder::take_screenshot).await
}

fn root(st: &AppState) -> PathBuf {
    PathBuf::from(&st.settings.read().output.root)
}

/// The folder of a session: the live one, or a search under the output root.
fn folder_of(st: &AppState, session_id: &str) -> AppResult<PathBuf> {
    match &*st.recorder.lock() {
        Phase::Recording(a) if a.store.id == session_id => return Ok(a.store.folder.clone()),
        Phase::Finishing { session_id: id, folder, .. } if id == session_id => return Ok(folder.clone()),
        _ => {}
    }
    recovery::find_folder(&root(st), session_id)
        .ok_or_else(|| AppError::internal(format!("session {session_id} not found")))
}

fn live_session(st: &AppState, session_id: &str) -> Option<Session> {
    match &*st.recorder.lock() {
        Phase::Recording(a) if a.store.id == session_id => Some(a.store.snapshot()),
        _ => None,
    }
}

fn load_session(st: &AppState, session_id: &str) -> AppResult<(Session, PathBuf)> {
    let folder = folder_of(st, session_id)?;
    if let Some(s) = live_session(st, session_id) {
        return Ok((s, folder));
    }
    let (session, _, _) = recovery::load(&folder)?;
    Ok((session, folder))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    #[serde(flatten)]
    pub session: Session,
    pub folder: String,
    /// How the session sounded, for its line; missing when the session has no `levels.bin`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<recovery::Sound>,
}

#[tauri::command]
pub async fn get_session(app: AppHandle, session_id: String) -> AppResult<SessionView> {
    blocking(app, move |_, st| {
        let (session, folder) = load_session(st, &session_id)?;
        let sound = recovery::sound(&session, &folder);
        Ok(SessionView { session, folder: folder.to_string_lossy().into_owned(), sound })
    })
    .await
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CopyFormat {
    Plain,
    Agent,
    Markdown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    pub chars: usize,
}

#[tauri::command]
pub async fn copy_transcript(app: AppHandle, session_id: String, format: CopyFormat) -> AppResult<CopyResult> {
    blocking(app, move |app, st| {
        let (session, _) = load_session(st, &session_id)?;
        let settings = st.settings.read().clone();
        let opts = settings.export_options();
        let text = match format {
            CopyFormat::Plain => plaintext::plain(&session, &opts),
            CopyFormat::Agent => plaintext::for_agent(&session, &opts, &settings.vocabulary),
            CopyFormat::Markdown => markdown::body(&session, &opts),
        };
        write_clipboard(app, &text)?;
        Ok(CopyResult { chars: text.chars().count() })
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathResult {
    pub path: String,
}

/// Asks where to save an export: Downloads first, named after the session folder, over the
/// always-on-top window. `None` when cancelled. Exports never go into the session folder.
fn ask_save_path(app: &AppHandle, folder: &Path, extension: &str, kind: &str) -> Option<PathBuf> {
    let stem = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "transcript".into());
    let mut dialog = app.dialog().file().set_file_name(format!("{stem}.{extension}")).add_filter(kind, &[extension]);
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.blocking_save_file().and_then(|p| p.into_path().ok())
}

fn saved(path: &Path) -> Option<PathResult> {
    Some(PathResult { path: path.to_string_lossy().into_owned() })
}

/// Asks for a folder to export into: Downloads first, over the always-on-top window.
fn ask_folder(app: &AppHandle) -> Option<PathBuf> {
    let mut dialog = app.dialog().file();
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.blocking_pick_folder().and_then(|p| p.into_path().ok())
}

/// 書き出し → Markdown. Without `zip`: a new folder in the directory the user picks, named after the
/// session, holding `transcript.md` and `images/`. With `zip`: the same in one archive where the
/// user saves it. `None` when the dialog is cancelled.
#[tauri::command]
pub async fn export_markdown(app: AppHandle, session_id: String, zip: bool) -> AppResult<Option<PathResult>> {
    blocking(app, move |app, st| {
        let (session, folder) = load_session(st, &session_id)?;
        let opts = st.settings.read().export_options();
        let version = app.package_info().version.to_string();
        if zip {
            let Some(out) = ask_save_path(app, &folder, "zip", "ZIP") else { return Ok(None) };
            archive::write(&session, &opts, &folder, &out, &version)?;
            return Ok(saved(&out));
        }
        let Some(parent) = ask_folder(app) else { return Ok(None) };
        let out = saving::markdown_folder(&session, &opts, &folder, &parent, &version)?;
        Ok(saved(&out))
    })
    .await
}

/// 書き出し → PDF: typeset with Typst where the user saves it, then opened. If Typst fails, the
/// print HTML is saved there instead (screenshots linked where they are) and opened in the browser.
#[tauri::command]
pub async fn export_pdf(app: AppHandle, session_id: String) -> AppResult<Option<PathResult>> {
    let (session, folder, opts) = {
        let st = app.state::<AppState>();
        let (session, folder) = load_session(&st, &session_id)?;
        let opts = st.settings.read().export_options();
        (session, folder, opts)
    };
    let ask = {
        let (app, folder) = (app.clone(), folder.clone());
        tauri::async_runtime::spawn_blocking(move || ask_save_path(&app, &folder, "pdf", "PDF"))
    };
    let Some(out) = ask.await.map_err(|e| AppError::internal(e.to_string()))? else { return Ok(None) };
    let version = app.package_info().version.to_string();
    let rendered = {
        let (session, folder) = (session.clone(), folder.clone());
        tauri::async_runtime::spawn_blocking(move || {
            crate::export::typst_pdf::render(&session, &opts, &folder, &version)
        })
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
    };
    match rendered.and_then(|pdf| recovery::write_atomic(&out, &pdf).map_err(anyhow::Error::from)) {
        Ok(()) => {
            let _ = app.opener().open_path(out.to_string_lossy(), None::<&str>);
            return Ok(saved(&out));
        }
        Err(e) => tracing::warn!("PDF export failed, falling back to HTML: {e:#}"),
    }
    // Fallback (section 11): save the HTML, open it in the browser, ask the user to print.
    let html_doc = html::render_linking(&session, &opts, &|shot| saving::file_url(&folder.join(&shot.file)));
    let fallback = out.with_extension("html");
    std::fs::write(&fallback, html_doc.as_bytes())?;
    let _ = app.opener().open_path(fallback.to_string_lossy(), None::<&str>);
    events::notice(&app, events::Notice::new(events::NoticeLevel::Warn, ErrorCode::PdfFailed.as_str(), "pdfFallback"));
    Err(AppError::new(ErrorCode::PdfFailed, "saved the print HTML instead"))
}

/// 書き出し → Typst: the document the PDF is made from, where the user saves it, with its
/// screenshots copied beside it (`<name>_images/`), so `typst compile` there makes the same PDF.
#[tauri::command]
pub async fn export_typst(app: AppHandle, session_id: String) -> AppResult<Option<PathResult>> {
    blocking(app, move |app, st| {
        let (session, folder) = load_session(st, &session_id)?;
        let opts = st.settings.read().export_options();
        let Some(out) = ask_save_path(app, &folder, "typ", "Typst") else { return Ok(None) };
        let links = saving::copy_images(&session, &folder, &out)?;
        let src = typst_pdf::typst_source(&session, &opts, &links, &app.package_info().version.to_string());
        recovery::write_atomic(&out, src.as_bytes())?;
        Ok(saved(&out))
    })
    .await
}

fn allowed_path(app: &AppHandle, st: &AppState, path: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    let Some(target) = canon(path) else { return false };
    let mut roots = vec![root(st), st.models_dir.clone()];
    if let Ok(d) = app.path().app_log_dir() {
        roots.push(d);
    }
    roots.iter().filter_map(|r| canon(r)).any(|r| target.starts_with(r))
}

#[tauri::command]
pub async fn open_path(app: AppHandle, path: String) -> AppResult<()> {
    blocking(app, move |app, st| {
        let p = PathBuf::from(&path);
        if !allowed_path(app, st, &p) {
            return Err(AppError::internal("path is outside Kikitori's folders"));
        }
        app.opener().open_path(path, None::<&str>).map_err(|e| AppError::internal(e.to_string()))
    })
    .await
}

#[tauri::command]
pub fn open_mic_privacy(app: AppHandle) -> AppResult<()> {
    app.opener().open_url("ms-settings:privacy-microphone", None::<&str>).map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub fn open_logs(app: AppHandle) -> AppResult<()> {
    let dir = app.path().app_log_dir().map_err(|e| AppError::internal(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub async fn list_recoverable(app: AppHandle) -> AppResult<Vec<SessionSummary>> {
    blocking(app, |_, st| {
        let active = st.recorder.lock().session_id();
        Ok(recovery::recoverable(&root(st), active.as_deref()))
    })
    .await
}

#[tauri::command]
pub async fn recover_session(app: AppHandle, session_id: String) -> AppResult<PathResult> {
    blocking(app, move |app, st| {
        let folder = folder_of(st, &session_id)?;
        let opts = st.settings.read().export_options();
        let path = recovery::recover(&folder, &opts, &app.package_info().version.to_string())?;
        Ok(PathResult { path: path.to_string_lossy().into_owned() })
    })
    .await
}

#[tauri::command]
pub async fn list_sessions(app: AppHandle) -> AppResult<Vec<SessionSummary>> {
    blocking(app, |_, st| {
        let active = st.recorder.lock().session_id();
        Ok(recovery::list(&root(st)).into_iter().filter(|s| Some(&s.id) != active.as_ref()).collect())
    })
    .await
}

#[tauri::command]
pub async fn delete_session(app: AppHandle, session_id: String) -> AppResult<()> {
    blocking(app, move |_, st| {
        if st.recorder.lock().session_id().as_deref() == Some(session_id.as_str()) {
            return Err(AppError::internal("cannot delete the session being recorded"));
        }
        let folder = folder_of(st, &session_id)?;
        #[cfg(windows)]
        trash::delete(&folder).map_err(|e| AppError::internal(format!("moving to the Recycle Bin failed: {e}")))?;
        #[cfg(not(windows))]
        let _ = folder;
        Ok(())
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct DeletedResult {
    pub deleted: usize,
}

/// Moves several sessions to the Recycle Bin in one go (History's 選択); the one being recorded stays.
#[tauri::command]
pub async fn delete_sessions(app: AppHandle, session_ids: Vec<String>) -> AppResult<DeletedResult> {
    blocking(app, move |_, st| {
        let active = st.recorder.lock().session_id();
        let wanted: std::collections::HashSet<&str> =
            session_ids.iter().map(String::as_str).filter(|id| active.as_deref() != Some(*id)).collect();
        let folders: Vec<PathBuf> = recovery::list(&root(st))
            .into_iter()
            .filter(|s| wanted.contains(s.id.as_str()))
            .map(|s| PathBuf::from(s.folder))
            .collect();
        if folders.is_empty() {
            return Ok(DeletedResult { deleted: 0 });
        }
        #[cfg(windows)]
        trash::delete_all(&folders)
            .map_err(|e| AppError::internal(format!("moving to the Recycle Bin failed: {e}")))?;
        Ok(DeletedResult { deleted: folders.len() })
    })
    .await
}

/// Applies an edit: through the live store while recording, else appended to the log and
/// the outputs regenerated.
fn edit(app: &AppHandle, st: &AppState, session_id: &str, event: LogEvent) -> AppResult<Session> {
    if let Phase::Recording(a) = &*st.recorder.lock()
        && a.store.id == session_id
    {
        a.store.record(event);
        return Ok(a.store.snapshot());
    }
    if matches!(&*st.recorder.lock(), Phase::Finishing { session_id: id, .. } if id == session_id) {
        return Err(AppError::internal("the session is still being saved"));
    }
    let folder = folder_of(st, session_id)?;
    SessionLog::open_append(&folder)?.append(&event)?;
    let (session, _, _) = recovery::load(&folder)?;
    let opts = st.settings.read().export_options();
    recovery::save_outputs(&folder, &session, &opts, &app.package_info().version.to_string())?;
    Ok(session)
}

#[tauri::command]
pub async fn update_segment(
    app: AppHandle,
    session_id: String,
    segment_id: String,
    text: String,
) -> AppResult<Segment> {
    blocking(app, move |app, st| {
        let session = edit(
            app,
            st,
            &session_id,
            LogEvent::SegmentEdited { id: segment_id.clone(), text: text.trim().to_string() },
        )?;
        session
            .items
            .into_iter()
            .find_map(|i| match i {
                TimelineItem::Segment(s) if s.id == segment_id => Some(s),
                _ => None,
            })
            .ok_or_else(|| AppError::internal("segment not found"))
    })
    .await
}

#[tauri::command]
pub async fn delete_segment(app: AppHandle, session_id: String, segment_id: String) -> AppResult<()> {
    blocking(app, move |app, st| {
        edit(app, st, &session_id, LogEvent::SegmentRemoved { id: segment_id, reason: "user".into() })?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn delete_screenshot(app: AppHandle, session_id: String, id: String) -> AppResult<()> {
    blocking(app, move |app, st| {
        edit(app, st, &session_id, LogEvent::ScreenshotDeleted { id })?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_caption(app: AppHandle, session_id: String, id: String, caption: String) -> AppResult<()> {
    blocking(app, move |app, st| {
        edit(app, st, &session_id, LogEvent::CaptionSet { id, caption })?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn rename_session(app: AppHandle, session_id: String, title: String) -> AppResult<()> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::internal("empty title"));
    }
    blocking(app, move |app, st| {
        edit(app, st, &session_id, LogEvent::TitleChanged { title })?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn models_list(st: State<'_, AppState>) -> Vec<manager::ModelEntry> {
    manager::list(&st)
}

#[tauri::command]
pub fn model_download(app: AppHandle, st: State<'_, AppState>, id: String) -> AppResult<()> {
    manager::start_download(&app, &st, &id)
}

#[tauri::command]
pub fn model_cancel(st: State<'_, AppState>, id: String) {
    manager::cancel_download(&st, &id);
}

#[tauri::command]
pub async fn model_select(app: AppHandle, id: String) -> AppResult<()> {
    blocking(app, move |app, st| manager::select(app, st, &id)).await
}

#[tauri::command]
pub async fn model_delete(app: AppHandle, id: String) -> AppResult<()> {
    blocking(app, move |app, st| manager::delete(app, st, &id)).await
}

#[tauri::command]
pub async fn model_import(app: AppHandle, id: String, path: String) -> AppResult<()> {
    blocking(app, move |app, st| manager::import(app, st, &id, Path::new(&path))).await
}

#[tauri::command]
pub async fn run_benchmark(app: AppHandle, model_id: String) -> AppResult<benchmark::BenchmarkResult> {
    blocking(app, move |app, st| benchmark::run(app, st, &model_id)).await
}

#[tauri::command]
pub async fn retry_gpu(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, st| {
        st.internal.lock().gpu_failure = None;
        st.persist_internal(app);
        if matches!(&*st.recorder.lock(), Phase::Idle) {
            st.load_selected_model();
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn mic_test(app: AppHandle, seconds: Option<u64>) -> AppResult<crate::mic_test::MicTestResult> {
    blocking(app, move |app, st| {
        #[cfg(windows)]
        {
            crate::mic_test::run(app, st, seconds.unwrap_or(10).clamp(3, 20))
        }
        #[cfg(not(windows))]
        {
            let _ = (app, st, seconds);
            Err(AppError::internal("unsupported platform"))
        }
    })
    .await
}

#[tauri::command]
pub async fn complete_setup(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, st| {
        st.internal.lock().setup_done = true;
        st.persist_internal(app);
        recorder::emit_state(app, st);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_window_layout(app: AppHandle, layout: Layout, persist: bool) -> AppResult<()> {
    blocking(app, move |app, st| {
        crate::window::apply_layout(app, layout);
        if persist {
            st.settings.write().window.layout = layout;
            st.persist_settings(app);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn window_action(app: AppHandle, action: String) -> AppResult<()> {
    let w = crate::window::main_window(&app).ok_or_else(|| AppError::internal("no window"))?;
    match action.as_str() {
        "minimize" => w.minimize()?,
        "hide" => crate::window::hide(&app),
        "close" => w.close()?,
        "show" => crate::window::show(&app),
        _ => return Err(AppError::internal("unknown action")),
    }
    Ok(())
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, st| {
        let busy = !matches!(&*st.recorder.lock(), Phase::Idle);
        if busy {
            st.quit_after_finish.store(true, std::sync::atomic::Ordering::SeqCst);
            if matches!(&*st.recorder.lock(), Phase::Recording(_)) {
                recorder::stop(app, st)?;
            }
        } else {
            app.exit(0);
        }
        Ok(())
    })
    .await
}

/// Installs the update the launch check found; refused while recording (section 17).
#[tauri::command]
pub async fn install_update(app: AppHandle) -> AppResult<()> {
    crate::updater::install(&app).await
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> AppResult<Option<String>> {
    blocking(app, |app, _| {
        Ok(app
            .dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
pub async fn pick_model_file(app: AppHandle) -> AppResult<Option<String>> {
    blocking(app, |app, _| {
        Ok(app
            .dialog()
            .file()
            .add_filter("Whisper GGML", &["bin"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned()))
    })
    .await
}
