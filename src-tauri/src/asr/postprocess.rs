//! Post-processing of Whisper output (section 8, steps 1–4).

use std::sync::LazyLock;

use unicode_normalization::UnicodeNormalization;

static HALLUCINATIONS: LazyLock<Vec<String>> = LazyLock::new(|| {
    include_str!("hallucinations_ja.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(normalize_for_match)
        .collect()
});

/// What happened to a segment.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Keep(String),
    Drop(DropReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    Empty,
    NoSpeech,
    SymbolsOnly,
    Hallucination,
    Repetition,
}

impl DropReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DropReason::Empty => "empty",
            DropReason::NoSpeech => "no_speech",
            DropReason::SymbolsOnly => "symbols_only",
            DropReason::Hallucination => "hallucination_phrase",
            DropReason::Repetition => "hallucination_repeat",
        }
    }
}

/// Facts about the utterance a segment came from.
#[derive(Debug, Clone, Copy)]
pub struct UtteranceStats {
    pub duration_ms: u64,
    pub rms_dbfs: f32,
}

pub struct PostProcessor {
    pub hallucination_filter: bool,
}

impl PostProcessor {
    pub fn process(&self, raw: &str, no_speech: f32, utt: UtteranceStats) -> Outcome {
        let text = tidy_spaces(raw);
        if text.is_empty() {
            return Outcome::Drop(DropReason::Empty);
        }
        if no_speech >= 0.8 {
            return Outcome::Drop(DropReason::NoSpeech);
        }
        if is_symbols_only(&text) {
            return Outcome::Drop(DropReason::SymbolsOnly);
        }
        if self.hallucination_filter
            && matches_hallucination(&text)
            && (utt.duration_ms < 4000 || utt.rms_dbfs < -40.0 || no_speech > 0.3)
        {
            return Outcome::Drop(DropReason::Hallucination);
        }
        let (collapsed, removed_ratio) = collapse_repeats(&text);
        if removed_ratio > 0.6 {
            return Outcome::Drop(DropReason::Repetition);
        }
        Outcome::Keep(collapsed)
    }
}

pub fn is_japanese(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{303F}' // CJK symbols and punctuation
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{31F0}'..='\u{31FF}' // Katakana extensions
        | '\u{3400}'..='\u{4DBF}' // CJK extension A
        | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
        | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
        | '\u{FF01}'..='\u{FF60}' // Full-width forms
        | '\u{FF61}'..='\u{FF9F}' // Half-width katakana
    )
}

/// Trims, collapses runs of whitespace, and removes spaces between two Japanese characters.
/// Spaces next to Latin words stay.
pub fn tidy_spaces(raw: &str) -> String {
    let chars: Vec<char> = raw.trim().chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let prev = out.chars().last();
            let next = chars.get(j).copied();
            let drop = matches!((prev, next), (Some(p), Some(n)) if is_japanese(p) && is_japanese(n));
            if !drop {
                out.push(' ');
            }
            i = j;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// True when nothing but symbols, punctuation and bracketed sound labels remains, e.g. `♪`,
/// `(音楽)`, `[拍手]`.
pub fn is_symbols_only(text: &str) -> bool {
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '(' | '（' | '[' | '［' | '【' | '〔' | '<' | '＜' => depth += 1,
            ')' | '）' | ']' | '］' | '】' | '〕' | '>' | '＞' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            c if c.is_alphanumeric() && !matches!(c, '♪' | '♫' | '♬') => return false,
            _ => {}
        }
    }
    true
}

