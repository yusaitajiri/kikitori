//! Plays a WAV file through the real pipeline (integration harness, section 18).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;

use super::source::{AudioSource, RawChunk};
use crate::session::model::SourceId;

pub struct FileSource {
    id: SourceId,
    path: PathBuf,
    /// Use only this channel of a multi-channel file (left = 0), or all channels.
    channel: Option<usize>,
    t0_100ns: u64,
    /// 1.0 = real time; larger plays faster (timestamps stay on the file's clock).
    speed: f32,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FileSource {
    pub fn new(id: SourceId, path: PathBuf, channel: Option<usize>, t0_100ns: u64, speed: f32) -> Self {
        Self { id, path, channel, t0_100ns, speed, stop: Arc::new(AtomicBool::new(false)), thread: None }
    }

    /// Waits until the whole file has been sent.
    pub fn join(&mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Reads a WAV into interleaved f32 samples.
pub fn read_wav(path: &std::path::Path) -> anyhow::Result<(Vec<f32>, u32, u16)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()?
        }
    };
    Ok((samples, spec.sample_rate, spec.channels))
}

impl AudioSource for FileSource {
    fn id(&self) -> SourceId {
        self.id
    }

    fn start(&mut self, tx: Sender<RawChunk>) -> anyhow::Result<()> {
        let (samples, rate, channels) = read_wav(&self.path)?;
        let (data, out_channels) = match self.channel {
            Some(c) if (c as u16) < channels => {
                (samples.chunks_exact(channels as usize).map(|f| f[c]).collect::<Vec<f32>>(), 1u16)
            }
            _ => (samples, channels),
        };
        let (id, t0, speed, stop) = (self.id, self.t0_100ns, self.speed.max(0.01), self.stop.clone());
        self.thread = Some(std::thread::spawn(move || {
            let frames_per_chunk = (rate / 100) as usize; // 10 ms
            let started = Instant::now();
            for (i, chunk) in data.chunks(frames_per_chunk * out_channels as usize).enumerate() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let frame_pos = (i * frames_per_chunk) as u64;
                let due = Duration::from_secs_f64(frame_pos as f64 / rate as f64 / speed as f64);
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    std::thread::sleep(wait);
                }
                let qpc = t0 + frame_pos * 10_000_000 / rate as u64;
                let raw = RawChunk {
                    source: id,
                    qpc_100ns: qpc,
                    rate,
                    channels: out_channels,
                    samples: chunk.to_vec(),
                    silent_flag: false,
                };
                if tx.send(raw).is_err() {
                    break;
                }
            }
        }));
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.join();
    }
}
