//! `earshot`: pure Rust, streaming, 256-sample (16 ms) frames at 16 kHz.

use super::Vad;

pub const FRAME_LEN: usize = 256;

pub struct EarshotVad {
    detector: Box<earshot::Detector>,
    scratch: [f32; FRAME_LEN],
}

impl EarshotVad {
    pub fn new() -> Self {
        Self { detector: earshot::Detector::default_boxed(), scratch: [0.0; FRAME_LEN] }
    }
}

impl Default for EarshotVad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad for EarshotVad {
    fn frame_len(&self) -> usize {
        FRAME_LEN
    }

    fn score(&mut self, frame: &[f32]) -> f32 {
        debug_assert_eq!(frame.len(), FRAME_LEN);
        // The detector expects [-1, 1]; float capture and resampling can overshoot slightly.
        for (dst, src) in self.scratch.iter_mut().zip(frame) {
            *dst = src.clamp(-1.0, 1.0);
        }
        self.detector.predict_f32(&self.scratch).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        self.detector.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_scores_low() {
        let mut vad = EarshotVad::new();
        let frame = [0.0f32; FRAME_LEN];
        let mut last = 1.0;
        for _ in 0..20 {
            last = vad.score(&frame);
        }
        assert!(last < 0.2, "silence scored {last}");
    }

    #[test]
    fn overshooting_input_does_not_panic() {
        let mut vad = EarshotVad::new();
        let frame = [1.5f32; FRAME_LEN];
        let s = vad.score(&frame);
        assert!((0.0..=1.0).contains(&s));
    }
}
