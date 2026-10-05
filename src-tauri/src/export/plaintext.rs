//! Clipboard formats: plain text and 「Agent用にコピー」 (section 11).

use super::{Block, ExportOptions, blocks, clock, image_number, marker_text, prompts};
use crate::session::model::{MarkerKind, Session, TimelineItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotStyle {
    /// `[画像 15:16:03]`
    Plain,
    /// `[画像:0001 15:16:03]`, the placeholder the cleanup prompt protects.
    Numbered,
}

/// One paragraph per line.
pub fn render(session: &Session, opts: &ExportOptions, shots: ShotStyle) -> String {
    let labels = opts.show_labels(session);
    let mut lines: Vec<String> = Vec::new();
    for block in blocks(session, opts) {
        match block {
            Block::Paragraph { t_ms, source, text } => {
                let mut line = String::new();
                if opts.timestamps {
                    line.push_str(&format!("[{}] ", clock(session, t_ms)));
                }
                if labels {
                    line.push_str(session.label_for(source));
                    line.push_str(": ");
                }
                line.push_str(&text);
                lines.push(line);
            }
            Block::Screenshot(shot) => match shots {
                ShotStyle::Plain if opts.screenshot_markers => {
                    lines.push(format!("[画像 {}]", clock(session, shot.t_ms)));
                }
                ShotStyle::Plain => {}
                ShotStyle::Numbered => {
                    lines.push(format!("[画像:{} {}]", image_number(shot), clock(session, shot.t_ms)));
                }
            },
            Block::Marker(marker) if marker.kind == MarkerKind::Unprocessed => {
                lines.push(format!("（{}）", marker_text(session, marker)));
            }
            Block::Marker(marker) => lines.push(format!("— {} —", marker_text(session, marker))),
        }
    }
    if session.unprocessed_ms > 0 && !has_unprocessed_marker(session) {
        lines.push(unprocessed_line(session.unprocessed_ms));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn plain(session: &Session, opts: &ExportOptions) -> String {
    render(session, opts, ShotStyle::Plain)
}

/// Cleanup prompt followed by the transcript with numbered image placeholders.
pub fn for_agent(session: &Session, opts: &ExportOptions, vocabulary: &[String]) -> String {
    let transcript = render(session, opts, ShotStyle::Numbered);
    prompts::cleanup_prompt(vocabulary, transcript.trim_end())
}

pub(crate) fn has_unprocessed_marker(session: &Session) -> bool {
    session.items.iter().any(|i| matches!(i, TimelineItem::Marker(m) if m.kind == MarkerKind::Unprocessed))
}

pub(crate) fn unprocessed_line(ms: u64) -> String {
    format!("（以降、未処理の音声 {} 秒）", ms.div_ceil(1000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::LabelMode;
    use crate::export::test_support::*;

    #[test]
    fn plain_default() {
        insta::assert_snapshot!(plain(&meeting(), &ExportOptions::default()));
    }

    #[test]
    fn plain_without_times_labels_or_images() {
        let opts = ExportOptions {
            timestamps: false,
            labels: LabelMode::Off,
            screenshot_markers: false,
            ..Default::default()
        };
        insta::assert_snapshot!(plain(&meeting(), &opts));
    }

    #[test]
    fn labels_auto_hidden_with_one_source() {
        use crate::session::model::{SourceId, tests::seg};
        let s = session(vec![seg("seg_000001", SourceId::App, 0, 500, "ひとつ")], false);
        assert_eq!(plain(&s, &ExportOptions::default()), "[15:13:05] ひとつ\n");
        let on = ExportOptions { labels: LabelMode::On, ..Default::default() };
        assert_eq!(plain(&s, &on), "[15:13:05] 相手: ひとつ\n");
    }

    #[test]
    fn agent_format() {
        insta::assert_snapshot!(for_agent(&meeting(), &ExportOptions::default(), &["大澤研".to_string()]));
    }

    #[test]
    fn unprocessed_tail() {
        let mut s = meeting();
        s.unprocessed_ms = 12_300;
        assert!(plain(&s, &ExportOptions::default()).ends_with("（以降、未処理の音声 13 秒）\n"));
    }
}
