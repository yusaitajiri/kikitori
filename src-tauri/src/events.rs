//! Rust → UI events (section 14). Names and payloads are the IPC contract.

use serde::Serialize;
use serde_json::Value;

pub const RECORDING_STATE: &str = "recording://state";
pub const AUDIO_LEVELS: &str = "audio://levels";
pub const AUDIO_APP_LEVELS: &str = "audio://app-levels";
pub const TRANSCRIPT_PARTIAL: &str = "transcript://partial";
pub const TRANSCRIPT_SEGMENT: &str = "transcript://segment";
pub const TRANSCRIPT_SEGMENT_REMOVED: &str = "transcript://segment-removed";
/// A line changed after it arrived (marked important, FR-07); the payload is the whole segment.
pub const TRANSCRIPT_SEGMENT_UPDATED: &str = "transcript://segment-updated";
pub const TRANSCRIPT_SCREENSHOT: &str = "transcript://screenshot";
pub const TRANSCRIPT_MARKER: &str = "transcript://marker";
pub const ASR_LAG: &str = "asr://lag";
pub const FINISHING_PROGRESS: &str = "finishing://progress";
pub const MODEL_DOWNLOAD: &str = "model://download";
pub const MODEL_STATUS: &str = "model://status";
pub const SESSION_SAVED: &str = "session://saved";
pub const APP_NOTICE: &str = "app://notice";
pub const UI_COMMAND: &str = "ui://command";

/// Something that can deliver events to the UI. `AppHandle` in the app, a recorder in tests.
pub trait EventSink: Send + Sync {
    fn emit_json(&self, event: &str, payload: Value);
}

pub fn emit<T: Serialize>(sink: &dyn EventSink, event: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(v) => sink.emit_json(event, v),
        Err(e) => tracing::error!("event {event} failed to serialize: {e}"),
    }
}

impl<R: tauri::Runtime> EventSink for tauri::AppHandle<R> {
    fn emit_json(&self, event: &str, payload: Value) {
        use tauri::Emitter;
        if let Err(e) = self.emit(event, payload) {
            tracing::warn!("emit {event} failed: {e}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiState {
    NeedsModel,
    Ready,
    Recording,
    Paused,
    Finishing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePayload {
    pub state: UiState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<RecordedSources>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The window screenshots take, when one was picked for this recording (FR-34).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_window: Option<crate::screenshot::WindowInfo>,
    /// While recording: transcription waits for Stop (FR-25).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deferred: bool,
}

/// What is being recorded, for the UI to name in its own language ("Zoom + マイク").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedSources {
    /// The app or the whole system first, then the mic.
    pub ids: Vec<crate::session::model::SourceId>,
    /// The app's display name when one app is recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialPayload {
    pub source: crate::session::model::SourceId,
    pub utterance_id: String,
    pub text: String,
    /// Start of the utterance, so the UI can place the provisional text on the timeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t_start_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedPayload {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotPayload {
    pub id: String,
    pub t_ms: u64,
    pub thumb_data_url: String,
    pub width: u32,
    pub height: u32,
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LagPayload {
    pub lag_ms: u64,
    pub queued: usize,
    pub device: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub done: usize,
    pub total: usize,
    /// The same by audio, in ms: jobs differ in length, so this is what a progress bar shows.
    pub done_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPayload {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub bytes_per_sec: f64,
    /// "downloading" | "verifying" | "done" | "error" | "cancelled"
    pub phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::error::AppError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPayload {
    pub session_id: String,
    pub folder: String,
    pub transcript_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeAction {
    /// i18n key of the button label.
    pub label: String,
    /// What the UI does: a command name it understands (`open_mic_privacy`, ...).
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub level: NoticeLevel,
    pub code: String,
    /// i18n key; `params` fill its placeholders.
    pub message: String,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub params: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<NoticeAction>,
    /// Show as a toast instead of a banner.
    #[serde(default)]
    pub toast: bool,
}

impl Notice {
    pub fn new(level: NoticeLevel, code: &str, message: &str) -> Self {
        Self {
            level,
            code: code.into(),
            message: message.into(),
            params: Default::default(),
            action: None,
            toast: false,
        }
    }

    pub fn toast(mut self) -> Self {
        self.toast = true;
        self
    }

    pub fn param(mut self, k: &str, v: impl Into<Value>) -> Self {
        self.params.insert(k.into(), v.into());
        self
    }

    pub fn action(mut self, label: &str, command: &str) -> Self {
        self.action = Some(NoticeAction { label: label.into(), command: command.into() });
        self
    }
}

pub fn notice(sink: &dyn EventSink, n: Notice) {
    emit(sink, APP_NOTICE, &n);
}

#[cfg(test)]
pub mod test_sink {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    pub struct Recorded(pub Mutex<Vec<(String, Value)>>);

    impl EventSink for Recorded {
        fn emit_json(&self, event: &str, payload: Value) {
            self.0.lock().push((event.to_string(), payload));
        }
    }

    impl Recorded {
        pub fn named(&self, event: &str) -> Vec<Value> {
            self.0.lock().iter().filter(|(e, _)| e == event).map(|(_, v)| v.clone()).collect()
        }
    }
}
