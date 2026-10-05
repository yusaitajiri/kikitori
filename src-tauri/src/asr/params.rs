//! Whisper parameters (section 8) and the prompt.

use whisper_rs::{FullParams, SamplingStrategy};

pub const PRIMER: &str = "えー、それでは始めます。よろしくお願いします。";
pub const PROMPT_MAX_CHARS: usize = 120;
pub const CONTEXT_MAX_CHARS: usize = 60;

#[derive(Debug, Clone, Copy)]
pub struct DecodeOptions {
    pub threads: i32,
    pub accuracy_first: bool,
    pub audio_ctx_experimental: bool,
}

/// Word list joined with 「、」 and ended with 「。」, then the last accepted segment from the
/// same source (or the primer), kept under 120 characters.
pub fn build_prompt(vocabulary: &[String], last_accepted: Option<&str>, language: &str) -> String {
    let context: String = match last_accepted.map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => {
            let chars: Vec<char> = text.chars().collect();
            chars[chars.len().saturating_sub(CONTEXT_MAX_CHARS)..].iter().collect()
        }
        // The Japanese primer nudges Whisper towards 、 and 。; it only helps Japanese.
        None if language == "ja" => PRIMER.to_string(),
        None => String::new(),
    };
    let budget = (PROMPT_MAX_CHARS - 1).saturating_sub(context.chars().count());
    let mut vocab = String::new();
    for term in vocabulary.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let candidate = if vocab.is_empty() { term.to_string() } else { format!("{vocab}、{term}") };
        // +1 for the closing 「。」
        if candidate.chars().count() + 1 > budget {
            break;
        }
        vocab = candidate;
    }
    if !vocab.is_empty() {
        vocab.push('。');
    }
    format!("{vocab}{context}")
}

/// The word list alone, for clips too short to carry context safely.
pub fn vocabulary_prompt(vocabulary: &[String]) -> String {
    build_prompt(vocabulary, None, "")
}

/// Encoder frames for a clip when the experimental `audio_ctx` setting is on (1500 = 30 s).
pub fn audio_ctx_for(samples: usize) -> i32 {
    let secs = samples as f64 / 16_000.0;
    ((secs * 50.0).ceil() as i32 + 64).clamp(256, 1500)
}

pub fn final_params<'a, 'b>(
    opts: &DecodeOptions,
    language: &'a str,
    prompt: &str,
    samples: usize,
) -> FullParams<'a, 'b> {
    let strategy = if opts.accuracy_first {
        SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 }
    } else {
        SamplingStrategy::Greedy { best_of: 1 }
    };
    let mut p = FullParams::new(strategy);
    p.set_n_threads(opts.threads);
    p.set_language(Some(language));
    p.set_translate(false);
    p.set_no_context(true);
    p.set_no_timestamps(false);
    p.set_single_segment(false);
    if !prompt.is_empty() {
        p.set_initial_prompt(prompt);
    }
    p.set_temperature(0.0);
    p.set_temperature_inc(0.2);
    p.set_entropy_thold(2.4);
    p.set_logprob_thold(-1.0);
    p.set_no_speech_thold(0.6);
    p.set_suppress_blank(true);
    p.set_suppress_nst(true);
    p.enable_vad(false);
    quiet(&mut p);
    if opts.audio_ctx_experimental {
        p.set_audio_ctx(audio_ctx_for(samples));
    }
    p
}

/// Partial (provisional) text: greedy, no prompt, one segment, no timestamps.
pub fn partial_params<'a, 'b>(threads: i32, language: &'a str) -> FullParams<'a, 'b> {
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    p.set_n_threads(threads);
    p.set_language(Some(language));
    p.set_translate(false);
    p.set_no_context(true);
    p.set_single_segment(true);
    p.set_no_timestamps(true);
    p.set_temperature(0.0);
    p.set_temperature_inc(0.0);
    p.set_suppress_blank(true);
    p.set_suppress_nst(true);
    p.enable_vad(false);
    quiet(&mut p);
    p
}

fn quiet(p: &mut FullParams) {
    p.set_print_special(false);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
}

/// CPU: physical cores capped at 8. GPU: 4.
pub fn thread_count(gpu: bool) -> i32 {
    if gpu {
        return 4;
    }
    let cores = sysinfo::System::physical_core_count()
        .or_else(|| std::thread::available_parallelism().ok().map(|n| n.get() / 2))
        .unwrap_or(4);
    cores.clamp(1, 8) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primer_when_nothing_accepted_yet() {
        assert_eq!(build_prompt(&[], None, "ja"), PRIMER);
        assert_eq!(build_prompt(&[], None, "en"), "");
    }

    #[test]
    fn vocabulary_then_context() {
        let p = build_prompt(&["大澤研".into(), "Kikitori".into()], Some("前の文です。"), "ja");
        assert_eq!(p, "大澤研、Kikitori。前の文です。");
    }

    #[test]
    fn context_is_last_sixty_characters() {
        let long = format!("{}{}", "あ".repeat(50), "い".repeat(60));
        let p = build_prompt(&[], Some(&long), "ja");
        assert_eq!(p, "い".repeat(60));
    }

    #[test]
    fn stays_under_120_characters() {
        let vocab: Vec<String> = (0..50).map(|i| format!("用語{i:02}")).collect();
        let p = build_prompt(&vocab, Some(&"う".repeat(80)), "ja");
        assert!(p.chars().count() < PROMPT_MAX_CHARS, "{}", p.chars().count());
        assert!(p.ends_with(&"う".repeat(60)));
        assert!(p.starts_with("用語00、用語01"));
    }

    #[test]
    fn vocabulary_only_prompt_has_no_primer_or_context() {
        assert_eq!(vocabulary_prompt(&[]), "");
        assert_eq!(vocabulary_prompt(&["大澤研".into()]), "大澤研。");
    }

    #[test]
    fn audio_ctx_bounds() {
        assert_eq!(audio_ctx_for(16_000 * 30), 1500);
        assert_eq!(audio_ctx_for(16_000), 256);
        assert_eq!(audio_ctx_for(16_000 * 10), 564);
    }
}
