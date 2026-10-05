//! Voice activity detection behind a trait, so whisper.cpp's Silero VAD can be swapped in.

pub mod earshot_vad;

pub use earshot_vad::EarshotVad;

pub trait Vad: Send {
    /// Samples per frame at 16 kHz.
    fn frame_len(&self) -> usize;
    /// Speech probability for one frame, 0..1.
    fn score(&mut self, frame: &[f32]) -> f32;
    fn reset(&mut self);
}
