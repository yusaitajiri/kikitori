//! Process-wide state shared by commands, the tray, hotkeys and worker threads.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::watch;

use crate::asr::worker::{AsrWorker, EngineStatus, ModelSpec, WorkerHost};
use crate::audio::dsp::Levels;
use crate::events::{self, Notice, NoticeLevel};
use crate::models::catalog;
use crate::recorder::Phase;
use crate::settings::{self, AppStateFile, GpuFailure, Settings};

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub internal: Mutex<AppStateFile>,
    pub worker: Arc<AsrWorker>,
    pub recorder: Mutex<Phase>,
    pub levels: Arc<Levels>,
    pub models_dir: PathBuf,
    pub default_output_root: String,
    /// Hotkeys that failed to register: action → error text.
    pub hotkey_errors: Mutex<BTreeMap<String, String>>,
    pub downloads: Mutex<HashMap<String, watch::Sender<bool>>>,
    pub last_shot: Mutex<Option<Instant>>,
    pub quit_after_finish: AtomicBool,
    #[cfg(windows)]
    pub capturer: crate::screenshot::XcapCapturer,
    /// How loud each app in the source picker is.
    #[cfg(windows)]
    pub meters: crate::audio::win::meters::AppMeters,
}

impl AppState {
    pub fn new(app: &AppHandle, worker: Arc<AsrWorker>) -> Self {
        let paths = app.path();
        let models_dir =
            paths.app_local_data_dir().map(|d| d.join("models")).unwrap_or_else(|_| PathBuf::from("models"));
        let default_output_root = paths
            .document_dir()
            .map(|d| d.join("Kikitori"))
            .unwrap_or_else(|_| PathBuf::from("Kikitori"))
            .to_string_lossy()
            .into_owned();
        let settings = load_settings(app, &default_output_root);
        let internal = load_internal(app);
        Self {
            settings: RwLock::new(settings),
            internal: Mutex::new(internal),
            worker,
            recorder: Mutex::new(Phase::Idle),
            levels: Arc::new(Levels::default()),
            models_dir,
            default_output_root,
            hotkey_errors: Mutex::new(BTreeMap::new()),
            downloads: Mutex::new(HashMap::new()),
            last_shot: Mutex::new(None),
            quit_after_finish: AtomicBool::new(false),
            #[cfg(windows)]
            capturer: crate::screenshot::XcapCapturer,
            #[cfg(windows)]
            meters: Default::default(),
        }
    }

    pub fn model_installed(&self, id: &str) -> bool {
        catalog::find(id).is_some_and(|e| e.path_in(&self.models_dir).exists())
    }

    pub fn any_model_installed(&self) -> bool {
        catalog::all().iter().any(|e| e.path_in(&self.models_dir).exists())
    }

    /// Loads the selected model in the background (section 8). GPU is skipped when it
    /// failed before for this model and GPU, or crashed the app last time.
    pub fn load_selected_model(&self) {
        let s = self.settings.read().clone();
        let Some(entry) = catalog::find(&s.model_id) else { return };
        let path = entry.path_in(&self.models_dir);
        if !path.exists() {
            self.worker.unload();
            return;
        }
        let gpu_name = crate::asr::engine::gpu_device_name().unwrap_or_default();
        let failed_before =
            self.internal.lock().gpu_failure.as_ref().is_some_and(|f| f.model_id == entry.id && f.gpu_name == gpu_name);
        self.worker.load_model(ModelSpec { path, model_id: entry.id.clone(), try_gpu: s.use_gpu && !failed_before });
    }

    /// Partials only on the 快適 tier (or before any benchmark ran).
    pub fn partials_tier_ok(&self, model_id: &str) -> bool {
        self.internal.lock().benchmarks.get(model_id).is_none_or(|b| b.tier == "comfortable")
    }

    pub fn persist_settings(&self, app: &AppHandle) {
        let s = self.settings.read().clone();
        if let Err(e) = save_settings(app, &s) {
            tracing::error!("saving settings failed: {e}");
        }
    }

    pub fn persist_internal(&self, app: &AppHandle) {
        let s = self.internal.lock().clone();
        if let Err(e) = save_internal(app, &s) {
            tracing::error!("saving state failed: {e}");
        }
    }
}

pub fn load_settings(app: &AppHandle, default_root: &str) -> Settings {
    let stored = match app.store(settings::SETTINGS_FILE) {
        Ok(store) => Value::Object(store.entries().into_iter().collect()),
        Err(e) => {
            tracing::warn!("settings store unavailable: {e}");
            Value::Object(Default::default())
        }
    };
    let mut settings = settings::parse_lenient(&stored, default_root);
    if let Some(locale) = settings::initial_locale(&stored, crate::platform::display_language_is_japanese()) {
        settings.locale = locale;
    }
    // Write back the validated form so unknown keys disappear from disk.
    if let Err(e) = save_settings(app, &settings) {
        tracing::warn!("rewriting settings failed: {e}");
    }
    settings
}

pub fn save_settings(app: &AppHandle, s: &Settings) -> anyhow::Result<()> {
    let store = app.store(settings::SETTINGS_FILE)?;
    store.clear();
    if let Value::Object(map) = serde_json::to_value(s)? {
        for (k, v) in map {
            store.set(k, v);
        }
    }
    store.save()?;
    Ok(())
}

pub fn load_internal(app: &AppHandle) -> AppStateFile {
    app.store(settings::STATE_FILE)
        .ok()
        .and_then(|store| store.get("state"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn save_internal(app: &AppHandle, s: &AppStateFile) -> anyhow::Result<()> {
    let store = app.store(settings::STATE_FILE)?;
    store.set("state", serde_json::to_value(s)?);
    store.save()?;
    Ok(())
}

/// Connects the ASR worker to the app: status events, the GPU crash marker, fallback notice.
pub struct AppHost {
    pub app: AppHandle,
}

impl WorkerHost for AppHost {
    fn status_changed(&self, status: &EngineStatus) {
        events::emit(&self.app, events::MODEL_STATUS, status);
        if let Some(st) = self.app.try_state::<AppState>() {
            crate::recorder::emit_state(&self.app, &st);
        }
    }

    fn gpu_init(&self, model_id: &str, begin: bool) {
        let Some(st) = self.app.try_state::<AppState>() else { return };
        let gpu_name = crate::asr::engine::gpu_device_name().unwrap_or_default();
        st.internal.lock().gpu_init_in_progress =
            begin.then(|| GpuFailure { model_id: model_id.to_string(), gpu_name });
        st.persist_internal(&self.app);
    }

    fn gpu_fallback(&self, model_id: &str) {
        let Some(st) = self.app.try_state::<AppState>() else { return };
        let gpu_name = crate::asr::engine::gpu_device_name().unwrap_or_default();
        st.internal.lock().gpu_failure = Some(GpuFailure { model_id: model_id.to_string(), gpu_name });
        st.persist_internal(&self.app);
        events::notice(&self.app, Notice::new(NoticeLevel::Warn, "E_GPU_FALLBACK", "gpuFallback").toast());
    }
}

/// At launch: a GPU init that never finished means it crashed the app; remember it.
pub fn check_gpu_crash_marker(app: &AppHandle, st: &AppState) {
    let crashed = st.internal.lock().gpu_init_in_progress.take();
    if let Some(failure) = crashed {
        tracing::warn!("GPU init crashed last time ({failure:?}); using CPU");
        st.internal.lock().gpu_failure = Some(failure);
        st.persist_internal(app);
        events::notice(app, Notice::new(NoticeLevel::Warn, "E_GPU_FALLBACK", "gpuFallback").toast());
    }
}