/// NFKC, lowercase, punctuation, symbols and spaces stripped.
pub fn normalize_for_match(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

pub fn matches_hallucination(text: &str) -> bool {
    let norm = normalize_for_match(text);
    !norm.is_empty() && HALLUCINATIONS.iter().any(|p| norm == *p || norm.starts_with(p.as_str()))
}

/// Collapses any 1–10 character unit repeated 4+ times in a row down to 3 repeats.
/// Returns the new text and the fraction of characters removed.
pub fn collapse_repeats(text: &str) -> (String, f32) {
    let original: Vec<char> = text.chars().collect();
    let mut chars = original.clone();
    loop {
        let mut changed = false;
        for unit in 1..=10usize {
            let mut i = 0;
            while i + unit * 4 <= chars.len() {
                let mut count = 1;
                while i + (count + 1) * unit <= chars.len()
                    && chars[i + count * unit..i + (count + 1) * unit] == chars[i..i + unit]
                {
                    count += 1;
                }
                if count >= 4 {
                    chars.drain(i + 3 * unit..i + count * unit);
                    changed = true;
                    i += 3 * unit;
                } else {
                    i += 1;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let removed = if original.is_empty() { 0.0 } else { (original.len() - chars.len()) as f32 / original.len() as f32 };
    (chars.into_iter().collect(), removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG_LOUD: UtteranceStats = UtteranceStats { duration_ms: 8000, rms_dbfs: -20.0 };
    const SHORT: UtteranceStats = UtteranceStats { duration_ms: 2000, rms_dbfs: -20.0 };
    const QUIET: UtteranceStats = UtteranceStats { duration_ms: 8000, rms_dbfs: -45.0 };

    fn pp() -> PostProcessor {
        PostProcessor { hallucination_filter: true }
    }

    #[test]
    fn removes_spaces_between_japanese_only() {
        assert_eq!(tidy_spaces(" これは テスト です。 "), "これはテストです。");
        assert_eq!(tidy_spaces("Zoom の 設定を 開きます"), "Zoom の設定を開きます");
        assert_eq!(tidy_spaces("API  key を 入力"), "API key を入力");
        assert_eq!(tidy_spaces("hello   world"), "hello world");
    }

    #[test]
    fn symbols_only_detection() {
        assert!(is_symbols_only("♪"));
        assert!(is_symbols_only("(音楽)"));
        assert!(is_symbols_only("[拍手]"));
        assert!(is_symbols_only("（拍手） ♪～"));
        assert!(!is_symbols_only("(笑) そうですね"));
        assert!(!is_symbols_only("はい。"));
    }

    #[test]
    fn no_speech_threshold() {
        assert_eq!(pp().process("こんにちは", 0.85, LONG_LOUD), Outcome::Drop(DropReason::NoSpeech));
        assert_eq!(pp().process("こんにちは", 0.5, LONG_LOUD), Outcome::Keep("こんにちは".into()));
    }

    #[test]
    fn hallucination_needs_a_suspicious_condition() {
        let text = "ご視聴ありがとうございました。";
        assert_eq!(pp().process(text, 0.1, SHORT), Outcome::Drop(DropReason::Hallucination));
        assert_eq!(pp().process(text, 0.1, QUIET), Outcome::Drop(DropReason::Hallucination));
        assert_eq!(pp().process(text, 0.4, LONG_LOUD), Outcome::Drop(DropReason::Hallucination));
        // Long, loud, confident: keep it, someone may really have said it.
        assert_eq!(pp().process(text, 0.1, LONG_LOUD), Outcome::Keep(text.into()));
    }

    #[test]
    fn hallucination_prefix_and_normalization() {
        assert!(matches_hallucination("ご視聴ありがとうございました！また来週"));
        assert!(matches_hallucination("Ｔｈａｎｋｓ　ｆｏｒ　ｗａｔｃｈｉｎｇ！"));
        assert!(matches_hallucination("Thanks for watching!"));
        assert!(matches_hallucination("thank you for watching."));
        assert!(!matches_hallucination("ご清聴ありがとうございました"));
        assert!(!matches_hallucination("視聴者の皆さん"));
    }

    #[test]
    fn filter_can_be_disabled() {
        let p = PostProcessor { hallucination_filter: false };
        assert_eq!(
            p.process("ご視聴ありがとうございました", 0.1, SHORT),
            Outcome::Keep("ご視聴ありがとうございました".into())
        );
    }

    #[test]
    fn collapses_repeated_units_to_three() {
        assert_eq!(collapse_repeats("ああああああ").0, "あああ");
        assert_eq!(collapse_repeats("はいはいはいはいはい").0, "はいはいはい");
        assert_eq!(
            collapse_repeats("それでは、それでは、それでは、それでは、始めます").0,
            "それでは、それでは、それでは、始めます"
        );
        assert_eq!(collapse_repeats("はいはいはい").0, "はいはいはい");
        assert_eq!(collapse_repeats("普通の文です。").0, "普通の文です。");
    }

    #[test]
    fn mostly_repetition_is_dropped() {
        let text = "ありがとう".repeat(20);
        assert_eq!(pp().process(&text, 0.1, LONG_LOUD), Outcome::Drop(DropReason::Repetition));
        let ok = format!("{}{}", "はい".repeat(5), "わかりました、それでは次の議題に進みましょう。");
        match pp().process(&ok, 0.1, LONG_LOUD) {
            Outcome::Keep(t) => assert!(t.starts_with("はいはいはいわかりました")),
            other => panic!("{other:?}"),
        }
    }
}
