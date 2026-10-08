//! The session's sound (FR-09): every source mixed into one Opus stream in an Ogg file per
//! recording (`audio/0001.ogg`, a continuation gets the next), so a line can be played back from
//! where it was said. The DSP threads hand over the 16 kHz audio they already make, placed in
//! session time; the file's time is session time from its start, pauses included as silence.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opusic_c::{Application, Bitrate, Channels, Encoder, SampleRate, Signal};

use super::model::SourceId;

pub const AUDIO_DIR: &str = "audio";
const RATE: u64 = 16_000;
/// 20 ms, Opus's usual frame.
const FRAME: usize = 320;
/// Speech at 24 kb/s: about 11 MB an hour, clear enough to tell who said what.
const BITRATE: u32 = 24_000;
/// A page, and a flush, about once a second: a crash loses at most that.
const PAGE_EVERY: u64 = 50;
/// A source that sent nothing for this long has stopped (paused, switched): the mix goes on without it.
const STALE: Duration = Duration::from_millis(1_500);
/// No source may hold the mix back by more than this.
const MAX_LEAD: u64 = 10 * RATE;
const SERIAL: u32 = 0x4b4b_4131;

/// Audio from one source at a session position, in 16 kHz samples.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub source: SourceId,
    pub pos: u64,
    pub samples: Vec<f32>,
}

/// The file a recording writes, relative to the session folder: `audio/0001.ogg`.
pub fn file_name(n: usize) -> String {
    format!("{AUDIO_DIR}/{n:04}.ogg")
}

/// Mixes the sources by session position: each chunk is added where it belongs, and the mix
/// is ready up to where every source still sending has delivered.
pub struct Mixer {
    /// Session position of `buf[0]`; everything before it was handed out.
    written: u64,
    buf: VecDeque<f32>,
    /// Per source: how far it has delivered, and when it last did.
    frontier: HashMap<SourceId, (u64, Instant)>,
}

impl Mixer {
    pub fn new(start: u64) -> Self {
        Self { written: start, buf: VecDeque::new(), frontier: HashMap::new() }
    }

    pub fn push(&mut self, chunk: &AudioChunk, now: Instant) {
        let end = chunk.pos + chunk.samples.len() as u64;
        let f = self.frontier.entry(chunk.source).or_insert((end, now));
        *f = (f.0.max(end), now);
        if end <= self.written {
            return;
        }
        // Late audio for what was handed out already is dropped.
        let skip = self.written.saturating_sub(chunk.pos) as usize;
        let offset = (chunk.pos.max(self.written) - self.written) as usize;
        let samples = &chunk.samples[skip..];
        if self.buf.len() < offset + samples.len() {
            self.buf.resize(offset + samples.len(), 0.0);
        }
        for (i, s) in samples.iter().enumerate() {
            self.buf[offset + i] += s;
        }
    }

    /// The session position the mix is complete up to.
    pub fn ready(&self, now: Instant) -> u64 {
        let furthest = self.written + self.buf.len() as u64;
        let sending = self.frontier.values().filter(|(_, at)| now.duration_since(*at) < STALE).map(|(f, _)| *f).min();
        sending.unwrap_or(furthest).max(furthest.saturating_sub(MAX_LEAD)).clamp(self.written, furthest)
    }

    /// The next 20 ms of the mix, if ready up to `upto`.
    pub fn frame(&mut self, upto: u64) -> Option<Vec<f32>> {
        if self.written + FRAME as u64 > upto || self.buf.len() < FRAME {
            return None;
        }
        self.written += FRAME as u64;
        Some(self.buf.drain(..FRAME).map(|s| s.clamp(-1.0, 1.0)).collect())
    }

    /// What is left, padded with silence to a whole frame.
    pub fn rest(&mut self) -> Option<Vec<f32>> {
        if self.buf.is_empty() {
            return None;
        }
        let mut last: Vec<f32> = self.buf.drain(..).map(|s| s.clamp(-1.0, 1.0)).collect();
        last.resize(FRAME * last.len().div_ceil(FRAME), 0.0);
        self.written += last.len() as u64;
        Some(last)
    }
}

/// Opus packets into an Ogg stream (RFC 7845). The last packet is held back until the next one
/// comes, so the stream can be ended on it.
struct OpusFile {
    ogg: PacketWriter<'static, BufWriter<File>>,
    encoder: Encoder,
    pre_skip: u64,
    packets: u64,
    held: Option<Vec<u8>>,
}

