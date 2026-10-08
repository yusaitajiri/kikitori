//! Per-source DSP thread (section 6): clock placement, downmix, resample, VAD, segmenter,
//! level meter. Utterances go to the ASR queue as jobs.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;

use super::clock::{ClockStats, Piece, StreamClock, downmix};
use super::level::{FLOOR_DBFS, PeakMeter, rms_dbfs};
use super::resample::{MonoResampler, TARGET_RATE};
use super::source::RawChunk;
use crate::asr::worker::{AsrWorker, Job, JobKind, SessionCtx};
use crate::platform;
use crate::segmenter::{SegEvent, Segmenter, SegmenterConfig};
use crate::session::model::SourceId;
use crate::vad::{EarshotVad, Vad};

/// Where jobs go; the real implementation is the ASR worker.
pub trait JobSink: Send + Sync {
    fn submit(&self, job: Job);
    fn partials_allowed(&self) -> bool;
}

impl JobSink for AsrWorker {
    fn submit(&self, job: Job) {
        AsrWorker::submit(self, job)
    }

    fn partials_allowed(&self) -> bool {
        AsrWorker::partials_allowed(self)
    }
}

/// Per-session counters for IDs (`utt_000001`, `seg_000001`, `img_0001`, `mk_0001`).
#[derive(Default)]
pub struct SessionIds {
    utt: std::sync::atomic::AtomicU64,
    seg: std::sync::atomic::AtomicU64,
    img: std::sync::atomic::AtomicU64,
    mk: std::sync::atomic::AtomicU64,
}

impl SessionIds {
    fn bump(c: &std::sync::atomic::AtomicU64) -> u64 {
        c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }
    pub fn next_utterance(&self) -> String {
        format!("utt_{:06}", Self::bump(&self.utt))
    }
    pub fn next_segment(&self) -> String {
        format!("seg_{:06}", Self::bump(&self.seg))
    }
    /// Returns the counter too, for the image file name.
    pub fn next_image(&self) -> (u64, String) {
        let n = Self::bump(&self.img);
        (n, format!("img_{n:04}"))
    }
    pub fn next_marker(&self) -> String {
        format!("mk_{:04}", Self::bump(&self.mk))
    }
}

/// Latest peak level per source, read by the 10 Hz emitter.
#[derive(Default)]
pub struct Levels {
    inner: Mutex<HashMap<SourceId, (f32, Instant)>>,
}

impl Levels {
    pub fn set(&self, source: SourceId, dbfs: f32) {
        self.inner.lock().insert(source, (dbfs, Instant::now()));
    }

