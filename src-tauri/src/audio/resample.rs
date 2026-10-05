//! Device rate to 16 kHz mono, in 10–20 ms chunks (section 8).
//!
//! The FFT resampler adds a fixed delay; the first `output_delay()` frames are dropped so
//! output sample `n` lines up with input time `n / 16000` s.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

pub const TARGET_RATE: u32 = 16_000;

pub struct MonoResampler {
    inner: Option<Fft<f32>>,
    pending: Vec<f32>,
    out_buf: Vec<f32>,
    delay_left: usize,
    in_rate: u32,
    frames_in: u64,
    frames_out: u64,
}

impl MonoResampler {
    pub fn new(in_rate: u32) -> anyhow::Result<Self> {
        let inner = if in_rate == TARGET_RATE {
            None
        } else {
            // About 10 ms of input per chunk; FixedSync::Both rounds it to fit the ratio.
            let chunk = (in_rate as usize / 100).max(1);
            Some(Fft::<f32>::new(in_rate as usize, TARGET_RATE as usize, chunk, 1, FixedSync::Both)?)
        };
        let delay_left = inner.as_ref().map(|r| r.output_delay()).unwrap_or(0);
        let out_len = inner.as_ref().map(|r| r.output_frames_max()).unwrap_or(0);
        Ok(Self {
            inner,
            pending: Vec::new(),
            out_buf: vec![0.0; out_len],
            delay_left,
            in_rate,
            frames_in: 0,
            frames_out: 0,
        })
    }

    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    /// Feeds mono samples and appends any 16 kHz output to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> anyhow::Result<()> {
        self.frames_in += input.len() as u64;
        let Some(resampler) = self.inner.as_mut() else {
            out.extend_from_slice(input);
            self.frames_out += input.len() as u64;
            return Ok(());
        };
        self.pending.extend_from_slice(input);
        let mut consumed = 0;
        loop {
            let need = resampler.input_frames_next();
            if self.pending.len() - consumed < need {
                break;
            }
            let chunk = &self.pending[consumed..consumed + need];
            let in_adapter = InterleavedSlice::new(chunk, 1, need)?;
            let out_frames = resampler.output_frames_next();
            let mut out_adapter = InterleavedSlice::new_mut(&mut self.out_buf[..out_frames], 1, out_frames)?;
            let (_, written) = resampler.process_into_buffer(&in_adapter, &mut out_adapter, None)?;
            consumed += need;
            Self::emit(&self.out_buf[..written], &mut self.delay_left, &mut self.frames_out, out);
        }
        self.pending.drain(..consumed);
        Ok(())
    }

    fn emit(chunk: &[f32], delay_left: &mut usize, frames_out: &mut u64, out: &mut Vec<f32>) {
        let skip = (*delay_left).min(chunk.len());
        *delay_left -= skip;
        out.extend_from_slice(&chunk[skip..]);
        *frames_out += (chunk.len() - skip) as u64;
    }

    /// Pushes out the tail so the output length matches the input duration.
    pub fn flush(&mut self, out: &mut Vec<f32>) -> anyhow::Result<()> {
        let Some(resampler) = self.inner.as_mut() else { return Ok(()) };
        let expected = self.frames_in * TARGET_RATE as u64 / self.in_rate as u64;
        let mut guard = 0;
        while self.frames_out < expected && guard < 64 {
            guard += 1;
            let need = resampler.input_frames_next();
            let take = self.pending.len().min(need);
            let mut chunk = vec![0.0f32; need];
            chunk[..take].copy_from_slice(&self.pending[..take]);
            self.pending.drain(..take);
            let in_adapter = InterleavedSlice::new(&chunk[..], 1, need)?;
            let out_frames = resampler.output_frames_next();
            let mut out_adapter = InterleavedSlice::new_mut(&mut self.out_buf[..out_frames], 1, out_frames)?;
            let indexing = Indexing::new().partial_len(take);
            let (_, written) = resampler.process_into_buffer(&in_adapter, &mut out_adapter, Some(&indexing))?;
            let remaining = (expected - self.frames_out) as usize;
            let mut tmp = Vec::new();
            Self::emit(&self.out_buf[..written], &mut self.delay_left, &mut self.frames_out, &mut tmp);
            if tmp.len() > remaining {
                self.frames_out -= (tmp.len() - remaining) as u64;
                tmp.truncate(remaining);
            }
            out.extend_from_slice(&tmp);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5).collect()
    }

    /// Strongest frequency by correlating against candidate sines.
    fn peak_freq(signal: &[f32], rate: u32) -> f32 {
        let mut best = (0.0f32, 0.0f32);
        let mut f = 100.0;
        while f <= 4000.0 {
            let w = 2.0 * std::f32::consts::PI * f / rate as f32;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in signal.iter().enumerate() {
                re += s * (w * i as f32).cos();
                im += s * (w * i as f32).sin();
            }
            let mag = re * re + im * im;
            if mag > best.1 {
                best = (f, mag);
            }
            f += 50.0;
        }
        best.0
    }

    #[test]
    fn one_khz_stays_one_khz() {
        let input = sine(1000.0, 48_000, 1.0);
        let mut r = MonoResampler::new(48_000).unwrap();
        let mut out = Vec::new();
        for chunk in input.chunks(960) {
            r.process(chunk, &mut out).unwrap();
        }
        r.flush(&mut out).unwrap();
        assert_eq!(out.len(), 16_000);
        assert_eq!(peak_freq(&out[2000..14000], TARGET_RATE), 1000.0);
    }

    #[test]
    fn handles_44100_and_uneven_chunks() {
        let input = sine(1000.0, 44_100, 0.5);
        let mut r = MonoResampler::new(44_100).unwrap();
        let mut out = Vec::new();
        for chunk in input.chunks(333) {
            r.process(chunk, &mut out).unwrap();
        }
        r.flush(&mut out).unwrap();
        assert_eq!(out.len(), 8_000);
        assert_eq!(peak_freq(&out[1000..7000], TARGET_RATE), 1000.0);
    }

    #[test]
    fn output_is_time_aligned_with_input() {
        // An impulse at 0.25 s must come out near sample 4000.
        let mut input = vec![0.0f32; 48_000];
        input[12_000] = 1.0;
        let mut r = MonoResampler::new(48_000).unwrap();
        let mut out = Vec::new();
        for chunk in input.chunks(480) {
            r.process(chunk, &mut out).unwrap();
        }
        r.flush(&mut out).unwrap();
        let peak = out.iter().enumerate().max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap()).unwrap().0;
        assert!((peak as i64 - 4000).abs() <= 2, "peak at {peak}");
    }

    #[test]
    fn passthrough_at_16k() {
        let mut r = MonoResampler::new(16_000).unwrap();
        let mut out = Vec::new();
        r.process(&[0.1, 0.2], &mut out).unwrap();
        r.flush(&mut out).unwrap();
        assert_eq!(out, vec![0.1, 0.2]);
    }
}
