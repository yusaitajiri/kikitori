//! The one error type that crosses the IPC boundary: `{ code, message }`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCode {
    #[serde(rename = "E_MIC_DENIED")]
    MicDenied,
    #[serde(rename = "E_APP_LOOPBACK_UNSUPPORTED")]
    AppLoopbackUnsupported,
    #[serde(rename = "E_SOURCE_LOST")]
    SourceLost,
    #[serde(rename = "E_SOURCE_SILENT")]
    SourceSilent,
    #[serde(rename = "E_MODEL_MISSING")]
    ModelMissing,
    #[serde(rename = "E_MODEL_CHECKSUM")]
    ModelChecksum,
    #[serde(rename = "E_GPU_FALLBACK")]
    GpuFallback,
    #[serde(rename = "E_DISK_FULL")]
    DiskFull,
    #[serde(rename = "E_HOTKEY_TAKEN")]
    HotkeyTaken,
    #[serde(rename = "E_PDF_FAILED")]
    PdfFailed,
    #[serde(rename = "E_NETWORK")]
    Network,
    #[serde(rename = "E_INTERNAL")]
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::MicDenied => "E_MIC_DENIED",
            ErrorCode::AppLoopbackUnsupported => "E_APP_LOOPBACK_UNSUPPORTED",
            ErrorCode::SourceLost => "E_SOURCE_LOST",
            ErrorCode::SourceSilent => "E_SOURCE_SILENT",
            ErrorCode::ModelMissing => "E_MODEL_MISSING",
            ErrorCode::ModelChecksum => "E_MODEL_CHECKSUM",
            ErrorCode::GpuFallback => "E_GPU_FALLBACK",
            ErrorCode::DiskFull => "E_DISK_FULL",
            ErrorCode::HotkeyTaken => "E_HOTKEY_TAKEN",
            ErrorCode::PdfFailed => "E_PDF_FAILED",
            ErrorCode::Network => "E_NETWORK",
            ErrorCode::Internal => "E_INTERNAL",
        }
    }
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

/// Windows reports a full disk as ERROR_DISK_FULL (112) or ERROR_HANDLE_DISK_FULL (39).
pub fn is_disk_full(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(112) | Some(39)) || err.kind() == std::io::ErrorKind::StorageFull
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        if is_disk_full(&err) {
            AppError::new(ErrorCode::DiskFull, err.to_string())
        } else {
            AppError::internal(err.to_string())
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::internal(err.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::internal(err.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        match err.downcast::<AppError>() {
            Ok(app) => app,
            Err(other) => AppError::internal(format!("{other:#}")),
        }
    }
}