impl OpusFile {
    fn create(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut encoder = Encoder::new(Channels::Mono, SampleRate::Hz16000, Application::Voip)
            .map_err(|e| anyhow::anyhow!("opus encoder: {e:?}"))?;
        encoder.set_bitrate(Bitrate::Value(BITRATE)).map_err(|e| anyhow::anyhow!("opus bitrate: {e:?}"))?;
        encoder.set_signal(Signal::Voice).map_err(|e| anyhow::anyhow!("opus signal: {e:?}"))?;
        // Encoder lookahead, in 48 kHz samples as the header counts them.
        let pre_skip = u64::from(encoder.get_look_ahead().map_err(|e| anyhow::anyhow!("opus lookahead: {e:?}"))?) * 3;
        let mut ogg = PacketWriter::new(BufWriter::new(File::create(path)?));
        let mut head = b"OpusHead".to_vec();
        head.push(1); // version
        head.push(1); // one channel
        head.extend_from_slice(&(pre_skip as u16).to_le_bytes());
        head.extend_from_slice(&(RATE as u32).to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // mapping family: mono/stereo
        ogg.write_packet(head, SERIAL, PacketWriteEndInfo::EndPage, 0)?;
        let vendor = concat!("Kikitori ", env!("CARGO_PKG_VERSION"));
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor.as_bytes());
        tags.extend_from_slice(&0u32.to_le_bytes()); // no comments
        ogg.write_packet(tags, SERIAL, PacketWriteEndInfo::EndPage, 0)?;
        ogg.inner_mut().flush()?;
        Ok(Self { ogg, encoder, pre_skip, packets: 0, held: None })
    }

    fn encode(&mut self, frame: &[f32]) -> anyhow::Result<()> {
        let mut packet = Vec::with_capacity(1_500);
        self.encoder.encode_float_to_vec(frame, &mut packet).map_err(|e| anyhow::anyhow!("opus encode: {e:?}"))?;
        if let Some(previous) = self.held.replace(packet) {
            self.write(previous, false)?;
        }
        Ok(())
    }

    /// Writes a packet; its granule position is where its audio ends, in 48 kHz samples.
    fn write(&mut self, packet: Vec<u8>, last: bool) -> anyhow::Result<()> {
        self.packets += 1;
        let granule = self.pre_skip + self.packets * (FRAME as u64 * 3);
        let end = if last {
            PacketWriteEndInfo::EndStream
        } else if self.packets.is_multiple_of(PAGE_EVERY) {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        self.ogg.write_packet(packet, SERIAL, end, granule)?;
        if end != PacketWriteEndInfo::NormalPacket {
            self.ogg.inner_mut().flush()?;
        }
        Ok(())
    }

    fn finish(mut self) -> anyhow::Result<()> {
        if let Some(last) = self.held.take() {
            self.write(last, true)?;
        }
        self.ogg.inner_mut().flush()?;
        Ok(())
    }
}

/// Writes one recording's sound on its own thread.
pub struct AudioRecorder {
    tx: Sender<AudioChunk>,
    thread: JoinHandle<()>,
}

impl AudioRecorder {
    /// Creates the file (its headers written, so a failure shows at once) for audio from session
    /// position `start` (16 kHz samples) on.
    pub fn start(path: PathBuf, start: u64) -> anyhow::Result<Self> {
        let file = OpusFile::create(&path)?;
        let (tx, rx) = crossbeam_channel::unbounded();
        let thread = std::thread::Builder::new().name("audio".into()).spawn(move || run(rx, file, start, &path))?;
        Ok(Self { tx, thread })
    }

    /// Where the DSP threads send their audio.
    pub fn sender(&self) -> Sender<AudioChunk> {
        self.tx.clone()
    }

