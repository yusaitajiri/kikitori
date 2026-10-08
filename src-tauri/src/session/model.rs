//! Session snapshot types (`session.json`) and the timeline ordering rule (section 9).

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceId {
    Mic,
    App,
    System,
}

impl SourceId {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceId::Mic => "mic",
            SourceId::App => "app",
            SourceId::System => "system",
        }
    }

    /// 相手 sorts before 自分 on ties.
    fn tie_rank(self) -> u8 {
        match self {
            SourceId::App | SourceId::System => 0,
            SourceId::Mic => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub id: SourceId,
    /// "自分" or "相手". Stored so exports read the same in any UI locale.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Display name of the app ("Zoom"); the spec's examples leave it out, so it is optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub source: SourceId,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_original: Option<String>,
    #[serde(default)]
    pub edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utterance_id: Option<String>,
    /// Marked important by the user (FR-07), while recording or after.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub important: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    pub id: String,
    pub t_ms: u64,
    /// Relative to the session folder, forward slashes.
    pub file: String,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    Paused,
    Resumed,
    SourceReattached,
    Unprocessed,
    /// The user started a new part of the session (a new topic or scene).
    Cut,
    /// The user switched what is recorded (FR-17); `detail` names the new source.
    SourceChanged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub id: String,
    pub t_ms: u64,
    #[serde(rename = "type")]
    pub kind: MarkerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TimelineItem {
    Segment(Segment),
    Screenshot(Screenshot),
    Marker(Marker),
}

impl TimelineItem {
    pub fn id(&self) -> &str {
        match self {
            TimelineItem::Segment(s) => &s.id,
            TimelineItem::Screenshot(s) => &s.id,
            TimelineItem::Marker(m) => &m.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub v: u32,
    pub id: String,
    pub title: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub duration_ms: u64,
    pub sources: Vec<SourceInfo>,
    pub model: ModelRef,
    pub language: String,
    pub gpu: bool,
    /// Stored by time; display order comes from [`order_timeline`].
    pub items: Vec<TimelineItem>,
    /// Audio that was never transcribed because the user cancelled finishing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unprocessed_ms: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

impl Session {
    pub fn segments(&self) -> impl Iterator<Item = &Segment> {
        self.items.iter().filter_map(|i| match i {
            TimelineItem::Segment(s) => Some(s),
            _ => None,
        })
    }

    pub fn screenshots(&self) -> impl Iterator<Item = &Screenshot> {
        self.items.iter().filter_map(|i| match i {
            TimelineItem::Screenshot(s) => Some(s),
            _ => None,
        })
    }

    /// Whether both sides were recorded at some point: 相手 (an app or the system) and 自分 (the mic).
    pub fn two_sides(&self) -> bool {
        self.sources.iter().any(|s| s.id == SourceId::Mic) && self.sources.iter().any(|s| s.id != SourceId::Mic)
    }

    pub fn label_for(&self, source: SourceId) -> &str {
        self.sources.iter().find(|s| s.id == source).map(|s| s.label.as_str()).unwrap_or(match source {
            SourceId::Mic => "自分",
            _ => "相手",
        })
    }

    /// Items in display order.
    pub fn ordered(&self) -> Vec<&TimelineItem> {
        order_timeline(&self.items).into_iter().map(|r| &self.items[r.0]).collect()
    }
}

/// Index into the slice given to [`order_timeline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRef(pub usize);

/// Orders segments, screenshots and markers for display and export (section 9), strictly by
/// time: a segment sorts at its start, a screenshot or marker at its own time, so the
/// timestamps shown never go backwards.
///
/// On a tie, a resume, reattach, cut or source marker comes first (what follows belongs after it), then
/// segments (相手 before 自分), then screenshots, then pause and unprocessed markers; the ID
/// breaks any remaining tie, which keeps capture order.
pub fn order_timeline(items: &[TimelineItem]) -> Vec<ItemRef> {
    // (time, kind rank, source rank, id, index)
    let mut keyed: Vec<(u64, u8, u8, &str, usize)> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| match item {
            TimelineItem::Segment(s) => (s.t_start_ms, 1, s.source.tie_rank(), s.id.as_str(), idx),
            TimelineItem::Screenshot(s) => (s.t_ms, 2, 0, s.id.as_str(), idx),
            TimelineItem::Marker(m) => {
                let rank = match m.kind {
                    MarkerKind::Resumed
                    | MarkerKind::SourceReattached
                    | MarkerKind::Cut
                    | MarkerKind::SourceChanged => 0,
                    MarkerKind::Paused | MarkerKind::Unprocessed => 3,
                };
                (m.t_ms, rank, 0, m.id.as_str(), idx)
            }
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(b.3)));
    keyed.into_iter().map(|k| ItemRef(k.4)).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn seg(id: &str, source: SourceId, start: u64, end: u64, text: &str) -> TimelineItem {
        TimelineItem::Segment(Segment {
            id: id.into(),
            source,
            t_start_ms: start,
            t_end_ms: end,
            text: text.into(),
            text_original: None,
            edited: false,
            utterance_id: None,
            important: false,
        })
    }

    pub fn shot(id: &str, t: u64) -> TimelineItem {
        TimelineItem::Screenshot(Screenshot {
            id: id.into(),
            t_ms: t,
            file: format!("images/{id}.png"),
            width: 100,
            height: 50,
            caption: None,
        })
    }

    fn ids(items: &[TimelineItem]) -> Vec<String> {
        order_timeline(items).into_iter().map(|r| items[r.0].id().to_string()).collect()
    }

    #[test]
    fn screenshot_outside_sentences_keeps_its_time() {
        let items = vec![
            seg("seg_000001", SourceId::App, 1000, 2000, "a"),
            shot("img_0001", 2500),
            seg("seg_000002", SourceId::App, 3000, 4000, "b"),
        ];
        assert_eq!(ids(&items), ["seg_000001", "img_0001", "seg_000002"]);
    }

    fn marker(id: &str, t: u64, kind: MarkerKind) -> TimelineItem {
        TimelineItem::Marker(Marker { id: id.into(), t_ms: t, kind, detail: None })
    }

    #[test]
    fn spec_example_screenshot_sorts_at_its_own_time() {
        // 相手 15:16:01–15:16:06, screenshot 15:16:03, 自分 starts 15:16:05.
        let items = vec![
            shot("img_0001", 3000),
            seg("seg_000002", SourceId::Mic, 5000, 7000, "自分"),
            seg("seg_000001", SourceId::App, 1000, 6000, "相手"),
        ];
        assert_eq!(ids(&items), ["seg_000001", "img_0001", "seg_000002"]);
    }

    #[test]
    fn screenshot_never_follows_a_later_sentence() {
        // Whisper often cuts one utterance into back-to-back segments; the old rule put the
        // screenshot after seg_000002 (4000) because it touched seg_000001's end.
        let items = vec![
            seg("seg_000001", SourceId::App, 1000, 4000, "a"),
            seg("seg_000002", SourceId::App, 4000, 9000, "b"),
            seg("seg_000003", SourceId::Mic, 2000, 9500, "c"),
            shot("img_0001", 3000),
        ];
        assert_eq!(ids(&items), ["seg_000001", "seg_000003", "img_0001", "seg_000002"]);
    }

    #[test]
    fn several_screenshots_keep_capture_order() {
        let items = vec![
            shot("img_0002", 4000),
            seg("seg_000001", SourceId::App, 1000, 6000, "a"),
            shot("img_0001", 2000),
            shot("img_0003", 5000),
            shot("img_0004", 5000),
        ];
        assert_eq!(ids(&items), ["seg_000001", "img_0001", "img_0002", "img_0003", "img_0004"]);
    }

    #[test]
    fn late_segment_reorders_existing_screenshot() {
        let mut items = vec![shot("img_0001", 3000)];
        assert_eq!(ids(&items), ["img_0001"]);
        items.push(seg("seg_000001", SourceId::App, 1000, 5000, "late"));
        assert_eq!(ids(&items), ["seg_000001", "img_0001"]);
    }

    #[test]
    fn ties_segments_first_then_others_before_me_then_id() {
        let items = vec![
            shot("img_0001", 1000),
            seg("seg_000003", SourceId::Mic, 1000, 1000, "me"),
            seg("seg_000002", SourceId::App, 1000, 1000, "them2"),
            seg("seg_000001", SourceId::App, 1000, 1000, "them1"),
        ];
        assert_eq!(ids(&items), ["seg_000001", "seg_000002", "seg_000003", "img_0001"]);
    }

    #[test]
    fn markers_on_a_tie_bracket_the_lines_they_describe() {
        let items = vec![
            seg("seg_000002", SourceId::App, 5000, 6000, "after resume"),
            marker("mk_0002", 5000, MarkerKind::Resumed),
            marker("mk_0001", 2000, MarkerKind::Paused),
            seg("seg_000001", SourceId::App, 2000, 2000, "before pause"),
            seg("seg_000003", SourceId::App, 8000, 9000, "first of a part"),
            marker("mk_0003", 8000, MarkerKind::Cut),
        ];
        assert_eq!(ids(&items), ["seg_000001", "mk_0001", "mk_0002", "seg_000002", "mk_0003", "seg_000003"]);
    }

    #[test]
    fn timeline_item_json_shape() {
        let json = serde_json::to_value(shot("img_0001", 5)).unwrap();
        assert_eq!(json["kind"], "screenshot");
        assert_eq!(json["tMs"], 5);
        let m = TimelineItem::Marker(Marker {
            id: "mk_0001".into(),
            t_ms: 9,
            kind: MarkerKind::SourceReattached,
            detail: None,
        });
        let json = serde_json::to_value(m).unwrap();
        assert_eq!(json["kind"], "marker");
        assert_eq!(json["type"], "source_reattached");
    }
}