    /// Readings older than 300 ms decay to the floor (a loopback stream that went quiet).
    pub fn snapshot(&self) -> HashMap<SourceId, f32> {
        self.inner
            .lock()
            .iter()
            .map(|(k, (v, at))| (*k, if at.elapsed() > Duration::from_millis(300) { FLOOR_DBFS } else { *v }))
            .collect()
    }

    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

#[derive(Clone)]
pub struct DspParams {
    pub source: SourceId,
    pub t0_100ns: u64,
    pub segmenter: SegmenterConfig,
    pub partials: bool,
    /// Pad with zeros when no packets arrive (system loopback goes silent by stopping).
    pub pad_quiet_stream: bool,
    /// Session time where this stream starts (0, or the resume time after a pause).
    pub start_ms: u64,
}

#[derive(Debug, Default, Clone)]
pub struct DspSummary {
    pub samples_16k: u64,
    pub utterances: u64,
    pub clock: ClockStats,
}

const PARTIAL_EVERY: usize = 24_000; // 1.5 s at 16 kHz
const QUIET_AFTER: Duration = Duration::from_millis(500);
const QUIET_MARGIN_100NS: u64 = 3_000_000; // pad up to 300 ms before now

struct Dsp {
    params: DspParams,
    ctx: Arc<SessionCtx>,
    sink: Arc<dyn JobSink>,
    ids: Arc<SessionIds>,
    levels: Arc<Levels>,
    clock: Option<StreamClock>,
    resampler: Option<MonoResampler>,
    vad: EarshotVad,
    seg: Segmenter,
    meter: PeakMeter,
    pending: Vec<f32>,
    current: Option<String>,
    last_partial_len: usize,
    summary: DspSummary,
    mono: Vec<f32>,
    out16: Vec<f32>,
}

pub fn spawn(
    rx: Receiver<RawChunk>,
    params: DspParams,
    ctx: Arc<SessionCtx>,
    sink: Arc<dyn JobSink>,
    ids: Arc<SessionIds>,
    levels: Arc<Levels>,
) -> JoinHandle<DspSummary> {
    std::thread::Builder::new()
        .name(format!("dsp-{}", params.source.as_str()))
        .spawn(move || {
            let mut vad = EarshotVad::new();
            vad.reset();
            let seg = Segmenter::with_position(params.segmenter.clone(), params.start_ms * TARGET_RATE as u64 / 1000);
            let mut dsp = Dsp {
                params,
                ctx,
                sink,
                ids,
                levels,
                clock: None,
                resampler: None,
                vad,
                seg,
                meter: PeakMeter::new(TARGET_RATE as usize / 10),
                pending: Vec::new(),
                current: None,
                last_partial_len: 0,
                summary: DspSummary::default(),
                mono: Vec::new(),
                out16: Vec::new(),
            };
            dsp.run(rx);
            dsp.summary
        })
        .expect("spawn DSP thread")
}

impl Dsp {
    fn run(&mut self, rx: Receiver<RawChunk>) {
        let mut last_packet = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => {
                    last_packet = Instant::now();
                    self.on_chunk(chunk);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.params.pad_quiet_stream && last_packet.elapsed() >= QUIET_AFTER {
                        self.pad_to(platform::qpc_now_100ns().saturating_sub(QUIET_MARGIN_100NS));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.finish();
    }

    /// The 16 kHz position, including samples not yet framed.
    fn position_16k(&self) -> u64 {
        self.seg.position() + self.pending.len() as u64
    }

    fn on_chunk(&mut self, chunk: RawChunk) {
        let needs_new = match &self.clock {
            Some(c) => c.rate() != chunk.rate || c.channels() != chunk.channels,
            None => true,
        };
        if needs_new {
            self.flush_resampler();
            let pos_16k = self.position_16k();
            let start_frame = pos_16k * chunk.rate as u64 / TARGET_RATE as u64;
            self.clock =
                Some(StreamClock::with_position(self.params.t0_100ns, chunk.rate, chunk.channels, start_frame));
            match MonoResampler::new(chunk.rate) {
                Ok(r) => self.resampler = Some(r),
                Err(e) => {
                    tracing::error!("resampler for {} Hz failed: {e:#}", chunk.rate);
                    return;
                }
            }
        }
        let clock = self.clock.as_mut().unwrap();
        let mut pieces: Vec<Piece> = Vec::with_capacity(2);
        clock.place(chunk.qpc_100ns, &chunk.samples, chunk.silent_flag, |p| pieces.push(p));
        for piece in pieces {
            match piece {
                Piece::Zeros(frames) => self.feed_zeros(frames),
                Piece::Data(data) => self.feed_interleaved(data),
            }
        }
    }

    fn pad_to(&mut self, qpc_100ns: u64) {
        let Some(clock) = self.clock.as_mut() else { return };
        let frames = clock.pad_until(qpc_100ns);
        if frames > 0 {
            self.feed_zeros(frames);
        }
    }

    /// Silence at the device rate, in half-second pieces so a long gap stays cheap.
    fn feed_zeros(&mut self, frames: u64) {
        let rate = self.clock.as_ref().map(|c| c.rate()).unwrap_or(TARGET_RATE) as u64;
        let step = (rate / 2).max(1);
        let mut left = frames;
        while left > 0 {
            let n = left.min(step) as usize;
            self.mono.clear();
            self.mono.resize(n, 0.0);
            self.resample_mono();
            left -= n as u64;
        }
    }

    fn feed_interleaved(&mut self, data: &[f32]) {
        let channels = self.clock.as_ref().map(|c| c.channels()).unwrap_or(1);
        self.mono.clear();
        downmix(data, channels, &mut self.mono);
        self.resample_mono();
    }

    fn resample_mono(&mut self) {
        self.out16.clear();
        if let Some(r) = self.resampler.as_mut()
            && let Err(e) = r.process(&self.mono, &mut self.out16)
        {
            tracing::error!("resample failed: {e:#}");
            return;
        }
        let out = std::mem::take(&mut self.out16);
        self.feed(&out);
        self.out16 = out;
    }

    fn flush_resampler(&mut self) {
        if let Some(mut r) = self.resampler.take() {
            let mut tail = Vec::new();
            if r.flush(&mut tail).is_ok() {
                self.feed(&tail);
            }
        }
    }

    fn feed(&mut self, samples: &[f32]) {
        self.summary.samples_16k += samples.len() as u64;
        self.pending.extend_from_slice(samples);
        let fl = self.seg.frame_len();
        let mut offset = 0;
        while self.pending.len() - offset >= fl {
            let frame: Vec<f32> = self.pending[offset..offset + fl].to_vec();
            offset += fl;
            let levels = self.levels.clone();
            let source = self.params.source;
            self.meter.push(&frame, |db| levels.set(source, db));
            let score = self.vad.score(&frame);
            for ev in self.seg.push_frame(&frame, score) {
                self.on_event(ev);
            }
            self.maybe_partial();
        }
        self.pending.drain(..offset);
    }

    fn on_event(&mut self, ev: SegEvent) {
        match ev {
            SegEvent::Opened { start_sample } => {
                let id = self.ids.next_utterance();
                self.ctx.speaking.lock().entry(self.params.source).or_default().open =
                    Some((id.clone(), start_sample * 1000 / TARGET_RATE as u64));
                self.current = Some(id);
                self.last_partial_len = 0;
            }
            SegEvent::Closed(u) => {
                let utterance_id = self.current.take().unwrap_or_else(|| self.ids.next_utterance());
                {
                    let mut speaking = self.ctx.speaking.lock();
                    let s = speaking.entry(self.params.source).or_default();
                    s.open = None;
                    s.last = Some((utterance_id.clone(), u.end_ms()));
                }
                self.summary.utterances += 1;
                let rms = rms_dbfs(&u.samples);
                let speech_ms = u.speech_ms;
                self.sink.submit(Job {
                    ctx: self.ctx.clone(),
                    source: self.params.source,
                    utterance_id,
                    start_ms: u.start_ms(),
                    samples: u.samples,
                    rms_dbfs: rms,
                    speech_ms,
                    kind: JobKind::Final,
                    retries: 0,
                });
            }
            SegEvent::Discarded { .. } => {
                self.current = None;
                self.ctx.speaking.lock().entry(self.params.source).or_default().open = None;
            }
        }
    }

    fn maybe_partial(&mut self) {
        if !self.params.partials {
            return;
        }
        let Some((start, audio)) = self.seg.open_audio() else { return };
        if audio.len() < PARTIAL_EVERY || audio.len() - self.last_partial_len < PARTIAL_EVERY {
            return;
        }
        if !self.sink.partials_allowed() {
            return;
        }
        let Some(id) = self.current.clone() else { return };
        self.last_partial_len = audio.len();
        self.sink.submit(Job {
            ctx: self.ctx.clone(),
            source: self.params.source,
            utterance_id: id,
            start_ms: start * 1000 / TARGET_RATE as u64,
            samples: audio.to_vec(),
            rms_dbfs: 0.0,
            speech_ms: 0,
            kind: JobKind::Partial,
            retries: 0,
        });
    }

    fn finish(&mut self) {
        self.flush_resampler();
        // Frame the remainder with zero padding so the last syllable reaches the VAD.
        let fl = self.seg.frame_len();
        let rem = self.pending.len() % fl;
        if rem > 0 {
            let pad = vec![0.0; fl - rem];
            self.feed(&pad);
        }
        if let Some(ev) = self.seg.flush() {
            self.on_event(ev);
        }
        if let Some(c) = &self.clock {
            self.summary.clock = c.stats;
        }
        self.levels.set(self.params.source, FLOOR_DBFS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::worker::{AsrConfig, FinalResult, SegmentSink};
    use crate::settings::Language;

    struct Collect(Mutex<Vec<Job>>);
    impl JobSink for Collect {
        fn submit(&self, job: Job) {
            self.0.lock().push(job);
        }
        fn partials_allowed(&self) -> bool {
            true
        }
    }
    struct NullSink;
    impl SegmentSink for NullSink {
        fn on_final(&self, _: FinalResult) {}
        fn on_partial(&self, _: SourceId, _: &str, _: u64, _: &str) {}
        fn on_language(&self, _: &str) {}
    }

    /// Speech-like noise bursts make earshot fire; silence keeps it quiet.
    fn voice_like(secs: f32, rate: u32) -> Vec<f32> {
        let n = (secs * rate as f32) as usize;
        let mut x = 12345u32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let noise = ((x >> 16) as f32 / 32_768.0 - 1.0) * 0.1;
                let t = i as f32 / rate as f32;
                // Voiced harmonics around 150 Hz with a syllable-rate envelope.
                let env = (2.0 * std::f32::consts::PI * 4.0 * t).sin().abs();
                let voiced: f32 =
                    (1..8).map(|k| (2.0 * std::f32::consts::PI * 150.0 * k as f32 * t).sin() / k as f32).sum();
                (voiced * 0.2 + noise) * env
            })
            .collect()
    }

    #[test]
    fn pipeline_places_and_cuts_with_correct_times() {
        let t0 = 5_000_000_000u64;
        let (tx, rx) = crossbeam_channel::unbounded();
        let collect = Arc::new(Collect(Mutex::new(Vec::new())));
        let ctx = SessionCtx::new(
            "s".into(),
            AsrConfig {
                language: Language::Ja,
                vocabulary: vec![],
                accuracy_first: false,
                hallucination_filter: true,
                audio_ctx_experimental: false,
            },
            Arc::new(NullSink),
        );
        let params = DspParams {
            source: SourceId::App,
            t0_100ns: t0,
            segmenter: SegmenterConfig::default(),
            partials: false,
            pad_quiet_stream: false,
            start_ms: 0,
        };
        let ctx_seen = ctx.clone();
        let handle =
            spawn(rx, params, ctx, collect.clone(), Arc::new(SessionIds::default()), Arc::new(Levels::default()));

        // Stereo 48 kHz: 1 s silence, 2 s "speech", then a 2 s gap with no packets at all,
        // then 1 s of silence.
        let rate = 48_000u32;
        let send = |start_ms: u64, mono: &[f32]| {
            for (i, chunk) in mono.chunks(480).enumerate() {
                let stereo: Vec<f32> = chunk.iter().flat_map(|&s| [s, s]).collect();
                let qpc = t0 + (start_ms * 10_000) + (i as u64 * 480 * 10_000_000 / rate as u64);
                tx.send(RawChunk {
                    source: SourceId::App,
                    qpc_100ns: qpc,
                    rate,
                    channels: 2,
                    samples: stereo,
                    silent_flag: false,
                })
                .unwrap();
            }
        };
        send(0, &vec![0.0; rate as usize]);
        send(1000, &voice_like(2.0, rate));
        send(5000, &vec![0.0; rate as usize]);
        drop(tx);
        let summary = handle.join().unwrap();
        let jobs = collect.0.lock();
        assert!(!jobs.is_empty(), "no utterance detected");
        let first = &jobs[0];
        assert!((600..=1100).contains(&first.start_ms), "start {}", first.start_ms);
        assert!(first.end_ms() <= 3_700, "end {}", first.end_ms());
        assert_eq!(first.utterance_id, "utt_000001");
        // The utterance is remembered as the last one said, for marking it (FR-07).
        let speaking = ctx_seen.speaking.lock().get(&SourceId::App).cloned().unwrap_or_default();
        assert_eq!(speaking.open, None);
        assert_eq!(speaking.last.as_ref().map(|(id, _)| id.as_str()), Some("utt_000001"));
        // 6 s of audio at 16 kHz, the 2 s gap filled with zeros.
        assert!((summary.samples_16k as i64 - 96_000).abs() < 2_000, "{}", summary.samples_16k);
        assert_eq!(summary.clock.gaps_filled, 1);
    }
}
