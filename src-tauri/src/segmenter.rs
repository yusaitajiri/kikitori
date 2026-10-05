//! Cuts a 16 kHz stream into utterances from per-frame VAD scores (section 8).
//!
//! Defaults: open at 0.5 for 3 frames, close below 0.35 for 640 ms, keep 320 ms of pre-roll,
//! drop speech under 400 ms, cut at the quietest 100 ms of the last 3 s once an utterance
//! reaches 15 s, and always cut at 25 s.

use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct SegmenterConfig {
    pub frame_len: usize,
    pub start_threshold: f32,
    pub start_frames: usize,
    pub end_threshold: f32,
    pub hangover_ms: u32,
    pub preroll_ms: u32,
    /// Silence kept after the last speech frame, capped by the hangover.
    pub trail_ms: u32,
    pub min_speech_ms: u32,
    pub soft_max_ms: u32,
    pub soft_search_ms: u32,
    pub quiet_window_ms: u32,
    pub hard_max_ms: u32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            frame_len: 256,
            start_threshold: 0.5,
            start_frames: 3,
            end_threshold: 0.35,
            hangover_ms: 640,
            preroll_ms: 320,
            trail_ms: 320,
            min_speech_ms: 400,
            soft_max_ms: 15_000,
            soft_search_ms: 3_000,
            quiet_window_ms: 100,
            hard_max_ms: 25_000,
        }
    }
}

impl SegmenterConfig {
    fn frames(&self, ms: u32) -> usize {
        ((ms as usize * SAMPLE_RATE as usize / 1000) + self.frame_len / 2) / self.frame_len
    }

    fn samples(&self, ms: u32) -> usize {
        ms as usize * SAMPLE_RATE as usize / 1000
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Silence,
    SoftMax,
    HardMax,
    Flush,
}

#[derive(Debug, Clone)]
pub struct Utterance {
    /// 16 kHz sample index since the session start.
    pub start_sample: u64,
    pub samples: Vec<f32>,
    pub speech_ms: u32,
    pub reason: CloseReason,
}

impl Utterance {
    pub fn start_ms(&self) -> u64 {
        self.start_sample * 1000 / SAMPLE_RATE as u64
    }

    pub fn end_ms(&self) -> u64 {
        (self.start_sample + self.samples.len() as u64) * 1000 / SAMPLE_RATE as u64
    }

    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1000 / SAMPLE_RATE as u64
    }
}

#[derive(Debug, Clone)]
pub enum SegEvent {
    /// An utterance opened; the caller assigns its ID here so partials and finals match.
    Opened {
        start_sample: u64,
    },
    Closed(Utterance),
    /// The open utterance had too little speech and was dropped.
    Discarded {
        start_sample: u64,
    },
}

fn speech_ms(speech_start: usize, speech_end: usize) -> u32 {
    (speech_end.saturating_sub(speech_start) as u64 * 1000 / SAMPLE_RATE as u64) as u32
}

struct Open {
    start_sample: u64,
    audio: Vec<f32>,
    /// Energy of each frame in `audio`.
    energy: Vec<f32>,
    speech_start: usize,
    last_speech_end: usize,
    silence_run: usize,
}

