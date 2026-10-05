//! Runs one clip through the ASR worker and waits for the result (benchmark, mic test).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, bounded};
use parking_lot::Mutex;

use super::worker::{AsrConfig, AsrWorker, EngineState, FinalResult, Job, JobKind, SegmentSink, SessionCtx};
use crate::audio::level::rms_dbfs;
use crate::session::model::SourceId;

struct OneShotSink {
    tx: Mutex<Option<Sender<FinalResult>>>,
}

impl SegmentSink for OneShotSink {
    fn on_final(&self, result: FinalResult) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(result);
        }
    }
    fn on_partial(&self, _: SourceId, _: &str, _: u64, _: &str) {}
    fn on_language(&self, _: &str) {}
}

/// Waits until the engine is ready (or failed) for up to `timeout`.
pub fn wait_ready(worker: &AsrWorker, timeout: Duration) -> bool {
    let started = Instant::now();
    loop {
        match worker.status().state {
            EngineState::Ready => return true,
            EngineState::Failed => return false,
            _ if started.elapsed() > timeout => return false,
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Transcribes 16 kHz mono samples; returns the result and the processing time.
pub fn transcribe(
    worker: &AsrWorker,
    samples: Vec<f32>,
    config: AsrConfig,
    timeout: Duration,
) -> anyhow::Result<(FinalResult, Duration)> {
    let (tx, rx) = bounded(1);
    let sink = Arc::new(OneShotSink { tx: Mutex::new(Some(tx)) });
    let ctx = SessionCtx::new(format!("oneshot-{}", ulid::Ulid::generate()), config, sink);
    let rms = rms_dbfs(&samples);
    let started = Instant::now();
    worker.submit(Job {
        ctx,
        source: SourceId::Mic,
        utterance_id: "utt_000001".into(),
        start_ms: 0,
        samples,
        rms_dbfs: rms,
        speech_ms: u32::MAX,
        kind: JobKind::Final,
        retries: 0,
    });
    let result = rx.recv_timeout(timeout).map_err(|_| anyhow::anyhow!("transcription timed out"));
    worker.release_states();
    Ok((result?, started.elapsed()))
}
