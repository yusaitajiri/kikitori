//! Echo guard (FR-16): drops 自分 lines that are the speakers leaking into the mic.
//!
//! The spec's test is the character-bigram Dice coefficient between overlapping lines. The two
//! streams are cut by separate voice detectors, though, so an echo line is often a fragment of
//! one 相手 line or spans several of them, and Dice punishes the length difference. A 自分 line
//! is therefore also an echo when most of its bigrams appear in the 相手 text overlapping it.

use std::collections::HashMap;

use super::postprocess::normalize_for_match;

pub const WINDOW_MS: u64 = 1500;
pub const THRESHOLD: f32 = 0.6;

#[derive(Debug, Clone)]
pub struct Line<'a> {
    pub id: &'a str,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub text: &'a str,
}

fn bigrams(chars: &[char]) -> HashMap<(char, char), i32> {
    let mut grams: HashMap<(char, char), i32> = HashMap::new();
    for w in chars.windows(2) {
        *grams.entry((w[0], w[1])).or_default() += 1;
    }
    grams
}

fn shared(a: &[char], b: &[char]) -> usize {
    let mut grams = bigrams(a);
    let mut n = 0;
    for w in b.windows(2) {
        if let Some(c) = grams.get_mut(&(w[0], w[1]))
            && *c > 0
        {
            *c -= 1;
            n += 1;
        }
    }
    n
}

/// Character-bigram Dice coefficient of two normalized texts.
pub fn dice(a: &str, b: &str) -> f32 {
    let a: Vec<char> = normalize_for_match(a).chars().collect();
    let b: Vec<char> = normalize_for_match(b).chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a.len() < 2 || b.len() < 2 {
        return if a == b { 1.0 } else { 0.0 };
    }
    2.0 * shared(&a, &b) as f32 / ((a.len() - 1) + (b.len() - 1)) as f32
}

/// The fraction of `part`'s bigrams that also occur in `whole`.
pub fn containment(part: &str, whole: &str) -> f32 {
    let p: Vec<char> = normalize_for_match(part).chars().collect();
    let w: Vec<char> = normalize_for_match(whole).chars().collect();
    if p.is_empty() || w.is_empty() {
        return 0.0;
    }
    if p.len() < 2 {
        return if w.contains(&p[0]) && w.len() == 1 { 1.0 } else { 0.0 };
    }
    shared(&w, &p) as f32 / (p.len() - 1) as f32
}

fn overlaps(a: &Line, b: &Line) -> bool {
    a.t_start_ms <= b.t_end_ms + WINDOW_MS && b.t_start_ms <= a.t_end_ms + WINDOW_MS
}

/// How much a 自分 line looks like the 相手 lines around it (0..1).
pub fn echo_score(mine: &Line, others: &[Line]) -> f32 {
    let near: Vec<&Line> = others.iter().filter(|o| overlaps(o, mine)).collect();
    if near.is_empty() {
        return 0.0;
    }
    let pairwise = near.iter().map(|o| dice(o.text, mine.text)).fold(0.0, f32::max);
    let joined: String = near.iter().map(|o| o.text).collect();
    pairwise.max(containment(mine.text, &joined))
}

/// True when a 自分 line echoes the 相手 lines overlapping it.
pub fn is_echo_of_any(mine: &Line, others: &[Line]) -> bool {
    echo_score(mine, others) >= THRESHOLD
}

/// A 相手 line was finalized: the 自分 lines near it that now look like echoes, judged against
/// every 相手 line overlapping them (which `others` must include, the new line too).
pub fn echoes_of_others<'a>(new_line: &Line, mine: &[Line<'a>], others: &[Line]) -> Vec<&'a str> {
    mine.iter().filter(|m| overlaps(new_line, m) && is_echo_of_any(m, others)).map(|m| m.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line<'a>(id: &'a str, s: u64, e: u64, text: &'a str) -> Line<'a> {
        Line { id, t_start_ms: s, t_end_ms: e, text }
    }

    #[test]
    fn dice_basics() {
        assert_eq!(dice("こんにちは", "こんにちは"), 1.0);
        assert_eq!(dice("こんにちは。", "こんにちは"), 1.0);
        assert!(dice("次のスライドお願いします", "次のスライドをお願いします") > 0.8);
        assert!(dice("今日は晴れです", "明日の会議は十時から") < 0.2);
        assert_eq!(dice("", "a"), 0.0);
    }

    #[test]
    fn removes_similar_overlapping_mine() {
        let others = [line("seg_1", 10_000, 13_000, "それでは資料を共有します")];
        let mine = [
            line("seg_2", 10_300, 13_200, "それでは資料を共有します"),
            line("seg_3", 10_500, 12_000, "了解です"),
            line("seg_4", 30_000, 33_000, "それでは資料を共有します"),
        ];
        assert_eq!(echoes_of_others(&others[0], &mine, &others), vec!["seg_2"]);
    }

    #[test]
    fn window_allows_one_and_a_half_seconds() {
        let others = [line("seg_1", 10_000, 12_000, "はい分かりました")];
        assert!(is_echo_of_any(&line("seg_2", 13_400, 14_000, "はい分かりました"), &others));
        assert!(!is_echo_of_any(&line("seg_3", 13_600, 14_000, "はい分かりました"), &others));
    }

    // Seen in the two-channel fixture: the mic's detector cut a fragment of one 相手 line.
    #[test]
    fn fragment_of_a_longer_line_is_an_echo() {
        let others = [line("a", 34_832, 38_452, "おりゃこのごろとても不思議なことがあるんだ。何が?")];
        assert!(dice(others[0].text, "何が?") < THRESHOLD);
        assert!(is_echo_of_any(&line("m", 37_532, 38_512, "何が?"), &others));
    }

    // Also seen there: one mic line spanning three 相手 lines.
    #[test]
    fn line_spanning_several_others_is_an_echo_once_they_all_arrived() {
        let mine = [line(
            "m",
            84_096,
            97_776,
            "ポンポンポンと木魚の音がしています。窓の障子に明かりがさしていて、大きな坊主頭が映って動いていました。ゴンは、お念仏があるんだなと思いながら",
        )];
        let first = [line("a1", 84_048, 87_184, "ポンポンポンと木魚の音がしています。")];
        assert!(echoes_of_others(&first[0], &mine, &first).is_empty());
        let all = [
            first[0].clone(),
            line("a2", 87_296, 92_216, "窓の障子に明かりがさしていて、大きな坊主頭が映って動いていました。"),
            line("a3", 92_216, 97_736, "ゴンは、お念仏があるんだなと思いながら、井戸のそばにしゃがんでいました。"),
        ];
        assert_eq!(echoes_of_others(&all[2], &mine, &all), vec!["m"]);
    }

    #[test]
    fn different_words_at_the_same_time_are_kept() {
        let others = [line("a", 10_000, 15_000, "それでは来週の予定を確認しましょう")];
        assert!(!is_echo_of_any(&line("m", 11_000, 13_000, "了解です、資料を送ります"), &others));
    }

    #[test]
    fn containment_basics() {
        assert_eq!(containment("何が", "何がどうした"), 1.0);
        assert!(containment("全然ちがう話", "何がどうした") < 0.2);
        assert_eq!(containment("", "x"), 0.0);
    }
}
