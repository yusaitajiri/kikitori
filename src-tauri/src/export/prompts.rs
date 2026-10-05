//! The cleanup prompt (section 11) that 「Claude用にコピー」 puts before the transcript.

/// Cleanup rules; `{vocabulary}` and `{transcript}` are substituted.
pub const CLEANUP_TEMPLATE: &str =
    "以下は音声認識で自動生成した文字起こしです。誤字・誤変換・句読点を直して、読みやすい日本語に整えてください。
ルール:
- 内容を要約・省略・追加しない
- 時刻と話者ラベル（自分／相手）はそのまま残す
- [画像:0001 15:16:03] のような行は一字も変えずに残す
- 聞き取れない箇所は推測せず「（不明）」と書く
- 固有名詞の候補: {vocabulary}
---
{transcript}";

/// Shown in place of an empty word list.
const NO_VOCABULARY: &str = "（なし）";

pub fn vocabulary_line(vocabulary: &[String]) -> String {
    let terms: Vec<&str> = vocabulary.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    if terms.is_empty() { NO_VOCABULARY.to_string() } else { terms.join("、") }
}

/// The full prompt for pasting into claude.ai.
pub fn cleanup_prompt(vocabulary: &[String], transcript: &str) -> String {
    CLEANUP_TEMPLATE.replace("{vocabulary}", &vocabulary_line(vocabulary)).replace("{transcript}", transcript)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_vocabulary_and_transcript() {
        let p = cleanup_prompt(&["大澤研".into(), " Kikitori ".into()], "[15:15:58] 相手: こんにちは");
        assert!(p.contains("- 固有名詞の候補: 大澤研、Kikitori\n---\n[15:15:58] 相手: こんにちは"));
        assert!(p.starts_with("以下は音声認識で自動生成した文字起こしです。"));
    }

    #[test]
    fn empty_vocabulary_reads_none() {
        let p = cleanup_prompt(&[], "x");
        assert!(p.contains("固有名詞の候補: （なし）"));
    }
}
