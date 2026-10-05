//! Keeps Whisper from translating. Told a language the speech is not in, Whisper translates the
//! speech into it or repeats itself: with 日本語 set, English speech came out in Japanese, above
//! all in the grey partial text, which has no prompt to steer it. So after each decode the worker
//! reads which language Whisper hears (`Engine::languages`) and, when Whisper is sure the clip is
//! in the other of Japanese and English, decodes it again in that one. The source's next clips
//! start in that language until Whisper stops hearing it.

/// How sure Whisper must be that a clip is in the other of Japanese and English before the clip
/// is decoded in that one. With large-v3-turbo and base, real Japanese speech never scored above
/// 0.74 for English (0.6 s fragments and silence score highest), and English speech never below
/// 0.9 (0.6 s clips with base; turbo scores 0.999 and up).
pub const SWITCH: f32 = 0.9;

/// Once a source has switched away from the configured language, how sure Whisper must stay that
/// a clip is in the language it switched to; below this, the clip goes back to the configured one.
pub const STAY: f32 = 0.5;

/// The language to decode a clip in again, if any, after decoding it in `tried`. `home` is the
/// configured language (or the one 自動 locked) and `p` gives how sure Whisper is of a language:
/// the other of Japanese and English when Whisper is sure of it, else `home` when the clip was
/// decoded in a language its source switched to and Whisper no longer hears that.
pub fn redo_in(home: &str, tried: &str, p: impl Fn(&str) -> f32) -> Option<String> {
    if let Some(lang) = ["ja", "en"].into_iter().find(|&l| l != tried && p(l) >= SWITCH) {
        return Some(lang.to_string());
    }
    (tried != home && p(tried) < STAY).then(|| home.to_string())
}

/// Language-token logits as probabilities, as whisper.cpp's own detection computes them.
pub fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    exp.into_iter().map(|e| e / sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(ja: f32, en: f32) -> impl Fn(&str) -> f32 {
        move |l| match l {
            "ja" => ja,
            "en" => en,
            _ => 0.0,
        }
    }

    #[test]
    fn decodes_again_in_the_other_language_only_when_sure() {
        assert_eq!(redo_in("ja", "ja", heard(0.0, 1.0)), Some("en".into()));
        assert_eq!(redo_in("ja", "ja", heard(0.1, 0.74)), None);
        assert_eq!(redo_in("en", "en", heard(0.95, 0.02)), Some("ja".into()));
        assert_eq!(redo_in("ja", "ja", heard(0.99, 0.0)), None);
    }

    #[test]
    fn stays_switched_while_whisper_still_hears_it() {
        assert_eq!(redo_in("ja", "en", heard(0.2, 0.6)), None);
        assert_eq!(redo_in("ja", "en", heard(0.95, 0.01)), Some("ja".into()));
        // Unsure, and not of English any more: back to the configured language.
        assert_eq!(redo_in("ja", "en", heard(0.4, 0.3)), Some("ja".into()));
    }

    #[test]
    fn a_locked_language_switches_like_a_configured_one() {
        assert_eq!(redo_in("ko", "ko", heard(0.0, 0.97)), Some("en".into()));
        assert_eq!(redo_in("ko", "en", heard(0.0, 0.1)), Some("ko".into()));
    }

    #[test]
    fn softmax_sums_to_one_and_keeps_the_order() {
        let p = softmax(&[2.0, 0.0, -1.0]);
        assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(p[0] > p[1] && p[1] > p[2]);
    }
}
