//! Speech recognition: one Whisper worker, post-processing, and the echo guard.

pub mod echo_guard;
pub mod engine;
pub mod language;
pub mod oneshot;
pub mod params;
pub mod postprocess;
pub mod worker;
