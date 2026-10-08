//! Integration harness (section 18): plays fixture WAVs through the real pipeline
//! (FileSource → DSP → VAD → segmenter → ASR worker → session store) and compares the result
//! with a one-pass transcription by the same model.
//!
//! Ignored by default. To run:
//!   uv run tests/fixtures/build_fixtures.py
//!   cargo test --release --test pipeline -- --ignored --nocapture --test-threads 1
//!
//! Environment:
//!   KIKITORI_TEST_MODEL    model file (default: base-q5 in %LOCALAPPDATA%\dev.yusai.kikitori\models)
//!   KIKITORI_TEST_SPEED    playback speed; 1 = real time (default), needed for latency figures
//!   KIKITORI_TEST_CPU=1    force CPU
//!   KIKITORI_ASSERT_LATENCY=1  also assert the section 16 latency targets (reference GPU machine)
//!   KIKITORI_CER_MARGIN    allowed streaming-vs-one-pass CER gap (default 0.03, section 18)
//!   KIKITORI_REQUIRE_FIXTURES=1  fail instead of skipping when the model or fixtures are missing

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use unicode_normalization::UnicodeNormalization;

use kikitori_lib::asr::engine::Engine;
use kikitori_lib::asr::oneshot;
use kikitori_lib::asr::params::{self, DecodeOptions};
use kikitori_lib::asr::worker::{
    AsrConfig, AsrWorker, EngineStatus, FinalResult, ModelSpec, SegmentSink, SessionCtx, WorkerHost,
};
use kikitori_lib::audio::dsp::{self, DspParams, JobSink, Levels, SessionIds};
use kikitori_lib::audio::file_source::{FileSource, read_wav};
use kikitori_lib::audio::source::AudioSource;
use kikitori_lib::events::EventSink;
use kikitori_lib::platform;
use kikitori_lib::segmenter::SegmenterConfig;
use kikitori_lib::session::log::{LogEvent, SessionLog};
use kikitori_lib::session::model::{ModelRef, Session, SourceId, SourceInfo};
use kikitori_lib::session::store::SessionStore;
use kikitori_lib::settings::Language;

struct NullHost;
impl WorkerHost for NullHost {
    fn status_changed(&self, _: &EngineStatus) {}
    fn gpu_init(&self, _: &str, _: bool) {}
    fn gpu_fallback(&self, _: &str) {}
}

struct NullEvents;
impl EventSink for NullEvents {
    fn emit_json(&self, _: &str, _: serde_json::Value) {}
}

/// Forwards to the session store and records when each utterance's text arrived.
struct TimingSink {
    store: Arc<SessionStore>,
    started: Instant,
    /// (utterance end in session ms, wall-clock ms since start when its text arrived)
    arrivals: Mutex<Vec<(u64, u64)>>,
    /// Every partial text, in order.
    partials: Mutex<Vec<String>>,
}

impl SegmentSink for TimingSink {
    fn on_final(&self, result: FinalResult) {
        let now = self.started.elapsed().as_millis() as u64;
        {
            let mut a = self.arrivals.lock();
            for p in &result.pieces {
                if result.segments.iter().any(|s| s.utterance_id == p.utterance_id) {
                    a.push((p.end_ms, now));
                }
            }
        }
        self.store.on_final(result);
    }
    fn on_partial(&self, s: SourceId, u: &str, start: u64, t: &str) {
        self.partials.lock().push(t.to_string());
        self.store.on_partial(s, u, start, t);
    }
    fn on_language(&self, l: &str) {
        self.store.on_language(l);
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("tests").join("fixtures").join("generated")
}

fn model_path() -> PathBuf {
    std::env::var("KIKITORI_TEST_MODEL").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
            .join("dev.yusai.kikitori")
            .join("models")
            .join("ggml-base-q5_1.bin")
    })
}

