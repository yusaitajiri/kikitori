//! Places capture buffers on the session timeline by their QPC timestamps (section 7).
//!
//! Buffers are never placed by counting samples: system loopback delivers nothing while
//! silent, and app loopback reports a device position of 0. Gaps over 20 ms become zeros,
//! overlaps are dropped, and silent buffers are zeros of their stated length.

/// Gaps up to this long are treated as jitter and joined without padding.
pub const GAP_TOLERANCE_MS: u64 = 20;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClockStats {
    pub gaps_filled: u64,
    pub zero_frames_inserted: u64,
    pub overlaps: u64,
    pub frames_dropped: u64,
}

/// What [`StreamClock::place`] hands back, in stream order.
#[derive(Debug, PartialEq)]
pub enum Piece<'a> {
    /// This many frames of silence (a gap, a silent buffer). Never materialized here, so a
    /// long gap (an app that was closed for minutes) costs no memory.
    Zeros(u64),
    /// Interleaved samples to append.
    Data(&'a [f32]),
}

/// Aligns one stream (one device rate, interleaved frames) to wall-clock time since `t0`.
#[derive(Debug)]
pub struct StreamClock {
    t0_100ns: u64,
    rate: u32,
    channels: u16,
    /// Frame index (at `rate`) where the next buffer is expected to start.
    next_frame: u64,
    pub stats: ClockStats,
}

impl StreamClock {
    pub fn new(t0_100ns: u64, rate: u32, channels: u16) -> Self {
        Self::with_position(t0_100ns, rate, channels, 0)
    }

    /// A clock that continues an existing stream (the device format changed mid-session).
    pub fn with_position(t0_100ns: u64, rate: u32, channels: u16, next_frame: u64) -> Self {
        Self { t0_100ns, rate, channels, next_frame, stats: ClockStats::default() }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Frames emitted so far, i.e. the stream position.
    pub fn position_frames(&self) -> u64 {
        self.next_frame
    }

    pub fn position_ms(&self) -> u64 {
        self.next_frame * 1000 / self.rate as u64
    }

    fn frame_at(&self, qpc_100ns: u64) -> u64 {
        let elapsed = qpc_100ns.saturating_sub(self.t0_100ns) as u128;
        (elapsed * self.rate as u128 / 10_000_000) as u64
    }

    /// Places a buffer: padding for a gap, then the buffer (or zeros if flagged silent),
    /// minus any overlap.
    pub fn place<'a>(&mut self, qpc_100ns: u64, samples: &'a [f32], silent: bool, mut out: impl FnMut(Piece<'a>)) {
        let ch = self.channels as usize;
        let frames = samples.len() / ch;
        let start = self.frame_at(qpc_100ns);
        let tolerance = self.rate as u64 * GAP_TOLERANCE_MS / 1000;

        let mut skip = 0usize;
        if start > self.next_frame + tolerance {
            let gap = start - self.next_frame;
            self.stats.gaps_filled += 1;
            self.stats.zero_frames_inserted += gap;
            out(Piece::Zeros(gap));
            self.next_frame = start;
        } else if start + tolerance < self.next_frame {
            let overlap = (self.next_frame - start) as usize;
            skip = overlap.min(frames);
            self.stats.overlaps += 1;
            self.stats.frames_dropped += skip as u64;
            tracing::warn!("audio overlap: dropping {} frames ({} ms)", skip, skip as u64 * 1000 / self.rate as u64);
        }

        let kept = frames - skip;
        if kept > 0 {
            if silent {
                out(Piece::Zeros(kept as u64));
            } else {
                out(Piece::Data(&samples[skip * ch..frames * ch]));
            }
        }
        self.next_frame += kept as u64;
    }

    /// Advances a stream that has gone quiet (no packets at all) up to `qpc_100ns`, so the
    /// voice detector can close an utterance. Returns the frames of silence to append.
    pub fn pad_until(&mut self, qpc_100ns: u64) -> u64 {
        let target = self.frame_at(qpc_100ns);
        if target <= self.next_frame {
            return 0;
        }
        let gap = target - self.next_frame;
        self.next_frame = target;
        self.stats.zero_frames_inserted += gap;
        gap
    }
}

/// Averages interleaved channels into mono.
pub fn downmix(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let scale = 1.0 / ch as f32;
    out.extend(interleaved.chunks_exact(ch).map(|f| f.iter().sum::<f32>() * scale));
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_000_000_000; // arbitrary QPC origin, 100 ns units

    /// Places a buffer and materializes the result, as the tests read it.
    fn place(c: &mut StreamClock, qpc: u64, samples: &[f32], silent: bool, out: &mut Vec<f32>) {
        let ch = c.channels() as usize;
        c.place(qpc, samples, silent, |p| match p {
            Piece::Zeros(n) => out.extend(std::iter::repeat_n(0.0, n as usize * ch)),
            Piece::Data(d) => out.extend_from_slice(d),
        });
    }

    fn qpc_ms(ms: u64) -> u64 {
        T0 + ms * 10_000
    }

    #[test]
    fn contiguous_buffers_pass_through() {
        let mut c = StreamClock::new(T0, 1000, 1);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(0), &[1.0; 10], false, &mut out);
        place(&mut c, qpc_ms(10), &[2.0; 10], false, &mut out);
        assert_eq!(out.len(), 20);
        assert_eq!(c.position_ms(), 20);
        assert_eq!(c.stats, ClockStats::default());
    }

