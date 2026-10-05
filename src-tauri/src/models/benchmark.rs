//! 10-second benchmark on a bundled Japanese clip (FR-72, section 12).

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::asr::oneshot;
use crate::asr::worker::{AsrConfig, EngineState, ModelSpec};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::{BenchmarkRecord, Language};
use crate::state::AppState;

pub const CLIP: &str = "resources/bench_ja.wav";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// 快適: partials on.
    Comfortable,
    /// 普通: partials off.
    Ok,
    /// 重い: suggest `small-q5`.
    Heavy,
}

impl Tier {
    pub fn from_seconds(secs: f32) -> Self {
        if secs < 1.5 {
            Tier::Comfortable
        } else if secs <= 4.0 {
            Tier::Ok
        } else {
            Tier::Heavy
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Comfortable => "comfortable",
            Tier::Ok => "ok",
            Tier::Heavy => "heavy",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResult {
    pub seconds: f32,
    pub tier: Tier,
    pub gpu: bool,
    pub text: String,
}

pub fn run(app: &AppHandle, st: &AppState, model_id: &str) -> AppResult<BenchmarkResult> {
    if !matches!(&*st.recorder.lock(), crate::recorder::Phase::Idle) {
        return Err(AppError::internal("not while recording"));
    }
    let entry = crate::models::catalog::find(model_id).ok_or_else(|| AppError::internal("unknown model"))?;
    let path = entry.path_in(&st.models_dir);
    if !path.exists() {
        return Err(AppError::new(ErrorCode::ModelMissing, "model not downloaded"));
    }
    let status = st.worker.status();
    if status.model_id.as_deref() != Some(model_id) || status.state != EngineState::Ready {
        let use_gpu = st.settings.read().use_gpu;
        st.worker.load_model(ModelSpec { path, model_id: model_id.to_string(), try_gpu: use_gpu });
        std::thread::sleep(Duration::from_millis(200));
    }
    if !oneshot::wait_ready(&st.worker, Duration::from_secs(300)) {
        return Err(AppError::internal("model failed to load"));
    }
    let clip = app.path().resource_dir().map_err(|e| AppError::internal(e.to_string()))?.join(CLIP);
    let (samples, rate, channels) =
        crate::audio::file_source::read_wav(&clip).map_err(|e| AppError::internal(format!("benchmark clip: {e:#}")))?;
    if rate != 16_000 || channels != 1 {
        return Err(AppError::internal("benchmark clip must be 16 kHz mono"));
    }
    let config = AsrConfig {
        language: Language::Ja,
        vocabulary: Vec::new(),
        accuracy_first: false,
        hallucination_filter: true,
        audio_ctx_experimental: false,
    };
    let (result, elapsed) =
        oneshot::transcribe(&st.worker, samples, config, Duration::from_secs(300)).map_err(AppError::from)?;
    let seconds = elapsed.as_secs_f32();
    let tier = Tier::from_seconds(seconds);
    let gpu = st.worker.status().gpu;
    st.internal
        .lock()
        .benchmarks
        .insert(model_id.to_string(), BenchmarkRecord { seconds, tier: tier.as_str().into(), gpu });
    st.persist_internal(app);
    if st.settings.read().model_id != model_id {
        st.load_selected_model();
    }
    let text = result.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("");
    tracing::info!("benchmark {model_id}: {seconds:.2}s ({}), gpu={gpu}", tier.as_str());
    Ok(BenchmarkResult { seconds, tier, gpu, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_match_spec() {
        assert_eq!(Tier::from_seconds(0.8), Tier::Comfortable);
        assert_eq!(Tier::from_seconds(1.5), Tier::Ok);
        assert_eq!(Tier::from_seconds(4.0), Tier::Ok);
        assert_eq!(Tier::from_seconds(4.1), Tier::Heavy);
    }
}
