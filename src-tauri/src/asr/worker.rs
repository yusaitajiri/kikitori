//! The single ASR worker thread (section 8). It owns the Whisper model and one state per
//! source, takes final jobs before partial ones (oldest first across sources), merges jobs
//! under backlog, and hands post-processed segments to the session's sink.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use whisper_rs::{FullParams, WhisperState};

use super::engine::{Engine, LoadOutcome};
use super::language;
use super::params::{self, DecodeOptions};
use super::postprocess::{Outcome, PostProcessor, UtteranceStats, is_symbols_only, tidy_spaces};
use crate::session::model::SourceId;
use crate::settings::Language;

pub const SAMPLE_RATE: usize = 16_000;
pub const MERGE_MAX_MS: u64 = 25_000;
pub const MERGE_GAP_MS: u64 = 200;
pub const BACKLOG_LAG_MS: u64 = 5_000;
pub const LANGUAGE_LOCK_MIN_MS: u64 = 3_000;
/// Below this much speech, Whisper tends to echo its prompt back (a lone 「四」 came out as
/// 「よろしくお願いします。」, the primer's last sentence), so such clips get only the word list.
pub const SHORT_SPEECH_MS: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Final,
    Partial,
}

#[derive(Debug, Clone)]
pub struct AsrConfig {
    pub language: Language,
    pub vocabulary: Vec<String>,
    pub accuracy_first: bool,
    pub hallucination_filter: bool,
    pub audio_ctx_experimental: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalSegment {
    pub utterance_id: String,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub text: String,
    pub no_speech: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PieceInfo {
    pub utterance_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FinalResult {
    pub source: SourceId,
    pub pieces: Vec<PieceInfo>,
    pub segments: Vec<FinalSegment>,
}

/// Where a session's results go (implemented by the recorder).
pub trait SegmentSink: Send + Sync {
    fn on_final(&self, result: FinalResult);
    fn on_partial(&self, source: SourceId, utterance_id: &str, start_ms: u64, text: &str);
    fn on_language(&self, language: &str);
}

/// One source's utterances as the DSP thread cuts them: the one being spoken and the last that
/// ended, each as (utterance ID, time in ms). Marking "the line being said" (FR-07) reads them
/// before the line has any text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Speaking {
    pub open: Option<(String, u64)>,
    pub last: Option<(String, u64)>,
}

/// The utterances "the current line" means at `t_ms`: every one being spoken then, else the one
/// that ended last.
pub fn current_utterances(speaking: &HashMap<SourceId, Speaking>, t_ms: u64) -> Vec<String> {
    let mut open: Vec<(u64, String)> = speaking
        .values()
        .filter_map(|s| s.open.as_ref())
        .filter(|(_, start)| *start <= t_ms)
        .map(|(id, start)| (*start, id.clone()))
        .collect();
    if !open.is_empty() {
        open.sort();
        return open.into_iter().map(|(_, id)| id).collect();
    }
    speaking
        .values()
        .filter_map(|s| s.last.as_ref())
        .max_by_key(|(_, end)| *end)
        .map(|(id, _)| id.clone())
        .into_iter()
        .collect()
}

/// Per-recording state shared by the DSP threads and the worker.
pub struct SessionCtx {
    pub session_id: String,
    pub config: AsrConfig,
    pub sink: Arc<dyn SegmentSink>,
    pub locked_language: Mutex<Option<String>>,
    /// Last accepted segment text per source, for the prompt.
    pub last_text: Mutex<HashMap<SourceId, String>>,
    /// Per source, the language its clips start in since Whisper was sure they were not in the
    /// configured one (see `language`).
    pub switched: Mutex<HashMap<SourceId, String>>,
    /// Per source, what is being said (see [`Speaking`]).
    pub speaking: Mutex<HashMap<SourceId, Speaking>>,
}

impl SessionCtx {
    pub fn new(session_id: String, config: AsrConfig, sink: Arc<dyn SegmentSink>) -> Arc<Self> {
        Arc::new(Self {
            session_id,
            config,
            sink,
            locked_language: Mutex::new(None),
            last_text: Mutex::new(HashMap::new()),
            switched: Mutex::new(HashMap::new()),
            speaking: Mutex::new(HashMap::new()),
        })
    }

    /// The language clips are decoded in: the configured one, the one 自動 locked, or `auto`.
    fn home_language(&self) -> String {
        match self.config.language {
            Language::Ja => "ja".into(),
            Language::En => "en".into(),
            Language::Auto => self.locked_language.lock().clone().unwrap_or_else(|| "auto".into()),
        }
    }
}

#[derive(Clone)]
pub struct Job {
    pub ctx: Arc<SessionCtx>,
    pub source: SourceId,
    pub utterance_id: String,
    pub start_ms: u64,
    pub samples: Vec<f32>,
    pub rms_dbfs: f32,
    /// Speech inside the clip (without pre-roll and trailing silence).
    pub speech_ms: u32,
    pub kind: JobKind,
    pub retries: u8,
}

impl Job {
    pub fn duration_ms(&self) -> u64 {
        (self.samples.len() * 1000 / SAMPLE_RATE) as u64
    }

    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.duration_ms()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    /// No model selected or the file is missing.
    Missing,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: EngineState,
    pub gpu: bool,
    pub device_name: Option<String>,
    pub model_id: Option<String>,
    pub error: Option<String>,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self { state: EngineState::Missing, gpu: false, device_name: None, model_id: None, error: None }
    }
}

/// Callbacks from the worker to the app.
pub trait WorkerHost: Send + Sync {
    fn status_changed(&self, status: &EngineStatus);
    /// Called right before a GPU init and right after it (crash detection, section 8).
    fn gpu_init(&self, model_id: &str, begin: bool);
    fn gpu_fallback(&self, model_id: &str);
}

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub path: PathBuf,
    pub model_id: String,
    pub try_gpu: bool,
}

