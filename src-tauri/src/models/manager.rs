//! Model list, downloads, import and delete (FR-70, FR-71).

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::catalog::{self, CatalogEntry};
use super::download::{self, DownloadError, DownloadRequest};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{self, DownloadPayload};
use crate::settings::BenchmarkRecord;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub installed: bool,
    pub selected: bool,
    pub recommended: bool,
    /// Bytes already in the `.part` file (a download can resume).
    pub partial_bytes: u64,
    pub downloading: bool,
    pub benchmark: Option<BenchmarkRecord>,
}

pub fn list(st: &AppState) -> Vec<ModelEntry> {
    let selected = st.settings.read().model_id.clone();
    let recommended = catalog::recommend(crate::platform::total_memory());
    let downloads = st.downloads.lock();
    let benchmarks = st.internal.lock().benchmarks.clone();
    catalog::all()
        .iter()
        .map(|e| ModelEntry {
            installed: e.path_in(&st.models_dir).exists(),
            selected: e.id == selected,
            recommended: e.id == recommended,
            partial_bytes: std::fs::metadata(e.part_path_in(&st.models_dir)).map(|m| m.len()).unwrap_or(0),
            downloading: downloads.contains_key(&e.id),
            benchmark: benchmarks.get(&e.id).cloned(),
            entry: e.clone(),
        })
        .collect()
}

fn find(id: &str) -> AppResult<&'static CatalogEntry> {
    catalog::find(id).ok_or_else(|| AppError::internal(format!("unknown model {id}")))
}

fn emit_progress(
    app: &AppHandle,
    id: &str,
    received: u64,
    total: u64,
    bps: f64,
    phase: &'static str,
    error: Option<AppError>,
) {
    events::emit(
        app,
        events::MODEL_DOWNLOAD,
        &DownloadPayload { id: id.to_string(), received, total, bytes_per_sec: bps, phase, error },
    );
}

/// Starts a download in the background; progress arrives as `model://download` events.
pub fn start_download(app: &AppHandle, st: &AppState, id: &str) -> AppResult<()> {
    let entry = find(id)?.clone();
    if entry.path_in(&st.models_dir).exists() {
        emit_progress(app, id, entry.size_bytes, entry.size_bytes, 0.0, "done", None);
        return Ok(());
    }
    let (tx, rx) = tokio::sync::watch::channel(false);
    {
        let mut downloads = st.downloads.lock();
        if downloads.contains_key(id) {
            return Ok(());
        }
        downloads.insert(id.to_string(), tx);
    }
    let req = DownloadRequest {
        url: entry.url(),
        dest: entry.path_in(&st.models_dir),
        part: entry.part_path_in(&st.models_dir),
        expected_size: entry.size_bytes,
        sha256: entry.sha256.clone(),
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let id = entry.id.clone();
        let result = download::download(&download::client(), &req, rx, |p| {
            emit_progress(&app, &id, p.received, p.total, p.bytes_per_sec, "downloading", None);
        })
        .await;
        let st = app.state::<AppState>();
        st.downloads.lock().remove(&id);
        match result {
            Ok(()) => {
                tracing::info!("model {id} downloaded and verified");
                emit_progress(&app, &id, entry.size_bytes, entry.size_bytes, 0.0, "done", None);
                if st.settings.read().model_id == id {
                    st.load_selected_model();
                }
                crate::recorder::emit_state(&app, &st);
            }
            Err(DownloadError::Cancelled) => emit_progress(&app, &id, 0, entry.size_bytes, 0.0, "cancelled", None),
            Err(err) => {
                tracing::warn!("model {id} download failed: {err}");
                let code = match &err {
                    DownloadError::Checksum => ErrorCode::ModelChecksum,
                    DownloadError::DiskFull { .. } => ErrorCode::DiskFull,
                    DownloadError::Io(e) if crate::error::is_disk_full(e) => ErrorCode::DiskFull,
                    DownloadError::Network(_) | DownloadError::Http(_) => ErrorCode::Network,
                    DownloadError::Io(_) => ErrorCode::Internal,
                    DownloadError::Cancelled => unreachable!(),
                };
                emit_progress(&app, &id, 0, entry.size_bytes, 0.0, "error", Some(AppError::new(code, err.to_string())));
            }
        }
    });
    Ok(())
}

pub fn cancel_download(st: &AppState, id: &str) {
    if let Some(tx) = st.downloads.lock().get(id) {
        let _ = tx.send(true);
    }
}

pub fn select(app: &AppHandle, st: &AppState, id: &str) -> AppResult<()> {
    find(id)?;
    if !matches!(&*st.recorder.lock(), crate::recorder::Phase::Idle) {
        return Err(AppError::internal("cannot switch models while recording"));
    }
    st.settings.write().model_id = id.to_string();
    st.persist_settings(app);
    st.load_selected_model();
    crate::recorder::emit_state(app, st);
    Ok(())
}

pub fn delete(app: &AppHandle, st: &AppState, id: &str) -> AppResult<()> {
    let entry = find(id)?;
    if st.settings.read().model_id == id && !matches!(&*st.recorder.lock(), crate::recorder::Phase::Idle) {
        return Err(AppError::internal("the model is in use"));
    }
    if st.settings.read().model_id == id {
        st.worker.unload();
    }
    // The worker releases the file on its thread; retry briefly.
    let path = entry.path_in(&st.models_dir);
    for _ in 0..20 {
        match std::fs::remove_file(&path) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
    let _ = std::fs::remove_file(entry.part_path_in(&st.models_dir));
    st.internal.lock().benchmarks.remove(id);
    st.persist_internal(app);
    crate::recorder::emit_state(app, st);
    Ok(())
}

/// 「ファイルから追加」: verify a file the user downloaded elsewhere and copy it in (P1).
pub fn import(app: &AppHandle, st: &AppState, id: &str, source: &Path) -> AppResult<()> {
    let entry = find(id)?;
    let meta = std::fs::metadata(source)?;
    if meta.len() != entry.size_bytes {
        return Err(AppError::new(
            ErrorCode::ModelChecksum,
            format!("size {} does not match {}", meta.len(), entry.size_bytes),
        ));
    }
    let total = entry.size_bytes;
    let mut last = std::time::Instant::now();
    let digest = download::sha256_file(source, |done| {
        if last.elapsed().as_millis() >= 250 {
            last = std::time::Instant::now();
            emit_progress(app, id, done, total, 0.0, "verifying", None);
        }
    })?;
    if !digest.eq_ignore_ascii_case(&entry.sha256) {
        return Err(AppError::new(ErrorCode::ModelChecksum, "checksum mismatch"));
    }
    std::fs::create_dir_all(&st.models_dir)?;
    let part = entry.part_path_in(&st.models_dir);
    std::fs::copy(source, &part)?;
    std::fs::rename(&part, entry.path_in(&st.models_dir))?;
    emit_progress(app, id, total, total, 0.0, "done", None);
    if st.settings.read().model_id == id {
        st.load_selected_model();
    }
    crate::recorder::emit_state(app, st);
    Ok(())
}
