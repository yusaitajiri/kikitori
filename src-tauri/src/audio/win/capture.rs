//! WASAPI capture for the mic, system loopback and per-app loopback (section 7).
//!
//! One thread per source: COM MTA, MMCSS "Pro Audio", event-driven shared mode with 20 ms
//! buffers, and every packet stamped with its QPC position.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, bounded};
use wasapi::{
    AudioCaptureClient, AudioClient, DeviceEnumerator, DeviceEventCallbacks, Direction, Handle, Role, SampleType,
    StreamMode, WasapiError, WaveFormat,
};

use super::process;
use crate::audio::source::{AudioSource, RawChunk, SourceStatus};
use crate::platform;
use crate::session::model::SourceId;

const BUFFER_100NS: i64 = 200_000; // 20 ms
const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
const NO_PACKETS_RESTART: Duration = Duration::from_secs(5);
const APP_SILENT_AFTER: Duration = Duration::from_secs(30);
const DEFAULT_CHANGE_SETTLE: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureTarget {
    /// `None` is the Windows default communications device.
    Mic {
        device_id: Option<String>,
    },
    System,
    App {
        root_pid: u32,
        exe: PathBuf,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("microphone access is turned off in Windows privacy settings")]
    MicDenied,
    #[error("{0}")]
    Other(String),
}

fn classify(err: &WasapiError) -> CaptureError {
    if let WasapiError::Windows(e) = err
        && e.code().0 == E_ACCESSDENIED
    {
        return CaptureError::MicDenied;
    }
    CaptureError::Other(err.to_string())
}

pub struct WasapiSource {
    id: SourceId,
    target: CaptureTarget,
    status: Sender<SourceStatus>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WasapiSource {
    pub fn new(id: SourceId, target: CaptureTarget, status: Sender<SourceStatus>) -> Self {
        Self { id, target, status, stop: Arc::new(AtomicBool::new(false)), thread: None }
    }
}

impl AudioSource for WasapiSource {
    fn id(&self) -> SourceId {
        self.id
    }

    fn start(&mut self, tx: Sender<RawChunk>) -> anyhow::Result<()> {
        let (init_tx, init_rx) = bounded::<Result<(), CaptureError>>(1);
        let ctx = ThreadCtx {
            id: self.id,
            target: self.target.clone(),
            status: self.status.clone(),
            stop: self.stop.clone(),
            tx,
        };
        let handle = std::thread::Builder::new()
            .name(format!("capture-{}", self.id.as_str()))
            .spawn(move || capture_thread(ctx, init_tx))?;
        match init_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(())) => {
                self.thread = Some(handle);
                Ok(())
            }
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err.into())
            }
            Err(_) => {
                self.stop.store(true, Ordering::SeqCst);
                Err(anyhow::anyhow!("timed out opening the {} stream", self.id.as_str()))
            }
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WasapiSource {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ThreadCtx {
    id: SourceId,
    target: CaptureTarget,
    status: Sender<SourceStatus>,
    stop: Arc<AtomicBool>,
    tx: Sender<RawChunk>,
}

#[derive(Debug, Clone, Copy)]
enum SampleKind {
    F32,
    F64,
    I16,
    I24,
    I32,
}

#[derive(Debug, Clone, Copy)]
struct Fmt {
    rate: u32,
    channels: u16,
    kind: SampleKind,
    block_align: usize,
}

impl Fmt {
    fn from_wave(w: &WaveFormat) -> Result<Self, CaptureError> {
        let bits = w.get_bitspersample();
        let kind = match (w.get_subformat().map_err(|e| classify(&e))?, bits) {
            (SampleType::Float, 32) => SampleKind::F32,
            (SampleType::Float, 64) => SampleKind::F64,
            (SampleType::Int, 16) => SampleKind::I16,
            (SampleType::Int, 24) => SampleKind::I24,
            (SampleType::Int, 32) => SampleKind::I32,
            (t, b) => return Err(CaptureError::Other(format!("unsupported sample format {t} {b}-bit"))),
        };
        Ok(Self {
            rate: w.get_samplespersec(),
            channels: w.get_nchannels(),
            kind,
            block_align: w.get_blockalign() as usize,
        })
    }

    fn convert(&self, bytes: &[u8]) -> Vec<f32> {
        match self.kind {
            SampleKind::F32 => bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect(),
            SampleKind::F64 => bytes.as_chunks::<8>().0.iter().map(|b| f64::from_le_bytes(*b) as f32).collect(),
            SampleKind::I16 => {
                bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32_768.0).collect()
            }
            SampleKind::I24 => bytes
                .as_chunks::<3>()
                .0
                .iter()
                .map(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0)
                .collect(),
            SampleKind::I32 => {
                bytes.as_chunks::<4>().0.iter().map(|b| i32::from_le_bytes(*b) as f32 / 2_147_483_648.0).collect()
            }
        }
    }
}