pub struct Segmenter {
    cfg: SegmenterConfig,
    /// Frames since the last utterance ended, for pre-roll (never reaches into the last one).
    recent: VecDeque<f32>,
    recent_cap: usize,
    above: usize,
    open: Option<Open>,
    position: u64,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig) -> Self {
        Self::with_position(cfg, 0)
    }

    /// A segmenter whose first frame starts at `position` (16 kHz samples since the session
    /// start), for streams that begin mid-session (after a pause).
    pub fn with_position(cfg: SegmenterConfig, position: u64) -> Self {
        let recent_cap = (cfg.frames(cfg.preroll_ms) + cfg.start_frames) * cfg.frame_len;
        Self { cfg, recent: VecDeque::with_capacity(recent_cap), recent_cap, above: 0, open: None, position }
    }

    pub fn config(&self) -> &SegmenterConfig {
        &self.cfg
    }

    pub fn frame_len(&self) -> usize {
        self.cfg.frame_len
    }

    /// Sample index after the last frame pushed.
    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The open utterance so far (for partial transcripts).
    pub fn open_audio(&self) -> Option<(u64, &[f32])> {
        self.open.as_ref().map(|o| (o.start_sample, &o.audio[..]))
    }

    pub fn push_frame(&mut self, frame: &[f32], score: f32) -> Vec<SegEvent> {
        debug_assert_eq!(frame.len(), self.cfg.frame_len);
        let mut events = Vec::new();
        let frame_start = self.position;
        self.position += frame.len() as u64;
        let energy: f32 = frame.iter().map(|s| s * s).sum();

        match self.open.as_mut() {
            None => {
                self.recent.extend(frame.iter().copied());
                while self.recent.len() > self.recent_cap {
                    self.recent.pop_front();
                }
                if score >= self.cfg.start_threshold {
                    self.above += 1;
                } else {
                    self.above = 0;
                }
                if self.above >= self.cfg.start_frames {
                    self.above = 0;
                    let fl = self.cfg.frame_len;
                    let start_frames_len = self.cfg.start_frames * fl;
                    let speech_begin = frame_start + fl as u64 - start_frames_len as u64;
                    let audio: Vec<f32> = self.recent.drain(..).collect();
                    let preroll = audio.len() - start_frames_len;
                    let start_sample = speech_begin - preroll as u64;
                    let energy = audio.chunks(fl).map(|f| f.iter().map(|s| s * s).sum()).collect();
                    let len = audio.len();
                    self.open = Some(Open {
                        start_sample,
                        audio,
                        energy,
                        speech_start: preroll,
                        last_speech_end: len,
                        silence_run: 0,
                    });
                    events.push(SegEvent::Opened { start_sample });
                }
            }
            Some(open) => {
                if open.audio.len() + frame.len() > self.cfg.samples(self.cfg.hard_max_ms) {
                    self.split_at_end(&mut events);
                }
                let open = self.open.as_mut().unwrap();
                open.audio.extend_from_slice(frame);
                open.energy.push(energy);
                if score >= self.cfg.end_threshold {
                    open.silence_run = 0;
                    open.last_speech_end = open.audio.len();
                } else {
                    open.silence_run += 1;
                }
                if open.silence_run >= self.cfg.frames(self.cfg.hangover_ms) {
                    events.push(self.close(CloseReason::Silence));
                } else if self.open.as_ref().unwrap().audio.len() >= self.cfg.samples(self.cfg.soft_max_ms)
                    && self.cfg.soft_max_ms < self.cfg.hard_max_ms
                {
                    self.split_at_quietest(&mut events);
                }
            }
        }
        events
    }

    /// Closes any open utterance (on Stop).
    pub fn flush(&mut self) -> Option<SegEvent> {
        self.above = 0;
        self.open.as_ref()?;
        Some(self.close(CloseReason::Flush))
    }

    fn close(&mut self, reason: CloseReason) -> SegEvent {
        let open = self.open.take().expect("close needs an open utterance");
        let trail = self.cfg.samples(self.cfg.trail_ms.min(self.cfg.hangover_ms));
        let end = (open.last_speech_end + trail).min(open.audio.len());
        // Leftover silence seeds the next pre-roll; it never overlaps what we emit.
        self.recent.clear();
        let leftover = &open.audio[end..];
        let keep_from = leftover.len().saturating_sub(self.recent_cap);
        self.recent.extend(leftover[keep_from..].iter().copied());
        self.above = 0;

        let speech_ms = speech_ms(open.speech_start, open.last_speech_end);
        if speech_ms < self.cfg.min_speech_ms {
            return SegEvent::Discarded { start_sample: open.start_sample };
        }
        let mut samples = open.audio;
        samples.truncate(end);
        SegEvent::Closed(Utterance { start_sample: open.start_sample, samples, speech_ms, reason })
    }

    fn split_at_end(&mut self, events: &mut Vec<SegEvent>) {
        let open = self.open.as_mut().unwrap();
        let start_sample = open.start_sample;
        let samples = std::mem::take(&mut open.audio);
        let speech_ms = speech_ms(open.speech_start, open.last_speech_end);
        let new_start = start_sample + samples.len() as u64;
        open.energy.clear();
        open.start_sample = new_start;
        open.speech_start = 0;
        open.last_speech_end = 0;
        events.push(SegEvent::Closed(Utterance { start_sample, samples, speech_ms, reason: CloseReason::HardMax }));
        events.push(SegEvent::Opened { start_sample: new_start });
    }

    fn split_at_quietest(&mut self, events: &mut Vec<SegEvent>) {
        let fl = self.cfg.frame_len;
        let window = self.cfg.frames(self.cfg.quiet_window_ms).max(1);
        let search = self.cfg.frames(self.cfg.soft_search_ms);
        let open = self.open.as_mut().unwrap();
        let n = open.energy.len();
        let from = n.saturating_sub(search);
        let mut best = (from, f32::INFINITY);
        let mut i = from;
        while i + window <= n {
            let e: f32 = open.energy[i..i + window].iter().sum();
            if e < best.1 {
                best = (i, e);
            }
            i += 1;
        }
        let cut_frame = (best.0 + window / 2).clamp(1, n);
        let cut = cut_frame * fl;

        let start_sample = open.start_sample;
        let rest = open.audio.split_off(cut);
        let first = std::mem::replace(&mut open.audio, rest);
        let first_speech_end = open.last_speech_end.min(cut);
        let speech_ms = speech_ms(open.speech_start, first_speech_end);
        open.energy.drain(..cut_frame);
        open.start_sample = start_sample + cut as u64;
        open.speech_start = 0;
        open.last_speech_end = open.last_speech_end.saturating_sub(cut);
        let new_start = open.start_sample;
        events.push(SegEvent::Closed(Utterance {
            start_sample,
            samples: first,
            speech_ms,
            reason: CloseReason::SoftMax,
        }));
        events.push(SegEvent::Opened { start_sample: new_start });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FL: usize = 256;

    /// Feeds `secs` of frames with a constant score and amplitude; returns events.
    fn feed(seg: &mut Segmenter, secs: f32, score: f32, amp: f32) -> Vec<SegEvent> {
        let frames = (secs * SAMPLE_RATE as f32 / FL as f32).round() as usize;
        let frame = vec![amp; FL];
        let mut out = Vec::new();
        for _ in 0..frames {
            out.extend(seg.push_frame(&frame, score));
        }
        out
    }

    fn closed(events: &[SegEvent]) -> Vec<&Utterance> {
        events.iter().filter_map(|e| if let SegEvent::Closed(u) = e { Some(u) } else { None }).collect()
    }

    fn ms(samples: u64) -> i64 {
        (samples * 1000 / SAMPLE_RATE as u64) as i64
    }

    #[test]
    fn one_utterance_with_preroll_and_trail() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 1.0, 0.0, 0.0);
        ev.extend(feed(&mut seg, 1.0, 0.9, 0.3));
        ev.extend(feed(&mut seg, 1.0, 0.0, 0.0));
        let utts = closed(&ev);
        assert_eq!(utts.len(), 1);
        let u = utts[0];
        // Speech began at 1.0 s; pre-roll 320 ms.
        assert!((ms(u.start_sample) - 680).abs() <= 16, "start {}", ms(u.start_sample));
        // 320 ms pre-roll + 1 s speech + 320 ms trail.
        assert!((u.duration_ms() as i64 - 1640).abs() <= 32, "len {}", u.duration_ms());
        assert_eq!(u.reason, CloseReason::Silence);
        assert!(matches!(ev[0], SegEvent::Opened { .. }));
    }

    #[test]
    fn short_blip_is_discarded() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 0.5, 0.0, 0.0);
        ev.extend(feed(&mut seg, 0.2, 0.9, 0.3));
        ev.extend(feed(&mut seg, 1.0, 0.0, 0.0));
        assert!(closed(&ev).is_empty());
        assert!(ev.iter().any(|e| matches!(e, SegEvent::Discarded { .. })));
    }

    #[test]
    fn pause_shorter_than_hangover_keeps_one_utterance() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 1.0, 0.9, 0.3);
        ev.extend(feed(&mut seg, 0.5, 0.1, 0.0));
        ev.extend(feed(&mut seg, 1.0, 0.9, 0.3));
        ev.extend(feed(&mut seg, 1.0, 0.0, 0.0));
        assert_eq!(closed(&ev).len(), 1);
    }

    #[test]
    fn pause_longer_than_hangover_splits() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 1.0, 0.9, 0.3);
        ev.extend(feed(&mut seg, 0.8, 0.1, 0.0));
        ev.extend(feed(&mut seg, 1.0, 0.9, 0.3));
        ev.extend(feed(&mut seg, 1.0, 0.0, 0.0));
        let utts = closed(&ev);
        assert_eq!(utts.len(), 2);
        // Never overlapping.
        let first_end = utts[0].start_sample + utts[0].samples.len() as u64;
        assert!(utts[1].start_sample >= first_end);
    }

    #[test]
    fn hysteresis_band_keeps_utterance_open() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 0.5, 0.9, 0.3);
        ev.extend(feed(&mut seg, 2.0, 0.4, 0.3)); // between 0.35 and 0.5
        ev.extend(feed(&mut seg, 1.0, 0.0, 0.0));
        let utts = closed(&ev);
        assert_eq!(utts.len(), 1);
        assert!(utts[0].duration_ms() > 2400);
    }

    #[test]
    fn needs_three_consecutive_frames_to_open() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let hi = vec![0.3f32; FL];
        let mut ev = Vec::new();
        for _ in 0..100 {
            ev.extend(seg.push_frame(&hi, 0.9));
            ev.extend(seg.push_frame(&hi, 0.9));
            ev.extend(seg.push_frame(&hi, 0.1));
        }
        assert!(ev.is_empty());
        assert!(!seg.is_open());
    }

    #[test]
    fn preroll_is_clamped_at_stream_start() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 0.1, 0.0, 0.0);
        ev.extend(feed(&mut seg, 1.0, 0.9, 0.3));
        ev.extend(feed(&mut seg, 1.0, 0.0, 0.0));
        assert_eq!(closed(&ev)[0].start_sample, 0);
    }

    #[test]
    fn soft_max_cuts_at_quietest_window_in_last_three_seconds() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 13.0, 0.9, 0.3);
        ev.extend(feed(&mut seg, 0.1, 0.9, 0.001)); // a quiet dip at 13.0–13.1 s
        ev.extend(feed(&mut seg, 4.0, 0.9, 0.3));
        let utts = closed(&ev);
        assert_eq!(utts.len(), 1);
        assert_eq!(utts[0].reason, CloseReason::SoftMax);
        let end = ms(utts[0].start_sample + utts[0].samples.len() as u64);
        assert!((13_000..=13_120).contains(&end), "cut at {end}");
        assert!(seg.is_open());
        let (start, _) = seg.open_audio().unwrap();
        assert_eq!(start, utts[0].start_sample + utts[0].samples.len() as u64);
    }

    #[test]
    fn continuous_speech_never_exceeds_hard_max() {
        let cfg = SegmenterConfig { soft_max_ms: 30_000, ..Default::default() };
        let mut seg = Segmenter::new(cfg);
        let ev = feed(&mut seg, 60.0, 0.9, 0.3);
        let utts = closed(&ev);
        assert_eq!(utts.len(), 2);
        assert!(utts.iter().all(|u| u.duration_ms() <= 25_000));
        assert!(utts.iter().all(|u| u.reason == CloseReason::HardMax));
    }

    #[test]
    fn long_speech_with_default_soft_max_stays_under_hard_max() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut ev = feed(&mut seg, 61.0, 0.9, 0.3);
        if let Some(e) = seg.flush() {
            ev.push(e);
        }
        let utts = closed(&ev);
        assert!(utts.len() >= 4);
        assert!(utts.iter().all(|u| u.duration_ms() <= 15_000));
        // Contiguous coverage with no gaps or overlaps between pieces.
        for w in utts.windows(2) {
            assert_eq!(w[0].start_sample + w[0].samples.len() as u64, w[1].start_sample);
        }
    }

    #[test]
    fn flush_closes_open_utterance() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        feed(&mut seg, 1.0, 0.9, 0.3);
        match seg.flush() {
            Some(SegEvent::Closed(u)) => assert_eq!(u.reason, CloseReason::Flush),
            other => panic!("{other:?}"),
        }
        assert!(seg.flush().is_none());
    }
}
