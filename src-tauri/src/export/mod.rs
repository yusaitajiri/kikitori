//! Every output is built from the session timeline (section 11).

pub mod archive;
pub mod html;
pub mod markdown;
pub mod plaintext;
pub mod prompts;
pub mod saving;
pub mod typst_pdf;

use chrono::{DateTime, Duration, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::session::model::{Marker, MarkerKind, Screenshot, Session, SourceId, TimelineItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LabelMode {
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    pub timestamps: bool,
    pub labels: LabelMode,
    pub merge_paragraphs: bool,
    /// Plain-text copy only: include `[画像 HH:MM:SS]` lines.
    pub screenshot_markers: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { timestamps: true, labels: LabelMode::Auto, merge_paragraphs: true, screenshot_markers: true }
    }
}

impl ExportOptions {
    pub fn show_labels(&self, session: &Session) -> bool {
        match self.labels {
            LabelMode::On => true,
            LabelMode::Off => false,
            LabelMode::Auto => session.two_sides(),
        }
    }
}

const MERGE_GAP_MS: u64 = 2000;
const MERGE_MAX_CHARS: usize = 400;

/// One rendered block of the transcript, in display order.
#[derive(Debug, Clone, PartialEq)]
pub enum Block<'a> {
    /// `important`: the user marked its line (FR-07); such a line is never merged with others.
    Paragraph {
        t_ms: u64,
        source: SourceId,
        text: String,
        important: bool,
    },
    Screenshot(&'a Screenshot),
    Marker(&'a Marker),
}

/// Applies the display order and the paragraph merging rule.
pub fn blocks<'a>(session: &'a Session, opts: &ExportOptions) -> Vec<Block<'a>> {
    let mut out: Vec<Block<'a>> = Vec::new();
    // End time of the last segment merged into the open paragraph.
    let mut open_end: Option<u64> = None;
    for item in session.ordered() {
        match item {
            TimelineItem::Segment(seg) => {
                let text = seg.text.trim();
                if text.is_empty() {
                    continue;
                }
                if opts.merge_paragraphs
                    && !seg.important
                    && let (Some(end), Some(Block::Paragraph { source, text: para, important: false, .. })) =
                        (open_end, out.last_mut())
                    && *source == seg.source
                    && seg.t_start_ms.saturating_sub(end) < MERGE_GAP_MS
                    && para.chars().count() + text.chars().count() <= MERGE_MAX_CHARS
                {
                    *para = join_text(para, text);
                    open_end = Some(end.max(seg.t_end_ms));
                    continue;
                }
                out.push(Block::Paragraph {
                    t_ms: seg.t_start_ms,
                    source: seg.source,
                    text: text.to_string(),
                    important: seg.important,
                });
                open_end = Some(seg.t_end_ms);
            }
            TimelineItem::Screenshot(shot) => {
                out.push(Block::Screenshot(shot));
                open_end = None;
            }
            TimelineItem::Marker(marker) => {
                out.push(Block::Marker(marker));
                open_end = None;
            }
        }
    }
    out
}

fn is_latin_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric() || ('\u{00C0}'..='\u{024F}').contains(&c) && c.is_alphabetic()
}

/// Joins two pieces of transcript text. Japanese joins without a space; a space is inserted
/// only between Latin letters or digits (and after Latin sentence punctuation before a Latin
/// word, so English sessions read naturally).
pub fn join_text(left: &str, right: &str) -> String {
    let right = right.trim_start();
    let left = left.trim_end();
    let need_space = match (left.chars().last(), right.chars().next()) {
        (Some(l), Some(r)) => {
            is_latin_alnum(r) && (is_latin_alnum(l) || matches!(l, '.' | ',' | '!' | '?' | ';' | ':'))
        }
        _ => false,
    };
    if need_space { format!("{left} {right}") } else { format!("{left}{right}") }
}

/// Parses the session start; falls back to the Unix epoch so a bad value never panics.
pub fn session_start(session: &Session) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(&session.started_at)
        .unwrap_or_else(|_| DateTime::from_timestamp(0, 0).unwrap().fixed_offset())
}

/// The local time at a session offset: from the session's start, or from the start of the
/// continuation it falls in (FR-08).
pub fn wall_time(session: &Session, t_ms: u64) -> DateTime<FixedOffset> {
    let part = session.continued.iter().rev().find(|c| c.at_ms <= t_ms);
    let (start, at) = match part.and_then(|c| DateTime::parse_from_rfc3339(&c.started_at).ok().map(|d| (d, c.at_ms))) {
        Some(found) => found,
        None => (session_start(session), 0),
    };
    start + Duration::milliseconds((t_ms - at) as i64)
}