    /// Writes what is left and closes the file, once the DSP threads have stopped.
    pub fn finish(self) {
        drop(self.tx);
        if self.thread.join().is_err() {
            tracing::error!("audio thread panicked");
        }
    }
}

fn run(rx: Receiver<AudioChunk>, mut file: OpusFile, start: u64, path: &Path) {
    let mut mixer = Mixer::new(start);
    let mut failed = false;
    let encode = |file: &mut OpusFile, frame: &[f32], failed: &mut bool| {
        if !*failed && let Err(e) = file.encode(frame) {
            tracing::warn!("{}: {e:#}; the rest of the recording keeps no sound", path.display());
            *failed = true;
        }
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => mixer.push(&chunk, Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let upto = mixer.ready(Instant::now());
        while let Some(frame) = mixer.frame(upto) {
            encode(&mut file, &frame, &mut failed);
        }
    }
    while let Some(frame) = mixer.frame(u64::MAX) {
        encode(&mut file, &frame, &mut failed);
    }
    if let Some(rest) = mixer.rest() {
        for frame in rest.chunks(FRAME) {
            encode(&mut file, frame, &mut failed);
        }
    }
    if let Err(e) = file.finish() {
        tracing::warn!("{}: {e:#}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(source: SourceId, pos: u64, value: f32, len: usize) -> AudioChunk {
        AudioChunk { source, pos, samples: vec![value; len] }
    }

    #[test]
    fn sources_mix_by_position_and_wait_for_each_other() {
        let now = Instant::now();
        let mut m = Mixer::new(16_000);
        m.push(&chunk(SourceId::App, 16_000, 0.25, 640), now);
        // The mic has sent nothing yet: the app alone is all there is.
        assert_eq!(m.ready(now), 16_640);
        m.push(&chunk(SourceId::Mic, 16_320, 0.5, 160), now);
        // Now the mic is sending, and has delivered less: the mix waits for it.
        assert_eq!(m.ready(now), 16_480);
        let first = m.frame(m.ready(now)).unwrap();
        assert!(first.iter().all(|&s| s == 0.25));
        assert!(m.frame(m.ready(now)).is_none());
        m.push(&chunk(SourceId::Mic, 16_480, 0.5, 160), now);
        let second = m.frame(m.ready(now)).unwrap();
        assert!(second.iter().all(|&s| s == 0.75));
    }

    #[test]
    fn a_stopped_source_stops_holding_the_mix_and_a_pause_is_silence() {
        let then = Instant::now();
        let mut m = Mixer::new(0);
        m.push(&chunk(SourceId::Mic, 0, 0.1, 320), then);
        m.push(&chunk(SourceId::App, 0, 0.1, 960), then);
        let later = then + STALE + Duration::from_millis(10);
        m.push(&chunk(SourceId::App, 960, 0.1, 320), later);
        // The mic went quiet long ago (paused, switched away): the app alone sets the pace.
        assert_eq!(m.ready(later), 1_280);
        while m.frame(m.ready(later)).is_some() {}
        // Resumed a second later: the gap is handed out as silence before the new audio.
        m.push(&chunk(SourceId::App, 1_280 + 16_000, 0.3, 320), later);
        let gap = m.frame(m.ready(later)).unwrap();
        assert!(gap.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn late_audio_is_dropped_and_the_rest_is_padded() {
        let now = Instant::now();
        let mut m = Mixer::new(0);
        m.push(&chunk(SourceId::App, 0, 0.2, 400), now);
        assert!(m.frame(m.ready(now)).is_some());
        m.push(&chunk(SourceId::Mic, 0, 0.9, 100), now);
        let rest = m.rest().unwrap();
        assert_eq!(rest.len(), FRAME);
        assert!(rest[..80].iter().all(|&s| s == 0.2) && rest[80..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn writes_an_ogg_opus_file_that_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(file_name(1));
        let rec = AudioRecorder::start(path.clone(), 0).unwrap();
        let tx = rec.sender();
        // Three seconds of a tone in 10 ms chunks, as a DSP thread sends them.
        for i in 0..300u64 {
            let samples = (0..160).map(|n| ((i * 160 + n) as f32 * 0.07).sin() * 0.3).collect();
            tx.send(AudioChunk { source: SourceId::Mic, pos: i * 160, samples }).unwrap();
        }
        drop(tx);
        rec.finish();
        let mut reader = ogg::reading::PacketReader::new(std::fs::File::open(&path).unwrap());
        let head = reader.read_packet().unwrap().unwrap();
        assert!(head.data.starts_with(b"OpusHead"));
        assert_eq!(u32::from_le_bytes(head.data[12..16].try_into().unwrap()), 16_000);
        let tags = reader.read_packet().unwrap().unwrap();
        assert!(tags.data.starts_with(b"OpusTags"));
        let mut packets = 0;
        let mut last = None;
        while let Some(p) = reader.read_packet().unwrap() {
            packets += 1;
            last = Some((p.absgp_page(), p.last_in_stream()));
        }
        assert_eq!(packets, 150);
        let (granule, end) = last.unwrap();
        assert!(end);
        let pre_skip = u64::from(u16::from_le_bytes(head.data[10..12].try_into().unwrap()));
        // Three seconds at 48 kHz after the pre-skip.
        assert_eq!(granule - pre_skip, 3 * 48_000);
        // Speech-rate audio: a few kilobytes a second.
        let bytes = std::fs::metadata(&path).unwrap().len();
        assert!(bytes > 3_000 && bytes < 20_000, "{bytes} bytes");
    }
}