enum Command {
    Load(ModelSpec),
    Unload,
    Shutdown,
}

struct InFlight {
    session_id: String,
    kind: JobKind,
    start_ms: u64,
    end_ms: u64,
}

#[derive(Default)]
struct Queue {
    finals: Vec<Job>,
    partials: HashMap<SourceId, Job>,
    in_flight: Option<InFlight>,
    /// Sessions whose finals wait for the recording to stop (FR-25): queued, not run.
    deferred: HashSet<String>,
    command: Option<Command>,
    /// Drop the Whisper states once the queue is empty (set when a recording ends).
    release_states: bool,
    engine_ready: bool,
    engine_gpu: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    status: Mutex<EngineStatus>,
    partials_ok: AtomicBool,
}

pub struct AsrWorker {
    shared: Arc<Shared>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl AsrWorker {
    pub fn spawn(host: Arc<dyn WorkerHost>) -> Arc<Self> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            cv: Condvar::new(),
            status: Mutex::new(EngineStatus::default()),
            partials_ok: AtomicBool::new(false),
        });
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("asr-worker".into())
            .spawn(move || run(s, host))
            .expect("spawn ASR worker");
        Arc::new(Self { shared, handle: Mutex::new(Some(handle)) })
    }

    pub fn load_model(&self, spec: ModelSpec) {
        let mut q = self.shared.queue.lock();
        q.command = Some(Command::Load(spec));
        self.shared.cv.notify_all();
    }

    pub fn unload(&self) {
        let mut q = self.shared.queue.lock();
        q.command = Some(Command::Unload);
        self.shared.cv.notify_all();
    }

    /// Frees the per-source Whisper states (their GPU buffers) once nothing is queued. A
    /// recording that starts in the meantime cancels it; the next job recreates them.
    pub fn release_states(&self) {
        let mut q = self.shared.queue.lock();
        q.release_states = true;
        self.shared.cv.notify_all();
    }

    pub fn shutdown(&self) {
        {
            let mut q = self.shared.queue.lock();
            q.command = Some(Command::Shutdown);
            self.shared.cv.notify_all();
        }
        if let Some(h) = self.handle.lock().take() {
            let _ = h.join();
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.shared.status.lock().clone()
    }

    pub fn submit(&self, job: Job) {
        let mut q = self.shared.queue.lock();
        q.release_states = false;
        match job.kind {
            JobKind::Final => {
                // A final supersedes the partial for the same utterance.
                if q.partials.get(&job.source).is_some_and(|p| p.utterance_id == job.utterance_id) {
                    q.partials.remove(&job.source);
                }
                q.finals.push(job);
            }
            JobKind::Partial => {
                if !q.deferred.contains(&job.ctx.session_id) {
                    q.partials.insert(job.source, job);
                }
            }
        }
        update_partials_ok(&self.shared, &q);
        self.shared.cv.notify_all();
    }

    /// Partials run only on GPU with no final job waiting or running (section 8).
    pub fn partials_allowed(&self) -> bool {
        self.shared.partials_ok.load(Ordering::Relaxed)
    }

    /// Final jobs of a session still queued or running.
    pub fn pending_finals(&self, session_id: &str) -> usize {
        let q = self.shared.queue.lock();
        q.finals.iter().filter(|j| j.ctx.session_id == session_id).count()
            + q.in_flight.as_ref().filter(|f| f.session_id == session_id && f.kind == JobKind::Final).map_or(0, |_| 1)
    }

    pub fn queued(&self) -> usize {
        self.shared.queue.lock().finals.len()
    }

    /// Lag = now minus the end time of the oldest waiting utterance.
    pub fn lag_ms(&self, session_id: &str, now_ms: u64) -> u64 {
        let q = self.shared.queue.lock();
        let oldest = q
            .finals
            .iter()
            .filter(|j| j.ctx.session_id == session_id)
            .map(|j| j.end_ms())
            .chain(
                q.in_flight.iter().filter(|f| f.session_id == session_id && f.kind == JobKind::Final).map(|f| f.end_ms),
            )
            .min();
        oldest.map_or(0, |end| now_ms.saturating_sub(end))
    }

    /// Drops every queued job of a session; returns the audio duration dropped.
    pub fn cancel_session(&self, session_id: &str) -> u64 {
        let mut q = self.shared.queue.lock();
        let mut dropped = 0;
        q.finals.retain(|j| {
            if j.ctx.session_id == session_id {
                dropped += j.duration_ms();
                false
            } else {
                true
            }
        });
        q.partials.retain(|_, j| j.ctx.session_id != session_id);
        update_partials_ok(&self.shared, &q);
        dropped
    }

    /// Holds a session's finals in the queue until `false` (FR-25), so the recording runs light;
    /// its partials go too, since nothing is shown until the finals run.
    pub fn set_deferred(&self, session_id: &str, deferred: bool) {
        let mut q = self.shared.queue.lock();
        if deferred {
            q.deferred.insert(session_id.to_string());
            q.partials.retain(|_, j| j.ctx.session_id != session_id);
        } else {
            q.deferred.remove(session_id);
        }
        self.shared.cv.notify_all();
    }

    /// Audio of a session's finals still queued or running, in ms: how much finishing has left.
    pub fn pending_ms(&self, session_id: &str) -> u64 {
        let q = self.shared.queue.lock();
        q.finals.iter().filter(|j| j.ctx.session_id == session_id).map(Job::duration_ms).sum::<u64>()
            + q.in_flight
                .as_ref()
                .filter(|f| f.session_id == session_id && f.kind == JobKind::Final)
                .map_or(0, |f| f.end_ms.saturating_sub(f.start_ms))
    }

    /// Removes queued partials (e.g. when a session stops).
    pub fn drop_partials(&self, session_id: &str) {
        let mut q = self.shared.queue.lock();
        q.partials.retain(|_, j| j.ctx.session_id != session_id);
    }

    /// Start time of the in-flight job, for tests and diagnostics.
    pub fn in_flight_start(&self) -> Option<u64> {
        self.shared.queue.lock().in_flight.as_ref().map(|f| f.start_ms)
    }
}