/// `HH:MM:SS` wall-clock time of a session offset.
pub fn clock(session: &Session, t_ms: u64) -> String {
    wall_time(session, t_ms).format("%H:%M:%S").to_string()
}

/// `HH:MM:SS` from a duration.
pub fn hms(duration_ms: u64) -> String {
    let s = duration_ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// `0001` from `img_0001`.
pub fn image_number(shot: &Screenshot) -> String {
    shot.id.strip_prefix("img_").unwrap_or(&shot.id).to_string()
}

/// Starts a line the user marked important (FR-07), in every text export; the agent prompt
/// says what it means.
pub const IMPORTANT: &str = "★ ";

pub fn marker_text(session: &Session, marker: &Marker) -> String {
    let time = clock(session, marker.t_ms);
    match marker.kind {
        MarkerKind::SourceReattached => format!("{time} 音声ソース再接続"),
        MarkerKind::Paused => format!("{time} 一時停止"),
        MarkerKind::Resumed => format!("{time} 再開"),
        MarkerKind::Cut => format!("{time} 区切り"),
        MarkerKind::SourceChanged => format!("{time} ソース変更: {}", marker.detail.as_deref().unwrap_or("?")),
        MarkerKind::Continued => {
            // The date only when the session went on another day.
            let day = wall_time(session, marker.t_ms).format("%Y-%m-%d").to_string();
            if day == session_start(session).format("%Y-%m-%d").to_string() {
                format!("{time} 続きを録音")
            } else {
                format!("{day} {time} 続きを録音")
            }
        }
        MarkerKind::Unprocessed => {
            format!("以降、未処理の音声 {} 秒", marker.detail.as_deref().unwrap_or("?"))
        }
    }
}

/// Description of a source for front matter: `相手 (Zoom)`, `自分 (マイク)`.
pub fn source_description(session: &Session) -> Vec<String> {
    session
        .sources
        .iter()
        .map(|s| {
            let what = match s.id {
                SourceId::Mic => "マイク".to_string(),
                SourceId::System => "システム全体".to_string(),
                SourceId::App => s.name.clone().or_else(|| s.exe.clone()).unwrap_or_else(|| "アプリ".to_string()),
            };
            format!("{} ({})", s.label, what)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::session::model::*;

    pub fn session(items: Vec<TimelineItem>, two_sources: bool) -> Session {
        let mut sources = vec![SourceInfo {
            id: SourceId::App,
            label: "相手".into(),
            exe: Some("Zoom.exe".into()),
            device: None,
            name: Some("Zoom".into()),
        }];
        if two_sources {
            sources.push(SourceInfo {
                id: SourceId::Mic,
                label: "自分".into(),
                exe: None,
                device: Some("Headset Microphone".into()),
                name: None,
            });
        }
        Session {
            v: 1,
            id: "01JABCDXYZ".into(),
            title: "Zoom".into(),
            started_at: "2026-10-02T15:13:05+09:00".into(),
            ended_at: Some("2026-10-02T16:15:20+09:00".into()),
            duration_ms: 3_735_000,
            sources,
            model: ModelRef { id: "turbo-q5".into(), sha256: "x".into() },
            language: "ja".into(),
            gpu: true,
            items,
            unprocessed_ms: 0,
            project: None,
            continued: Vec::new(),
        }
    }

    /// Times are seconds after 15:13:05.
    pub fn at(h: u64, m: u64, s: u64) -> u64 {
        ((h * 3600 + m * 60 + s) - (15 * 3600 + 13 * 60 + 5)) * 1000
    }

    pub fn meeting() -> Session {
        use crate::session::model::tests::{seg, shot};
        let mut items = vec![
            seg("seg_000001", SourceId::App, at(15, 15, 58), at(15, 16, 0), "それでは始めます。"),
            seg("seg_000002", SourceId::App, at(15, 16, 1), at(15, 16, 2) + 500, "次のスライドお願いします。"),
            shot("img_0001", at(15, 16, 3)),
            seg("seg_000003", SourceId::Mic, at(15, 16, 5), at(15, 16, 7), "よろしくお願いします。"),
        ];
        if let TimelineItem::Screenshot(s) = &mut items[2] {
            s.file = "images/0001_151603.png".into();
        }
        items.push(TimelineItem::Marker(Marker {
            id: "mk_0001".into(),
            t_ms: at(15, 40, 12),
            kind: MarkerKind::SourceReattached,
            detail: None,
        }));
        session(items, true)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::session::model::SourceInfo;
    use crate::session::model::tests::seg;

    #[test]
    fn join_rules() {
        assert_eq!(join_text("これは", "テストです"), "これはテストです");
        assert_eq!(join_text("Zoom", "meeting"), "Zoom meeting");
        assert_eq!(join_text("価格は100", "円"), "価格は100円");
        assert_eq!(join_text("version 2", "3"), "version 2 3");
        assert_eq!(join_text("It works.", "Next one"), "It works. Next one");
        assert_eq!(join_text("API", "を使う"), "APIを使う");
        assert_eq!(join_text("終わり。", "Next"), "終わり。Next");
    }

    #[test]
    fn merges_same_source_within_gap() {
        let s = meeting();
        let b = blocks(&s, &ExportOptions::default());
        assert_eq!(b.len(), 4);
        match &b[0] {
            Block::Paragraph { text, t_ms, .. } => {
                assert_eq!(text, "それでは始めます。次のスライドお願いします。");
                assert_eq!(clock(&s, *t_ms), "15:15:58");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_merge_when_disabled_or_gap_too_large() {
        let s = meeting();
        let opts = ExportOptions { merge_paragraphs: false, ..Default::default() };
        assert_eq!(blocks(&s, &opts).len(), 5);

        let s = session(
            vec![
                seg("seg_000001", SourceId::App, 0, 1000, "一。"),
                seg("seg_000002", SourceId::App, 3000, 4000, "二。"),
            ],
            false,
        );
        assert_eq!(blocks(&s, &ExportOptions::default()).len(), 2);
    }

    /// A meeting whose second line (merged into the first otherwise) is marked important.
    fn meeting_with_important_line() -> Session {
        let mut s = meeting();
        if let Some(TimelineItem::Segment(seg)) = s.items.get_mut(1) {
            seg.important = true;
        }
        s
    }

    #[test]
    fn an_important_line_stands_alone() {
        let s = meeting_with_important_line();
        let b = blocks(&s, &ExportOptions::default());
        let paragraphs: Vec<(&str, bool)> = b
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph { text, important, .. } => Some((text.as_str(), *important)),
                _ => None,
            })
            .collect();
        assert_eq!(
            paragraphs,
            [("それでは始めます。", false), ("次のスライドお願いします。", true), ("よろしくお願いします。", false)]
        );
        let plain = plaintext::plain(&s, &ExportOptions::default());
        assert!(
            plain.contains(
                "
★ [15:16:01] 相手: 次のスライドお願いします。
"
            ),
            "{plain}"
        );
        let md = markdown::body(&s, &ExportOptions::default());
        assert!(
            md.contains(
                "

★ **[15:16:01] 相手:** 次のスライドお願いします。

"
            ),
            "{md}"
        );
    }

    #[test]
    fn a_continuation_keeps_clock_times_true() {
        let mut s = meeting();
        // Continued the next morning, after the session's last second (01:02:15).
        s.continued.push(crate::session::model::Continuation {
            at_ms: 3_735_100,
            started_at: "2026-10-03T09:00:00+09:00".into(),
        });
        assert_eq!(clock(&s, at(15, 16, 1)), "15:16:01");
        assert_eq!(clock(&s, 3_735_100), "09:00:00");
        assert_eq!(clock(&s, 3_735_100 + 61_000), "09:01:01");
        let marker = Marker { id: "mk_0002".into(), t_ms: 3_735_100, kind: MarkerKind::Continued, detail: None };
        assert_eq!(marker_text(&s, &marker), "2026-10-03 09:00:00 続きを録音");
    }

    #[test]
    fn labels_follow_both_sides_not_the_number_of_sources() {
        // Switched from one app to another (FR-17): two sources, one side, so no labels.
        let mut s = session(vec![seg("seg_000001", SourceId::App, 0, 1000, "一。")], false);
        s.sources.push(SourceInfo {
            id: SourceId::App,
            label: "相手".into(),
            exe: None,
            device: None,
            name: Some("Teams".into()),
        });
        assert!(!ExportOptions::default().show_labels(&s));
        assert!(ExportOptions::default().show_labels(&meeting()));
        let marker =
            Marker { id: "mk_0002".into(), t_ms: 0, kind: MarkerKind::SourceChanged, detail: Some("Teams".into()) };
        assert_eq!(marker_text(&s, &marker), "15:13:05 ソース変更: Teams");
    }

    #[test]
    fn merge_stops_at_four_hundred_characters() {
        let long = "あ".repeat(250);
        let s = session(
            vec![seg("seg_000001", SourceId::App, 0, 1000, &long), seg("seg_000002", SourceId::App, 1500, 2000, &long)],
            false,
        );
        assert_eq!(blocks(&s, &ExportOptions::default()).len(), 2);
    }

    #[test]
    fn hms_formats() {
        assert_eq!(hms(3_735_000), "01:02:15");
        assert_eq!(hms(59_999), "00:00:59");
    }
}
