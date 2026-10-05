//! The source picker's live waves: how loud each app is right now, from the peak meters Windows
//! keeps for every audio session (the bars of its volume mixer). Nothing is captured. While the
//! picker shows apps, a thread reads their meters and sends `audio://app-levels` ten times a
//! second as long as one of them makes a sound; in silence it sends nothing.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use wasapi::{AudioMeterInformation, DeviceEnumerator, Direction};

use super::process;
use crate::audio::level::to_dbfs;
use crate::events::{self, EventSink};

/// How often the meters are read. A meter holds only its last device period (about 10 ms), so
/// each report carries the loudest of `READS_PER_REPORT` reads. One read takes about 30 µs and
/// listing the sessions about 2 ms (measured 2026-10-05), so watching one app costs ~0.15% of a
/// core.
const READ_EVERY: Duration = Duration::from_millis(50);
const READS_PER_REPORT: u32 = 2;
/// How often the sessions are listed again, to find apps that start playing.
const RELIST_EVERY: Duration = Duration::from_secs(2);
/// Reports still sent after the last sound, so the waves hear it stop.
const TAIL_REPORTS: u32 = 2;
/// Quieter than this (-60 dBFS) is silence.
const SILENT: f32 = 0.001;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLevel {
    pub root_pid: u32,
    pub dbfs: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppLevelsPayload {
    pub levels: Vec<AppLevel>,
}

#[derive(Default)]
struct Watch {
    /// The apps to report on, by root PID; the thread ends when there are none.
    roots: Vec<u32>,
    running: bool,
}

/// Starts, retargets and stops the meter thread.
#[derive(Default)]
pub struct AppMeters {
    state: Arc<Mutex<Watch>>,
}

impl AppMeters {
    /// Reports on these apps (root PIDs, as in `AudioApp.rootPid`) from now on; none stops it.
    pub fn watch(&self, sink: Arc<dyn EventSink>, mut roots: Vec<u32>) {
        roots.retain(|&r| r != 0);
        roots.sort_unstable();
        roots.dedup();
        let mut w = self.state.lock();
        w.roots = roots;
        if w.roots.is_empty() || w.running {
            return;
        }
        let state = self.state.clone();
        match std::thread::Builder::new().name("app-meters".into()).spawn(move || run(&state, sink.as_ref())) {
            Ok(_) => w.running = true,
            Err(e) => tracing::warn!("app meters: {e}"),
        }
    }
}

fn run(state: &Mutex<Watch>, sink: &dyn EventSink) {
    let _ = wasapi::initialize_mta();
    let mut sessions = Sessions::default();
    let mut peaks: HashMap<u32, f32> = HashMap::new();
    let mut reads = 0;
    // Nothing heard yet, so nothing to send.
    let mut quiet = TAIL_REPORTS + 1;
    let mut next = Instant::now();
    loop {
        let roots = {
            let mut w = state.lock();
            if w.roots.is_empty() {
                w.running = false;
                break;
            }
            w.roots.clone()
        };
        if crate::window::webview_hidden() {
            // No one can see the waves: wait without reading.
            peaks.clear();
            reads = 0;
            quiet = TAIL_REPORTS + 1;
            std::thread::sleep(Duration::from_millis(250));
            next = Instant::now();
            continue;
        }
        sessions.relist();
        for &root in &roots {
            let peak = peaks.entry(root).or_default();
            *peak = peak.max(sessions.peak(root));
        }
        reads += 1;
        if reads == READS_PER_REPORT {
            reads = 0;
            let peak_of = |root: u32| peaks.get(&root).copied().unwrap_or(0.0);
            quiet = if roots.iter().any(|&r| peak_of(r) > SILENT) { 0 } else { quiet.saturating_add(1) };
            if quiet <= TAIL_REPORTS {
                let levels = roots
                    .iter()
                    .map(|&root_pid| AppLevel { root_pid, dbfs: (to_dbfs(peak_of(root_pid)) * 10.0).round() / 10.0 })
                    .collect();
                events::emit(sink, events::AUDIO_APP_LEVELS, &AppLevelsPayload { levels });
            }
            peaks.clear();
        }
        next += READ_EVERY;
        match next.checked_duration_since(Instant::now()) {
            Some(wait) => std::thread::sleep(wait),
            None => next = Instant::now(),
        }
    }
    // The meters are COM objects: release them before leaving the apartment.
    drop(sessions);
    wasapi::deinitialize();
}

/// The session meters of every app, listed again every `RELIST_EVERY`.
#[derive(Default)]
struct Sessions {
    meters: Vec<(u32, AudioMeterInformation)>,
    /// The root of each process seen with a session, so the process list is read only when a new one appears.
    roots: HashMap<u32, u32>,
    listed: Option<Instant>,
}

impl Sessions {
    fn relist(&mut self) {
        if self.listed.is_some_and(|t| t.elapsed() < RELIST_EVERY) {
            return;
        }
        self.listed = Some(Instant::now());
        let found = session_meters();
        if found.iter().any(|(pid, _)| !self.roots.contains_key(pid)) {
            let procs = process::snapshot();
            self.roots = found.iter().map(|&(pid, _)| (pid, process::root_pid(&procs, pid))).collect();
        }
        self.meters = found.into_iter().filter_map(|(pid, meter)| Some((*self.roots.get(&pid)?, meter))).collect();
    }

    /// The loudest of the app's sessions, 0..1.
    fn peak(&self, root: u32) -> f32 {
        self.meters.iter().filter(|(r, _)| *r == root).filter_map(|(_, m)| m.get_peak_value().ok()).fold(0.0, f32::max)
    }
}

/// The process and peak meter of every audio session on the output devices.
fn session_meters() -> Vec<(u32, AudioMeterInformation)> {
    let mut out = Vec::new();
    let Ok(enumerator) = DeviceEnumerator::new() else { return out };
    let Ok(devices) = enumerator.get_device_collection(&Direction::Render) else { return out };
    for i in 0..devices.get_nbr_devices().unwrap_or(0) {
        let Ok(device) = devices.get_device_at_index(i) else { continue };
        let Ok(manager) = device.get_iaudiosessionmanager() else { continue };
        let Ok(sessions) = manager.get_audiosessionenumerator() else { continue };
        for s in 0..sessions.get_count().unwrap_or(0) {
            let Ok(control) = sessions.get_session(s) else { continue };
            // PID 0 is the system sounds.
            let Ok(pid) = control.get_process_id() else { continue };
            if pid == 0 {
                continue;
            }
            if let Ok(meter) = control.get_audiometerinformation() {
                out.push((pid, meter));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::audio::win::devices::run_in_mta;
    use crate::events::test_sink::Recorded;

    #[test]
    fn session_meters_read_between_zero_and_one() {
        // A CI runner may have no output device; reading must still not fail.
        run_in_mta(|| {
            for (_, meter) in session_meters() {
                let peak = meter.get_peak_value().unwrap_or(0.0);
                assert!((0.0..=1.0).contains(&peak));
            }
        });
    }

    #[test]
    fn a_silent_app_sends_nothing_and_no_apps_stop_the_thread() {
        let sink = Arc::new(Recorded::default());
        let meters = AppMeters::default();
        // No process has this PID, so it never makes a sound.
        meters.watch(sink.clone(), vec![u32::MAX - 7]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(meters.state.lock().running);
        meters.watch(sink.clone(), Vec::new());
        let deadline = Instant::now() + Duration::from_secs(2);
        while meters.state.lock().running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!meters.state.lock().running);
        assert!(sink.named(events::AUDIO_APP_LEVELS).is_empty());
    }
}