struct Stream {
    client: AudioClient,
    capture: AudioCaptureClient,
    event: Handle,
    fmt: Fmt,
    device: Option<String>,
    device_id: Option<String>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.client.stop_stream();
    }
}

fn shared_mode() -> StreamMode {
    StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: BUFFER_100NS }
}

fn open_device_stream(target: &CaptureTarget) -> Result<Stream, CaptureError> {
    let map = |e: WasapiError| classify(&e);
    let enumerator = DeviceEnumerator::new().map_err(map)?;
    let device = match target {
        CaptureTarget::Mic { device_id: Some(id) } => enumerator.get_device(id).map_err(map)?,
        CaptureTarget::Mic { device_id: None } => {
            enumerator.get_default_device_for_role(&Direction::Capture, &Role::Communications).map_err(map)?
        }
        CaptureTarget::System => enumerator.get_default_device(&Direction::Render).map_err(map)?,
        CaptureTarget::App { .. } => unreachable!("app streams use open_app_stream"),
    };
    let name = device.get_friendlyname().ok();
    let id = device.get_id().ok();
    let mut client = device.get_iaudioclient().map_err(map)?;
    let format = client.get_mixformat().map_err(map)?;
    // On a render device, Direction::Capture means loopback.
    client.initialize_client(&format, &Direction::Capture, &shared_mode()).map_err(map)?;
    let mut stream = finish_open(client, Fmt::from_wave(&format)?, name)?;
    stream.device_id = id;
    Ok(stream)
}

/// ID of the device a default-following stream would open right now.
fn current_default_id(target: &CaptureTarget) -> Option<String> {
    let enumerator = DeviceEnumerator::new().ok()?;
    let device = match target {
        CaptureTarget::Mic { device_id: None } => {
            enumerator.get_default_device_for_role(&Direction::Capture, &Role::Communications).ok()?
        }
        CaptureTarget::System => enumerator.get_default_device(&Direction::Render).ok()?,
        _ => return None,
    };
    device.get_id().ok()
}

/// Process loopback: never ask for the mix format or period (they fail in this mode).
/// Request 48 kHz stereo float, else 16-bit PCM at the same rate.
fn open_app_stream(root_pid: u32) -> Result<Stream, CaptureError> {
    let map = |e: WasapiError| classify(&e);
    let candidates = [
        WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None),
        WaveFormat::new(16, 16, &SampleType::Int, 48_000, 2, None),
    ];
    let mut last_err = None;
    for format in candidates {
        let mut client = AudioClient::new_application_loopback_client(root_pid, true).map_err(map)?;
        match client.initialize_client(&format, &Direction::Capture, &shared_mode()) {
            Ok(()) => return finish_open(client, Fmt::from_wave(&format)?, None),
            Err(e) => last_err = Some(classify(&e)),
        }
    }
    Err(last_err.unwrap_or_else(|| CaptureError::Other("app loopback failed".into())))
}