fn speed() -> f32 {
    std::env::var("KIKITORI_TEST_SPEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0)
}

fn use_gpu() -> bool {
    std::env::var("KIKITORI_TEST_CPU").as_deref() != Ok("1")
}

/// NFKC, punctuation and whitespace removed.
fn normalize(s: &str) -> Vec<char> {
    s.nfkc().filter(|c| c.is_alphanumeric()).collect()
}

/// Character error rate: edit distance / reference length.
fn cer(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    let mut cur = vec![0; h.len() + 1];
    for i in 1..=r.len() {
        cur[0] = i;
        for j in 1..=h.len() {
            let sub = prev[j - 1] + usize::from(r[i - 1] != h[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[h.len()] as f64 / r.len() as f64
}

fn percentile(values: &mut [u64], p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let idx = ((values.len() - 1) as f64 * p).round() as usize;
    values[idx]
}

/// One-pass baseline: the whole file in one call, as a file transcriber would.
fn one_pass(engine: &Engine, samples: &[f32]) -> String {
    let mut state = engine.create_state().unwrap();
    let opts = DecodeOptions { threads: engine.threads(), accuracy_first: false, audio_ctx_experimental: false };
    let mut p = params::final_params(&opts, "ja", params::PRIMER, samples.len());
    p.set_no_context(false);
    state.full(p, samples).unwrap();
    let mut text = String::new();
    for i in 0..state.full_n_segments() {
        text.push_str(&state.get_segment(i).unwrap().to_str_lossy().unwrap());
    }
    text
}

struct RunResult {
    session: Session,
    latencies: Vec<u64>,
    partials: Vec<String>,
    wall: Duration,
}

/// Streams a WAV through the pipeline. `channels` maps file channels to sources; `partials`
/// asks for partial text, which the worker writes only on GPU.
fn stream(
    worker: &Arc<AsrWorker>,
    wav: &Path,
    channels: &[(Option<usize>, SourceId, &str)],
    partials: bool,
) -> RunResult {
    let dir = tempfile::tempdir().unwrap();
    let sources: Vec<SourceInfo> = channels
        .iter()
        .map(|(_, id, label)| SourceInfo { id: *id, label: label.to_string(), exe: None, device: None, name: None })
        .collect();
    let session = Session {
        v: 1,
        id: format!("it-{}", ulid::Ulid::generate()),
        title: "integration".into(),
        started_at: chrono::Local::now().to_rfc3339(),
        ended_at: None,
        duration_ms: 0,
        sources: sources.clone(),
        model: ModelRef::default(),
        language: "ja".into(),
        gpu: false,
        items: vec![],
        unprocessed_ms: 0,
        project: None,
        continued: Vec::new(),
        audio: Vec::new(),
    };
    let log = SessionLog::create(dir.path()).unwrap();
    log.append(&LogEvent::SessionStarted {
        id: session.id.clone(),
        title: session.title.clone(),
        started_at: session.started_at.clone(),
        sources,
        model: ModelRef::default(),
        language: "ja".into(),
        gpu: false,
        app_version: "test".into(),
    })
    .unwrap();
    let ids = Arc::new(SessionIds::default());
    let store = Arc::new(SessionStore::new(dir.path(), log, session.clone(), ids.clone(), Arc::new(NullEvents), true));
    let started = Instant::now();
    let sink = Arc::new(TimingSink {
        store: store.clone(),
        started,
        arrivals: Mutex::new(Vec::new()),
        partials: Mutex::new(Vec::new()),
    });
    let ctx = SessionCtx::new(
        session.id.clone(),
        AsrConfig {
            language: Language::Ja,
            vocabulary: vec![],
            accuracy_first: false,
            hallucination_filter: true,
            audio_ctx_experimental: false,
        },
        sink.clone(),
    );
    let t0 = platform::qpc_now_100ns();
    let levels = Arc::new(Levels::default());
    let job_sink: Arc<dyn JobSink> = worker.clone();
    let mut running = Vec::new();
    for (channel, id, _) in channels {
        let (tx, rx) = crossbeam_channel::bounded(400);
        let params = DspParams {
            source: *id,
            t0_100ns: t0 as i64,
            segmenter: SegmenterConfig::default(),
            partials,
            pad_quiet_stream: false,
            start_ms: 0,
            audio: None,
        };
        let dsp = dsp::spawn(rx, params, ctx.clone(), job_sink.clone(), ids.clone(), levels.clone());
        let mut src = FileSource::new(*id, wav.to_path_buf(), *channel, t0, speed());
        src.start(tx).unwrap();
        running.push((src, dsp));
    }
    for (mut src, dsp) in running {
        src.join();
        drop(src);
        dsp.join().unwrap();
    }
    while worker.pending_finals(&session.id) > 0 {
        std::thread::sleep(Duration::from_millis(50));
    }
    let wall = started.elapsed();
    let latencies = sink.arrivals.lock().iter().map(|(end, at)| at.saturating_sub(*end)).collect();
    let partials = sink.partials.lock().clone();
    RunResult { session: store.snapshot(), latencies, partials, wall }
}

fn text_of(session: &Session, source: SourceId, until_ms: Option<u64>) -> String {
    session
        .ordered()
        .into_iter()
        .filter_map(|i| match i {
            kikitori_lib::session::model::TimelineItem::Segment(s)
                if s.source == source && until_ms.is_none_or(|u| s.t_start_ms < u) =>
            {
                Some(s.text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn setup() -> Option<(Arc<AsrWorker>, Engine)> {
    let model = model_path();
    if !model.exists() || !fixtures().join("meeting.wav").exists() {
        let msg = format!("need {} and generated fixtures (uv run tests/fixtures/build_fixtures.py)", model.display());
        assert!(std::env::var("KIKITORI_REQUIRE_FIXTURES").as_deref() != Ok("1"), "{msg}");
        eprintln!("skipping: {msg}");
        return None;
    }
    kikitori_lib::asr::engine::disable_vulkan_implicit_layers();
    let worker = AsrWorker::spawn(Arc::new(NullHost));
    worker.load_model(ModelSpec { path: model.clone(), model_id: "test".into(), try_gpu: use_gpu() });
    std::thread::sleep(Duration::from_millis(100));
    assert!(oneshot::wait_ready(&worker, Duration::from_secs(300)), "model failed to load");
    let (engine, outcome) = Engine::load(&model, use_gpu()).unwrap();
    eprintln!("model {} on {:?} ({:?})", model.display(), outcome, engine.device_name);
    Some((worker, engine))
}

#[test]
#[ignore = "needs a model and generated fixtures"]
fn meeting_streaming_matches_one_pass() {
    let Some((worker, engine)) = setup() else { return };
    let wav = fixtures().join("meeting.wav");
    let reference = std::fs::read_to_string(fixtures().join("meeting.ref.txt")).unwrap();
    let (samples, _, _) = read_wav(&wav).unwrap();

    let t = Instant::now();
    let baseline = one_pass(&engine, &samples);
    let baseline_secs = t.elapsed().as_secs_f64();
    let run = stream(&worker, &wav, &[(None, SourceId::App, "相手")], false);
    let streamed = text_of(&run.session, SourceId::App, None);

    let base_cer = cer(&reference, &baseline);
    let stream_cer = cer(&reference, &streamed);
    let mut lat = run.latencies.clone();
    let (p50, p95) = (percentile(&mut lat, 0.5), percentile(&mut lat, 0.95));
    eprintln!(
        "one-pass CER {:.1}% ({baseline_secs:.1}s) | streaming CER {:.1}% | segments {} | wall {:.1}s at {}x",
        base_cer * 100.0,
        stream_cer * 100.0,
        run.session.segments().count(),
        run.wall.as_secs_f64(),
        speed()
    );
    eprintln!("latency end-of-utterance → text: p50 {p50} ms, p95 {p95} ms over {} utterances", run.latencies.len());
    eprintln!("streamed: {streamed}");

    let margin: f64 = std::env::var("KIKITORI_CER_MARGIN").ok().and_then(|s| s.parse().ok()).unwrap_or(0.03);
    assert!(
        stream_cer <= base_cer + margin,
        "streaming CER {stream_cer:.3} more than {margin} worse than one-pass {base_cer:.3}"
    );
    if std::env::var("KIKITORI_ASSERT_LATENCY").as_deref() == Ok("1") && speed() == 1.0 {
        assert!(p50 <= 2000 && p95 <= 4000, "latency p50 {p50} ms / p95 {p95} ms misses the section 16 targets");
    }
    worker.shutdown();
}

#[test]
#[ignore = "needs a model and generated fixtures"]
fn two_channel_labels_and_echo_guard() {
    let Some((worker, _engine)) = setup() else { return };
    let wav = fixtures().join("two_channel.wav");
    let ref_others = std::fs::read_to_string(fixtures().join("two_channel.others.ref.txt")).unwrap();
    let ref_me = std::fs::read_to_string(fixtures().join("two_channel.me.ref.txt")).unwrap();
    // Right channel = app (相手), left = mic (自分).
    let run = stream(&worker, &wav, &[(Some(1), SourceId::App, "相手"), (Some(0), SourceId::Mic, "自分")], false);

    let others = text_of(&run.session, SourceId::App, None);
    let me = text_of(&run.session, SourceId::Mic, None);
    // The right channel speaks for the first ~106 s; mic lines then can only be echo.
    let echo_left = text_of(&run.session, SourceId::Mic, Some(106_000));
    let others_cer = cer(&ref_others, &others);
    let me_cer = cer(&ref_me, &me);
    eprintln!(
        "相手 CER {:.1}% | 自分 CER {:.1}% | echo characters left {}",
        others_cer * 100.0,
        me_cer * 100.0,
        normalize(&echo_left).len()
    );

    // Show what survived, next to the 相手 lines around it.
    for item in run.session.ordered() {
        if let kikitori_lib::session::model::TimelineItem::Segment(seg) = item
            && seg.t_start_ms < 106_000
        {
            eprintln!("{:>7}-{:>7} {:?}: {}", seg.t_start_ms, seg.t_end_ms, seg.source, seg.text);
        }
    }
    assert!(others_cer < 0.35, "相手 transcript unexpectedly poor: {others_cer:.3}");
    assert!(me_cer < 0.35, "自分 transcript unexpectedly poor: {me_cer:.3}");
    let other_chars = normalize(&others).len().max(1);
    assert!(normalize(&echo_left).len() * 10 <= other_chars, "echo guard left too much: {echo_left}");
    worker.shutdown();
}

/// Kana, kanji and Japanese punctuation: what English speech translated into Japanese is made of.
fn japanese_chars(s: &str) -> usize {
    s.chars().filter(|c| matches!(*c as u32, 0x3000..=0x30FF | 0x4E00..=0x9FFF | 0xFF66..=0xFF9F)).count()
}

#[test]
#[ignore = "needs a model and generated fixtures"]
fn english_is_not_translated_while_japanese_is_set() {
    let Some((worker, _engine)) = setup() else { return };
    let wav = fixtures().join("english.wav");
    if !wav.exists() {
        let msg = format!("need {} (uv run tests/fixtures/build_fixtures.py)", wav.display());
        assert!(std::env::var("KIKITORI_REQUIRE_FIXTURES").as_deref() != Ok("1"), "{msg}");
        eprintln!("skipping: {msg}");
        return;
    }
    // `stream` decodes with 日本語 set, the app's default.
    let run = stream(&worker, &wav, &[(None, SourceId::App, "相手")], true);
    let text = text_of(&run.session, SourceId::App, None);
    eprintln!("final: {text}");
    eprintln!("{} partials", run.partials.len());
    for p in &run.partials {
        eprintln!("partial: {p}");
    }
    let latin = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    assert!(latin > 300, "too little English came out: {text}");
    assert_eq!(japanese_chars(&text), 0, "final text in Japanese: {text}");
    let translated: Vec<&String> = run.partials.iter().filter(|p| japanese_chars(p) > 0).collect();
    assert!(translated.is_empty(), "partial text in Japanese: {translated:?}");
    worker.shutdown();
}

#[test]
fn cer_metric() {
    assert_eq!(cer("こんにちは。", "こんにちは"), 0.0);
    assert!((cer("あいうえお", "あいうえか") - 0.2).abs() < 1e-9);
    assert_eq!(cer("ＡＢ", "ab".to_uppercase().as_str()), 0.0);
}
