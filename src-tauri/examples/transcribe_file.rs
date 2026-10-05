//! One-pass transcription of a 16 kHz mono WAV with the app's engine and parameters:
//! `cargo run --release --example transcribe_file -- <model.bin> <file.wav> [--cpu]`.
//! Prints timestamped segments and the real-time factor. This is the one-pass baseline the
//! integration harness compares streaming output against.

use std::time::Instant;

use kikitori_lib::asr::engine::Engine;
use kikitori_lib::asr::params::{self, DecodeOptions};
use kikitori_lib::audio::file_source::read_wav;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let model = &args[1];
    let wav = &args[2];
    let cpu = args.iter().any(|a| a == "--cpu");
    let t = Instant::now();
    let (engine, outcome) = Engine::load(std::path::Path::new(model), !cpu)?;
    println!("loaded in {:?} ({outcome:?}, device {:?})", t.elapsed(), engine.device_name);
    let (samples, rate, channels) = read_wav(std::path::Path::new(wav))?;
    anyhow::ensure!(rate == 16_000 && channels == 1, "need 16 kHz mono");
    let mut state = engine.create_state()?;
    let mut text_all = String::new();
    let t = Instant::now();
    // One pass over the whole file: whisper.cpp walks long audio in 30 s windows itself.
    let opts = DecodeOptions { threads: engine.threads(), accuracy_first: false, audio_ctx_experimental: false };
    let mut p = params::final_params(&opts, "ja", params::PRIMER, samples.len());
    // Long-form decoding keeps context between windows, as Whisper does for a file.
    p.set_no_context(false);
    state.full(p, &samples).map_err(|e| anyhow::anyhow!("{e}"))?;
    for s in 0..state.full_n_segments() {
        let seg = state.get_segment(s).unwrap();
        let t0 = seg.start_timestamp() as f64 / 100.0;
        let t1 = seg.end_timestamp() as f64 / 100.0;
        let text = seg.to_str_lossy()?.into_owned();
        println!("[{t0:7.2} - {t1:7.2}] {text}");
        text_all.push_str(&text);
    }
    let secs = samples.len() as f64 / 16_000.0;
    println!("audio {secs:.1}s, processed in {:?}, RTF {:.3}", t.elapsed(), t.elapsed().as_secs_f64() / secs);
    println!("TEXT: {text_all}");
    Ok(())
}