fn finish_open(client: AudioClient, fmt: Fmt, device: Option<String>) -> Result<Stream, CaptureError> {
    let map = |e: WasapiError| classify(&e);
    let event = client.set_get_eventhandle().map_err(map)?;
    let capture = client.get_audiocaptureclient().map_err(map)?;
    client.start_stream().map_err(map)?;
    Ok(Stream { client, capture, event, fmt, device, device_id: None })
}

fn open(target: &CaptureTarget, app_pid: u32) -> Result<Stream, CaptureError> {
    match target {
        CaptureTarget::App { .. } => open_app_stream(app_pid),
        other => open_device_stream(other),
    }
}

enum ReadOutcome {
    Ok {
        packets: usize,
        any_sound: bool,
    },
    /// The device went away; reopen.
    Reopen,
    /// The DSP side hung up.
    Closed,
}

fn read_packets(stream: &Stream, ctx: &ThreadCtx) -> ReadOutcome {
    let mut packets = 0;
    let mut any_sound = false;
    loop {
        let frames = match stream.capture.get_next_packet_size() {
            Ok(Some(n)) => n as usize,
            Ok(None) => 0,
            Err(e) => {
                tracing::warn!("{}: packet size failed: {e}", ctx.id.as_str());
                return ReadOutcome::Reopen;
            }
        };
        if frames == 0 {
            return ReadOutcome::Ok { packets, any_sound };
        }
        let mut buf = vec![0u8; frames * stream.fmt.block_align];
        let (got, info) = match stream.capture.read_from_device(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("{}: read failed: {e}", ctx.id.as_str());
                return ReadOutcome::Reopen;
            }
        };
        let got = got as usize;
        if got == 0 {
            continue;
        }
        let samples = stream.fmt.convert(&buf[..got * stream.fmt.block_align]);
        let silent = info.flags.silent;
        if !silent && samples.iter().any(|s| s.abs() > 1e-5) {
            any_sound = true;
        }
        let qpc = if info.timestamp == 0 || info.flags.timestamp_error {
            platform::qpc_now_100ns().saturating_sub(got as u64 * 10_000_000 / stream.fmt.rate as u64)
        } else {
            info.timestamp
        };
        let chunk = RawChunk {
            source: ctx.id,
            qpc_100ns: qpc,
            rate: stream.fmt.rate,
            channels: stream.fmt.channels,
            samples,
            silent_flag: silent,
        };
        if ctx.tx.send(chunk).is_err() {
            return ReadOutcome::Closed;
        }
        packets += 1;
    }
}

/// Peak of the app's audio sessions on any render device (for the silence health check).
fn app_session_peak(root_pid: u32) -> Option<f32> {
    let procs = process::snapshot();
    let enumerator = DeviceEnumerator::new().ok()?;
    let devices = enumerator.get_device_collection(&Direction::Render).ok()?;
    let mut peak: Option<f32> = None;
    for i in 0..devices.get_nbr_devices().ok()? {
        let Ok(device) = devices.get_device_at_index(i) else { continue };
        let Ok(manager) = device.get_iaudiosessionmanager() else { continue };
        let Ok(sessions) = manager.get_audiosessionenumerator() else { continue };
        for s in 0..sessions.get_count().unwrap_or(0) {
            let Ok(control) = sessions.get_session(s) else { continue };
            let Ok(pid) = control.get_process_id() else { continue };
            if process::root_pid(&procs, pid) != root_pid {
                continue;
            }
            if let Ok(meter) = control.get_audiometerinformation()
                && let Ok(v) = meter.get_peak_value()
            {
                peak = Some(peak.unwrap_or(0.0).max(v));
            }
        }
    }
    peak
}

