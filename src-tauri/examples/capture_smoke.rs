//! Manual check of live capture: `cargo run --example capture_smoke -- <system|mic|app PID EXE> [secs]`.
//! Prints packet counts, format, peak level and timestamp continuity.

use std::time::{Duration, Instant};

use kikitori_lib::audio::source::AudioSource;
use kikitori_lib::audio::win::capture::{CaptureTarget, WasapiSource};
use kikitori_lib::platform;
use kikitori_lib::session::model::SourceId;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (id, target, rest) = match args.first().map(String::as_str) {
        Some("mic") => (SourceId::Mic, CaptureTarget::Mic { device_id: None }, &args[1..]),
        Some("app") => {
            let pid: u32 = args[1].parse()?;
            (SourceId::App, CaptureTarget::App { root_pid: pid, exe: args[2].clone().into() }, &args[3..])
        }
        _ => (SourceId::System, CaptureTarget::System, &args[1.min(args.len())..]),
    };
    let secs: u64 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(3);
    let (status_tx, status_rx) = crossbeam_channel::unbounded();
    let (tx, rx) = crossbeam_channel::bounded(256);
    let mut src = WasapiSource::new(id, target, status_tx);
    let t0 = platform::qpc_now_100ns();
    let started = Instant::now();
    src.start(tx)?;
    println!("started in {:?}", started.elapsed());
    let mut packets = 0u64;
    let mut frames = 0u64;
    let mut peak = 0f32;
    let mut silent = 0u64;
    let mut first_ts: Option<u64> = None;
    let mut last_end = 0u64;
    let mut max_gap_ms = 0f64;
    while started.elapsed() < Duration::from_secs(secs) {
        if let Ok(c) = rx.recv_timeout(Duration::from_millis(100)) {
            packets += 1;
            let n = c.samples.len() as u64 / c.channels as u64;
            frames += n;
            if c.silent_flag {
                silent += 1;
            }
            peak = c.samples.iter().fold(peak, |p, s| p.max(s.abs()));
            first_ts.get_or_insert(c.qpc_100ns);
            if last_end > 0 {
                let gap = (c.qpc_100ns as f64 - last_end as f64) / 10_000.0;
                max_gap_ms = max_gap_ms.max(gap.abs());
            }
            last_end = c.qpc_100ns + n * 10_000_000 / c.rate as u64;
            if packets == 1 {
                println!(
                    "format: {} Hz, {} ch; first packet at +{:.1} ms after t0",
                    c.rate,
                    c.channels,
                    (c.qpc_100ns as f64 - t0 as f64) / 10_000.0
                );
            }
        }
        while let Ok(s) = status_rx.try_recv() {
            println!("status: {s:?}");
        }
    }
    src.stop();
    println!(
        "packets {packets}, frames {frames}, silent packets {silent}, peak {peak:.4}, max timestamp jump {max_gap_ms:.1} ms"
    );
    Ok(())
}