    #[test]
    fn first_buffer_late_is_padded_from_t0() {
        let mut c = StreamClock::new(T0, 1000, 2);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(300), &[0.5; 20], false, &mut out);
        // 300 frames of stereo zeros, then 10 frames of data.
        assert_eq!(out.len(), (300 + 10) * 2);
        assert!(out[..600].iter().all(|&s| s == 0.0));
        assert!(out[600..].iter().all(|&s| s == 0.5));
        assert_eq!(c.stats.gaps_filled, 1);
    }

    #[test]
    fn small_jitter_is_tolerated() {
        let mut c = StreamClock::new(T0, 1000, 1);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(0), &[1.0; 10], false, &mut out);
        place(&mut c, qpc_ms(25), &[1.0; 10], false, &mut out); // 15 ms late: jitter
        assert_eq!(out.len(), 20);
        place(&mut c, qpc_ms(5), &[1.0; 10], false, &mut out); // 15 ms early: jitter
        assert_eq!(out.len(), 30);
        assert_eq!(c.stats.gaps_filled, 0);
        assert_eq!(c.stats.overlaps, 0);
    }

    #[test]
    fn gap_over_20ms_is_filled_with_zeros() {
        let mut c = StreamClock::new(T0, 1000, 1);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(0), &[1.0; 10], false, &mut out);
        place(&mut c, qpc_ms(50), &[1.0; 10], false, &mut out);
        assert_eq!(out.len(), 60);
        assert!(out[10..50].iter().all(|&s| s == 0.0));
        assert_eq!(c.position_ms(), 60);
    }

    #[test]
    fn overlap_drops_the_overlapping_samples() {
        let mut c = StreamClock::new(T0, 1000, 1);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(0), &[1.0; 100], false, &mut out);
        let data: Vec<f32> = (0..50).map(|i| i as f32).collect();
        place(&mut c, qpc_ms(60), &data, false, &mut out); // starts 40 frames before expected
        assert_eq!(out.len(), 110);
        assert_eq!(out[100], 40.0);
        assert_eq!(c.stats.frames_dropped, 40);
        assert_eq!(c.position_ms(), 110);
    }

    #[test]
    fn silent_flag_yields_zeros_of_stated_length() {
        let mut c = StreamClock::new(T0, 1000, 2);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(0), &[0.9; 40], true, &mut out);
        assert_eq!(out, vec![0.0; 40]);
        assert_eq!(c.position_frames(), 20);
    }

    #[test]
    fn pad_until_advances_a_quiet_stream() {
        let mut c = StreamClock::new(T0, 1000, 1);
        let mut out = Vec::new();
        place(&mut c, qpc_ms(0), &[1.0; 10], false, &mut out);
        assert_eq!(c.pad_until(qpc_ms(500)), 490);
        assert_eq!(c.position_ms(), 500);
        assert_eq!(c.pad_until(qpc_ms(400)), 0);
    }

    #[test]
    fn downmix_averages_channels() {
        let mut out = Vec::new();
        downmix(&[1.0, 0.0, 0.5, 0.5], 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }
}