fn update_partials_ok(shared: &Shared, q: &Queue) {
    let busy_final = q.in_flight.as_ref().is_some_and(|f| f.kind == JobKind::Final);
    let ok = q.engine_ready && q.engine_gpu && q.finals.is_empty() && !busy_final;
    shared.partials_ok.store(ok, Ordering::Relaxed);
}

fn set_status(shared: &Shared, host: &dyn WorkerHost, status: EngineStatus) {
    *shared.status.lock() = status.clone();
    host.status_changed(&status);
}

enum Work {
    Command(Command),
    Finals(Vec<Job>),
    Partial(Job),
    ReleaseStates,
}

/// Picks the oldest final (and, under backlog, merges later finals of the same source),
/// else the newest partial.
fn next_work(q: &mut Queue) -> Option<Work> {
    if let Some(cmd) = q.command.take() {
        return Some(Work::Command(cmd));
    }
    if q.release_states && q.finals.is_empty() && q.partials.is_empty() {
        q.release_states = false;
        return Some(Work::ReleaseStates);
    }
    if !q.engine_ready {
        return None;
    }
    q.finals.sort_by_key(|j| (j.start_ms, j.source.as_str()));
    if let Some(i) = q.finals.iter().position(|j| !q.deferred.contains(&j.ctx.session_id)) {
        let first = q.finals.remove(i);
        let now_end = q
            .finals
            .iter()
            .filter(|j| j.ctx.session_id == first.ctx.session_id)
            .map(|j| j.end_ms())
            .max()
            .unwrap_or(first.end_ms());
        let backlog = now_end.saturating_sub(first.end_ms()) >= BACKLOG_LAG_MS;
        let mut batch = vec![first];
        if backlog {
            let mut total = batch[0].duration_ms();
            let mut i = 0;
            while i < q.finals.len() {
                let j = &q.finals[i];
                if j.source == batch[0].source && j.ctx.session_id == batch[0].ctx.session_id {
                    let add = MERGE_GAP_MS + j.duration_ms();
                    if total + add > MERGE_MAX_MS {
                        break;
                    }
                    total += add;
                    batch.push(q.finals.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        return Some(Work::Finals(batch));
    }
    if q.engine_gpu {
        // Newest partial across sources; the rest are stale by the time it finishes.
        let key = q.partials.iter().max_by_key(|(_, j)| j.end_ms()).map(|(k, _)| *k)?;
        return q.partials.remove(&key).map(Work::Partial);
    }
    q.partials.clear();
    None
}

/// Joins pieces with 200 ms of silence and records where each one landed.
pub fn merge_pieces(pieces: &[Job]) -> (Vec<f32>, Vec<MergedPiece>) {
    let gap = MERGE_GAP_MS as usize * SAMPLE_RATE / 1000;
    let mut audio = Vec::new();
    let mut map = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        if i > 0 {
            audio.extend(std::iter::repeat_n(0.0, gap));
        }
        let offset_ms = (audio.len() * 1000 / SAMPLE_RATE) as u64;
        audio.extend_from_slice(&p.samples);
        map.push(MergedPiece {
            utterance_id: p.utterance_id.clone(),
            offset_ms,
            len_ms: p.duration_ms(),
            start_ms: p.start_ms,
            rms_dbfs: p.rms_dbfs,
        });
    }
    (audio, map)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergedPiece {
    pub utterance_id: String,
    pub offset_ms: u64,
    pub len_ms: u64,
    pub start_ms: u64,
    pub rms_dbfs: f32,
}

/// Maps a segment in merged time back to its piece by its midpoint, clamped to the piece.
pub fn map_to_piece(map: &[MergedPiece], t0: u64, t1: u64) -> Option<(usize, u64, u64)> {
    if map.is_empty() {
        return None;
    }
    let mid = (t0 + t1) / 2;
    let idx = map.iter().position(|p| mid >= p.offset_ms && mid < p.offset_ms + p.len_ms.max(1)).unwrap_or_else(|| {
        // In a silence gap or past the end: the nearest piece.
        map.iter()
            .enumerate()
            .min_by_key(|(_, p)| {
                let end = p.offset_ms + p.len_ms;
                if mid < p.offset_ms { p.offset_ms - mid } else { mid.saturating_sub(end) }
            })
            .map(|(i, _)| i)
            .unwrap()
    });
    let p = &map[idx];
    let rel = |t: u64| t.saturating_sub(p.offset_ms).min(p.len_ms);
    let (s, e) = (p.start_ms + rel(t0), p.start_ms + rel(t1));
    Some((idx, s, e.max(s)))
}

struct Runner {
    engine: Option<Engine>,
    model_id: Option<String>,
    states: HashMap<SourceId, WhisperState>,
}

fn run(shared: Arc<Shared>, host: Arc<dyn WorkerHost>) {
    let mut runner = Runner { engine: None, model_id: None, states: HashMap::new() };
    loop {
        let work = {
            let mut q = shared.queue.lock();
            loop {
                if let Some(w) = next_work(&mut q) {
                    if let Work::Finals(ref b) = w {
                        let (start, end) = (b[0].start_ms, b.last().unwrap().end_ms());
                        q.in_flight = Some(InFlight {
                            session_id: b[0].ctx.session_id.clone(),
                            kind: JobKind::Final,
                            start_ms: start,
                            end_ms: end,
                        });
                    } else if let Work::Partial(ref j) = w {
                        q.in_flight = Some(InFlight {
                            session_id: j.ctx.session_id.clone(),
                            kind: JobKind::Partial,
                            start_ms: j.start_ms,
                            end_ms: j.end_ms(),
                        });
                    }
                    update_partials_ok(&shared, &q);
                    break w;
                }
                shared.cv.wait(&mut q);
            }
        };

        match work {
            Work::Command(Command::Shutdown) => return,
            Work::Command(Command::Unload) => {
                runner.states.clear();
                runner.engine = None;
                runner.model_id = None;
                let mut q = shared.queue.lock();
                q.engine_ready = false;
                q.engine_gpu = false;
                update_partials_ok(&shared, &q);
                drop(q);
                set_status(&shared, host.as_ref(), EngineStatus::default());
            }
            Work::Command(Command::Load(spec)) => {
                runner.states.clear();
                runner.engine = None;
                {
                    let mut q = shared.queue.lock();
                    q.engine_ready = false;
                    update_partials_ok(&shared, &q);
                }
                set_status(
                    &shared,
                    host.as_ref(),
                    EngineStatus {
                        state: EngineState::Loading,
                        model_id: Some(spec.model_id.clone()),
                        ..Default::default()
                    },
                );
                let gpu_attempt =
                    spec.try_gpu && super::engine::gpu_compiled() && super::engine::gpu_device_name().is_some();
                if gpu_attempt {
                    host.gpu_init(&spec.model_id, true);
                }
                let result = Engine::load(&spec.path, spec.try_gpu);
                if gpu_attempt {
                    host.gpu_init(&spec.model_id, false);
                }
                match result {
                    Ok((engine, outcome)) => {
                        if outcome == LoadOutcome::CpuAfterGpuFailure {
                            host.gpu_fallback(&spec.model_id);
                        }
                        let status = EngineStatus {
                            state: EngineState::Ready,
                            gpu: engine.gpu,
                            device_name: engine.device_name.clone(),
                            model_id: Some(spec.model_id.clone()),
                            error: None,
                        };
                        let gpu = engine.gpu;
                        runner.engine = Some(engine);
                        runner.model_id = Some(spec.model_id);
                        {
                            let mut q = shared.queue.lock();
                            q.engine_ready = true;
                            q.engine_gpu = gpu;
                            update_partials_ok(&shared, &q);
                        }
                        set_status(&shared, host.as_ref(), status);
                        shared.cv.notify_all();
                    }
                    Err(err) => {
                        tracing::error!("model load failed: {err:#}");
                        set_status(
                            &shared,
                            host.as_ref(),
                            EngineStatus {
                                state: EngineState::Failed,
                                model_id: Some(spec.model_id),
                                error: Some(format!("{err:#}")),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            Work::Finals(batch) => {
                let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| runner.process_finals(&batch)));
                let retry = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(err)) => {
                        tracing::warn!("ASR job failed: {err:#}");
                        Some(())
                    }
                    Err(_) => {
                        tracing::error!("ASR worker panicked; resetting states");
                        runner.states.clear();
                        Some(())
                    }
                };
                let mut q = shared.queue.lock();
                q.in_flight = None;
                if retry.is_some() {
                    for mut job in batch {
                        if job.retries == 0 {
                            job.retries = 1;
                            q.finals.push(job);
                        } else {
                            tracing::error!("dropping utterance {} after a retry", job.utterance_id);
                            job.ctx.sink.on_final(FinalResult {
                                source: job.source,
                                pieces: vec![PieceInfo {
                                    utterance_id: job.utterance_id.clone(),
                                    start_ms: job.start_ms,
                                    end_ms: job.end_ms(),
                                }],
                                segments: Vec::new(),
                            });
                        }
                    }
                }
                update_partials_ok(&shared, &q);
                shared.cv.notify_all();
            }
            Work::ReleaseStates => {
                if !runner.states.is_empty() {
                    runner.states.clear();
                    tracing::info!("released Whisper states");
                }
            }
            Work::Partial(job) => {
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    if let Err(err) = runner.process_partial(&job) {
                        tracing::debug!("partial failed: {err:#}");
                    }
                }));
                let mut q = shared.queue.lock();
                q.in_flight = None;
                update_partials_ok(&shared, &q);
                shared.cv.notify_all();
            }
        }
    }
}

impl Runner {
    /// Decodes `audio` into the source's state. With `home` = `auto` Whisper picks the language;
    /// otherwise the clip is decoded in `home` (the configured language, or the one 自動 locked)
    /// or in the one its source switched to, and again in another language when Whisper is sure
    /// the clip is in that one (see `language`). The segments are left in the returned state.
    fn decode(
        &mut self,
        ctx: &SessionCtx,
        source: SourceId,
        home: &str,
        audio: &[f32],
        params_for: impl for<'l> Fn(&'l str) -> FullParams<'l, 'static>,
    ) -> anyhow::Result<&mut WhisperState> {
        let engine = self.engine.as_ref().ok_or_else(|| anyhow::anyhow!("no model loaded"))?;
        let state = match self.states.entry(source) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(engine.create_state()?),
        };
        let whisper = |state: &mut WhisperState, lang: &str| {
            state.full(params_for(lang), audio).map_err(|e| anyhow::anyhow!("whisper: {e}"))
        };
        if home == "auto" {
            whisper(state, home)?;
            return Ok(state);
        }
        let tried = ctx.switched.lock().get(&source).cloned().unwrap_or_else(|| home.to_string());
        whisper(state, &tried)?;
        let probs = match engine.languages(state) {
            Ok(Some(probs)) => probs,
            Ok(None) => return Ok(state),
            Err(err) => {
                tracing::debug!("{err:#}");
                return Ok(state);
            }
        };
        let p =
            |lang: &str| whisper_rs::get_lang_id(lang).and_then(|id| probs.get(id as usize)).copied().unwrap_or(0.0);
        let lang = match language::redo_in(home, &tried, p) {
            Some(lang) => {
                tracing::info!(
                    "{}: decoded in {tried}, but Whisper hears {lang} ({:.2}); decoding again in {lang}",
                    source.as_str(),
                    p(&lang)
                );
                whisper(state, &lang)?;
                lang
            }
            None => tried,
        };
        let mut switched = ctx.switched.lock();
        if lang == home {
            switched.remove(&source);
        } else {
            switched.insert(source, lang);
        }
        Ok(state)
    }

    fn process_finals(&mut self, batch: &[Job]) -> anyhow::Result<()> {
        let first = &batch[0];
        let ctx = first.ctx.clone();
        let source = first.source;
        let (audio, map) = merge_pieces(batch);
        if audio.is_empty() {
            return Ok(());
        }
        let duration_ms = (audio.len() * 1000 / SAMPLE_RATE) as u64;
        let home = ctx.home_language();
        let try_lock = home == "auto" && duration_ms >= LANGUAGE_LOCK_MIN_MS;
        let short = batch.len() == 1 && first.speech_ms < SHORT_SPEECH_MS;
        let last = if short { None } else { ctx.last_text.lock().get(&source).cloned() };
        let vocabulary = &ctx.config.vocabulary;
        let engine = self.engine.as_ref().ok_or_else(|| anyhow::anyhow!("no model loaded"))?;
        let opts = DecodeOptions {
            threads: engine.threads(),
            accuracy_first: ctx.config.accuracy_first,
            audio_ctx_experimental: ctx.config.audio_ctx_experimental,
        };
        let state = self.decode(&ctx, source, &home, &audio, |lang| {
            let prompt = if short {
                params::vocabulary_prompt(vocabulary)
            } else {
                params::build_prompt(vocabulary, last.as_deref(), lang)
            };
            params::final_params(&opts, lang, &prompt, audio.len())
        })?;

        if try_lock && let Some(code) = whisper_rs::get_lang_str(state.full_lang_id_from_state()) {
            *ctx.locked_language.lock() = Some(code.to_string());
            ctx.sink.on_language(code);
        }

        let pp = PostProcessor { hallucination_filter: ctx.config.hallucination_filter };
        let mut segments = Vec::new();
        for i in 0..state.full_n_segments() {
            let Some(seg) = state.get_segment(i) else { continue };
            let raw = match seg.to_str_lossy() {
                Ok(t) => t.into_owned(),
                Err(_) => continue,
            };
            let t0 = (seg.start_timestamp().max(0) as u64) * 10;
            let t1 = (seg.end_timestamp().max(0) as u64) * 10;
            let no_speech = seg.no_speech_probability();
            let Some((idx, start, end)) = map_to_piece(&map, t0, t1) else { continue };
            let piece = &map[idx];
            let stats = UtteranceStats { duration_ms: piece.len_ms, rms_dbfs: piece.rms_dbfs };
            match pp.process(&raw, no_speech, stats) {
                Outcome::Keep(text) => segments.push(FinalSegment {
                    utterance_id: piece.utterance_id.clone(),
                    t_start_ms: start,
                    t_end_ms: end,
                    text,
                    no_speech,
                }),
                Outcome::Drop(reason) => {
                    tracing::info!(
                        "dropped segment ({}): {} chars, utterance {}",
                        reason.as_str(),
                        raw.chars().count(),
                        piece.utterance_id
                    );
                }
            }
        }
        if let Some(last) = segments.last() {
            ctx.last_text.lock().insert(source, last.text.clone());
        }
        let pieces = map
            .iter()
            .map(|p| PieceInfo {
                utterance_id: p.utterance_id.clone(),
                start_ms: p.start_ms,
                end_ms: p.start_ms + p.len_ms,
            })
            .collect();
        ctx.sink.on_final(FinalResult { source, pieces, segments });
        Ok(())
    }

    fn process_partial(&mut self, job: &Job) -> anyhow::Result<()> {
        let ctx = job.ctx.clone();
        let home = ctx.home_language();
        let threads = self.engine.as_ref().map(|e| e.threads()).unwrap_or(4);
        let state = self.decode(&ctx, job.source, &home, &job.samples, |lang| params::partial_params(threads, lang))?;
        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i)
                && let Ok(t) = seg.to_str_lossy()
            {
                text.push_str(&t);
            }
        }
        let text = tidy_spaces(&text);
        if !text.is_empty() && !is_symbols_only(&text) {
            ctx.sink.on_partial(job.source, &job.utterance_id, job.start_ms, &text);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullSink;
    impl SegmentSink for NullSink {
        fn on_final(&self, _: FinalResult) {}
        fn on_partial(&self, _: SourceId, _: &str, _: u64, _: &str) {}
        fn on_language(&self, _: &str) {}
    }

    fn ctx() -> Arc<SessionCtx> {
        SessionCtx::new(
            "s1".into(),
            AsrConfig {
                language: Language::Ja,
                vocabulary: vec![],
                accuracy_first: false,
                hallucination_filter: true,
                audio_ctx_experimental: false,
            },
            Arc::new(NullSink),
        )
    }

    fn job(ctx: &Arc<SessionCtx>, source: SourceId, id: &str, start_ms: u64, len_ms: u64) -> Job {
        Job {
            ctx: ctx.clone(),
            source,
            utterance_id: id.into(),
            start_ms,
            samples: vec![0.1; (len_ms as usize) * SAMPLE_RATE / 1000],
            rms_dbfs: -20.0,
            speech_ms: len_ms as u32,
            kind: JobKind::Final,
            retries: 0,
        }
    }

    #[test]
    fn merged_segments_map_back_by_midpoint() {
        let c = ctx();
        let pieces =
            vec![job(&c, SourceId::App, "utt_1", 10_000, 2_000), job(&c, SourceId::App, "utt_2", 30_000, 3_000)];
        let (audio, map) = merge_pieces(&pieces);
        // 2 s + 0.2 s gap + 3 s
        assert_eq!(audio.len(), 5_200 * 16);
        assert_eq!(map[1].offset_ms, 2_200);
        // A segment inside the first piece.
        assert_eq!(map_to_piece(&map, 500, 1_500), Some((0, 10_500, 11_500)));
        // A segment in the second piece.
        assert_eq!(map_to_piece(&map, 2_300, 4_000), Some((1, 30_100, 31_800)));
        // Spanning the gap: the midpoint (2_350) lies in piece 2, start clamps to its start.
        assert_eq!(map_to_piece(&map, 1_800, 2_900), Some((1, 30_000, 30_700)));
        // Midpoint inside the first piece: end clamps to its end.
        assert_eq!(map_to_piece(&map, 1_000, 2_600), Some((0, 11_000, 12_000)));
    }

    #[test]
    fn queue_orders_finals_before_partials_and_by_time() {
        let c = ctx();
        let mut q = Queue { engine_ready: true, engine_gpu: true, ..Default::default() };
        q.finals.push(job(&c, SourceId::Mic, "utt_2", 5_000, 1_000));
        q.finals.push(job(&c, SourceId::App, "utt_1", 1_000, 1_000));
        let mut partial = job(&c, SourceId::App, "utt_3", 9_000, 1_000);
        partial.kind = JobKind::Partial;
        q.partials.insert(SourceId::App, partial);
        let order: Vec<String> = std::iter::from_fn(|| next_work(&mut q))
            .map(|w| match w {
                Work::Finals(b) => b[0].utterance_id.clone(),
                Work::Partial(j) => format!("partial:{}", j.utterance_id),
                Work::Command(_) => "cmd".into(),
                Work::ReleaseStates => "release".into(),
            })
            .collect();
        assert_eq!(order, ["utt_1", "utt_2", "partial:utt_3"]);
    }

    #[test]
    fn backlog_merges_same_source_up_to_25_seconds() {
        let c = ctx();
        let mut q = Queue { engine_ready: true, ..Default::default() };
        for i in 0..6 {
            q.finals.push(job(&c, SourceId::App, &format!("utt_{i}"), i * 6_000, 5_000));
        }
        q.finals.push(job(&c, SourceId::Mic, "mic_1", 2_000, 1_000));
        match next_work(&mut q) {
            Some(Work::Finals(batch)) => {
                let ids: Vec<&str> = batch.iter().map(|j| j.utterance_id.as_str()).collect();
                // 5 + 4×(0.2 + 5) = 25.8 s would exceed 25 s: four pieces fit (20.6 s).
                assert_eq!(ids, ["utt_0", "utt_1", "utt_2", "utt_3"]);
            }
            _ => panic!("expected finals"),
        }
        // The mic job is next by start time; nothing waits behind it on its source.
        match next_work(&mut q) {
            Some(Work::Finals(batch)) => assert_eq!(batch[0].utterance_id, "mic_1"),
            _ => panic!(),
        }
    }

    #[test]
    fn no_merge_without_backlog() {
        let c = ctx();
        let mut q = Queue { engine_ready: true, ..Default::default() };
        q.finals.push(job(&c, SourceId::App, "utt_1", 0, 2_000));
        q.finals.push(job(&c, SourceId::App, "utt_2", 3_000, 2_000));
        match next_work(&mut q) {
            Some(Work::Finals(batch)) => assert_eq!(batch.len(), 1),
            _ => panic!(),
        }
    }

    #[test]
    fn a_deferred_session_waits_in_the_queue_until_released() {
        let c = ctx();
        let mut q = Queue { engine_ready: true, engine_gpu: true, ..Default::default() };
        q.deferred.insert(c.session_id.clone());
        q.finals.push(job(&c, SourceId::App, "utt_1", 0, 2_000));
        assert!(next_work(&mut q).is_none());
        assert_eq!(q.finals.len(), 1);
        q.deferred.clear();
        assert!(matches!(next_work(&mut q), Some(Work::Finals(_))));
    }

    #[test]
    fn partials_dropped_on_cpu() {
        let c = ctx();
        let mut q = Queue { engine_ready: true, engine_gpu: false, ..Default::default() };
        let mut partial = job(&c, SourceId::App, "utt_3", 9_000, 1_000);
        partial.kind = JobKind::Partial;
        q.partials.insert(SourceId::App, partial);
        assert!(next_work(&mut q).is_none());
        assert!(q.partials.is_empty());
    }

    #[test]
    fn states_are_released_only_after_the_queue_drains() {
        let c = ctx();
        let mut q = Queue { engine_ready: true, release_states: true, ..Default::default() };
        q.finals.push(job(&c, SourceId::App, "utt_1", 0, 2_000));
        assert!(matches!(next_work(&mut q), Some(Work::Finals(_))));
        assert!(matches!(next_work(&mut q), Some(Work::ReleaseStates)));
        assert!(next_work(&mut q).is_none());
    }

    #[test]
    fn nothing_runs_until_the_model_is_ready() {
        let c = ctx();
        let mut q = Queue::default();
        q.finals.push(job(&c, SourceId::App, "utt_1", 0, 2_000));
        assert!(next_work(&mut q).is_none());
        assert_eq!(q.finals.len(), 1);
    }

    #[test]
    fn the_current_line_is_what_is_being_said_else_the_last_said() {
        let spoken = |open: Option<(&str, u64)>, last: Option<(&str, u64)>| Speaking {
            open: open.map(|(id, t)| (id.to_string(), t)),
            last: last.map(|(id, t)| (id.to_string(), t)),
        };
        let mut speaking = HashMap::new();
        assert!(current_utterances(&speaking, 1_000).is_empty());
        speaking.insert(SourceId::App, spoken(None, Some(("utt_000001", 4_000))));
        speaking.insert(SourceId::Mic, spoken(None, Some(("utt_000002", 6_000))));
        // Nobody speaking: the line that ended last.
        assert_eq!(current_utterances(&speaking, 7_000), ["utt_000002"]);
        // Someone speaking: that line, even before its text exists.
        speaking.insert(SourceId::App, spoken(Some(("utt_000003", 6_500)), Some(("utt_000001", 4_000))));
        assert_eq!(current_utterances(&speaking, 7_000), ["utt_000003"]);
        // Both speaking: both lines, earlier first.
        speaking.insert(SourceId::Mic, spoken(Some(("utt_000004", 6_800)), None));
        assert_eq!(current_utterances(&speaking, 7_000), ["utt_000003", "utt_000004"]);
    }
}