/// Polls for a running root process with this executable and opens its loopback stream.
fn wait_for_app(ctx: &ThreadCtx, exe: &std::path::Path) -> Option<(u32, Stream)> {
    loop {
        if ctx.stop.load(Ordering::SeqCst) {
            return None;
        }
        let procs = process::snapshot();
        if let Some(pid) = process::find_root_by_exe(&procs, exe)
            && let Ok(s) = open_app_stream(pid)
        {
            return Some((pid, s));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn capture_thread(ctx: ThreadCtx, init: Sender<Result<(), CaptureError>>) {
    let hr = wasapi::initialize_mta();
    if hr.is_err() {
        tracing::warn!("CoInitializeEx: {hr:?}");
    }
    let mmcss = platform::raise_to_pro_audio();
    // Every COM object lives inside `capture_body`, so all are released before COM shuts down.
    capture_body(&ctx, init);
    drop(mmcss);
    wasapi::deinitialize();
}

fn capture_body(ctx: &ThreadCtx, init: Sender<Result<(), CaptureError>>) {
    let (mut app_pid, app_exe) = match &ctx.target {
        CaptureTarget::App { root_pid, exe } => (*root_pid, Some(exe.clone())),
        _ => (0, None),
    };

    // Default-device changes (FR-15) for streams that follow the default.
    let device_changed = Arc::new(AtomicBool::new(false));
    let _registration = {
        let follow = match &ctx.target {
            CaptureTarget::Mic { device_id: None } => Some((Direction::Capture, Role::Communications)),
            CaptureTarget::System => Some((Direction::Render, Role::Console)),
            _ => None,
        };
        follow.and_then(|(dir, role)| {
            let flag = device_changed.clone();
            let mut callbacks = DeviceEventCallbacks::new();
            callbacks.set_default_device_callback(move |d, r, _id| {
                if d == dir && r == role {
                    flag.store(true, Ordering::SeqCst);
                }
            });
            let enumerator = DeviceEnumerator::new().ok()?;
            enumerator.register_notification_callback(callbacks).ok()
        })
    };

    let app_waiting =
        matches!(ctx.target, CaptureTarget::App { .. }) && (app_pid == 0 || !platform::process_alive(app_pid));
    let mut stream = if let (true, Some(exe)) = (app_waiting, &app_exe) {
        // The app is not running yet: recording starts now and audio joins when it appears.
        let _ = init.send(Ok(()));
        let _ = ctx.status.send(SourceStatus::Lost { source: ctx.id });
        match wait_for_app(ctx, exe) {
            Some((pid, s)) => {
                app_pid = pid;
                let _ = ctx.status.send(SourceStatus::Reattached { source: ctx.id, detail: format!("pid {pid}") });
                s
            }
            None => return,
        }
    } else {
        match open(&ctx.target, app_pid) {
            Ok(s) => {
                let _ = init.send(Ok(()));
                s
            }
            Err(err) => {
                if matches!(err, CaptureError::MicDenied) {
                    let _ = ctx.status.send(SourceStatus::MicDenied);
                }
                let _ = init.send(Err(err));
                return;
            }
        }
    };
    let _ = ctx.status.send(SourceStatus::Started {
        source: ctx.id,
        rate: stream.fmt.rate,
        channels: stream.fmt.channels,
        device: stream.device.clone(),
    });

    let mut last_packet = Instant::now();
    let mut last_sound = Instant::now();
    let mut last_check = Instant::now();
    // Bluetooth headsets flap the default while switching profiles: let it settle first.
    let mut change_pending: Option<Instant> = None;
    let mut restarted_for_silence = false;
    let mut silent_reported = false;

    'outer: while !ctx.stop.load(Ordering::SeqCst) {
        let _ = stream.event.wait_for_event(100);
        let mut reopen = false;
        match read_packets(&stream, ctx) {
            ReadOutcome::Ok { packets, any_sound } => {
                if packets > 0 {
                    last_packet = Instant::now();
                    restarted_for_silence = false;
                }
                if any_sound {
                    last_sound = Instant::now();
                    silent_reported = false;
                }
            }
            ReadOutcome::Reopen => reopen = true,
            ReadOutcome::Closed => break,
        }

        if last_check.elapsed() >= Duration::from_secs(1) {
            last_check = Instant::now();
            if device_changed.swap(false, Ordering::SeqCst) {
                change_pending = Some(Instant::now());
            }
            if change_pending.is_some_and(|t| t.elapsed() >= DEFAULT_CHANGE_SETTLE) {
                change_pending = None;
                let now_default = current_default_id(&ctx.target);
                if now_default.is_some() && now_default != stream.device_id {
                    tracing::info!("{}: default device changed, reopening", ctx.id.as_str());
                    reopen = true;
                }
            }
            // FR-14: the app exited; wait for the same executable to come back.
            if let (Some(exe), CaptureTarget::App { .. }) = (&app_exe, &ctx.target)
                && !platform::process_alive(app_pid)
            {
                let _ = ctx.status.send(SourceStatus::Lost { source: ctx.id });
                drop(stream);
                match wait_for_app(ctx, exe) {
                    Some((pid, s)) => {
                        app_pid = pid;
                        stream = s;
                        let _ =
                            ctx.status.send(SourceStatus::Reattached { source: ctx.id, detail: format!("pid {pid}") });
                        last_packet = Instant::now();
                        last_sound = Instant::now();
                    }
                    None => break 'outer,
                }
                continue;
            }
            // System loopback is legitimately packet-free while nothing plays.
            let expects_packets = !matches!(ctx.target, CaptureTarget::System);
            if expects_packets && last_packet.elapsed() >= NO_PACKETS_RESTART {
                if !restarted_for_silence {
                    tracing::warn!("{}: no packets for 5 s, restarting once", ctx.id.as_str());
                    restarted_for_silence = true;
                    last_packet = Instant::now();
                    reopen = true;
                } else if matches!(ctx.target, CaptureTarget::Mic { .. }) {
                    let _ = ctx.status.send(SourceStatus::Failed {
                        source: ctx.id,
                        message: "マイクから音声が届いていません".into(),
                    });
                    last_packet = Instant::now();
                }
            }
            if matches!(ctx.target, CaptureTarget::App { .. })
                && !silent_reported
                && last_sound.elapsed() >= APP_SILENT_AFTER
                && app_session_peak(app_pid).is_some_and(|p| p > 0.01)
            {
                silent_reported = true;
                let _ = ctx.status.send(SourceStatus::AppSilent);
            }
        }

        if reopen {
            drop(stream);
            let mut delay = Duration::from_millis(200);
            loop {
                if ctx.stop.load(Ordering::SeqCst) {
                    break 'outer;
                }
                match open(&ctx.target, app_pid) {
                    Ok(s) => {
                        let _ =
                            ctx.status.send(SourceStatus::DeviceSwitched { source: ctx.id, device: s.device.clone() });
                        stream = s;
                        last_packet = Instant::now();
                        break;
                    }
                    Err(err) => {
                        tracing::warn!("{}: reopen failed: {err}", ctx.id.as_str());
                        if matches!(err, CaptureError::MicDenied) {
                            let _ = ctx.status.send(SourceStatus::MicDenied);
                        }
                        std::thread::sleep(delay);
                        delay = (delay * 2).min(Duration::from_secs(2));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_sample_formats() {
        let f = |kind, block_align| Fmt { rate: 48_000, channels: 1, kind, block_align };
        assert_eq!(f(SampleKind::I16, 2).convert(&[0x00, 0x40]), vec![0.5]);
        assert_eq!(f(SampleKind::F32, 4).convert(&0.25f32.to_le_bytes()), vec![0.25]);
        let v = f(SampleKind::I24, 3).convert(&[0x00, 0x00, 0xC0]);
        assert!((v[0] + 0.5).abs() < 1e-6);
        let v = f(SampleKind::I32, 4).convert(&(i32::MIN / 2).to_le_bytes());
        assert!((v[0] + 0.5).abs() < 1e-6);
    }
}
