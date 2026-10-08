//! HTML the PDF export falls back to when Typst fails (section 11): saved and opened in the
//! browser to print. Built from the session, not from Markdown.

use super::{Block, ExportOptions, blocks, clock, hms, marker_text, session_start, source_description};
use crate::export::markdown::image_alt;
use crate::session::model::{MarkerKind, Screenshot, Session};

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#"
@page {
  size: A4 portrait;
  margin: 18mm;
  @bottom-center { content: counter(page) " / " counter(pages); font-size: 8.5pt; color: #666; }
}
html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }
body {
  font-family: "Yu Gothic UI", "Meiryo", sans-serif;
  font-size: 10.5pt;
  line-height: 1.75;
  color: #111;
  margin: 0;
  overflow-wrap: anywhere;
}
header { border-bottom: 1.5pt solid #222; padding-bottom: 6pt; margin-bottom: 12pt; }
h1 { font-size: 17pt; margin: 0 0 4pt; line-height: 1.3; }
.meta { color: #555; font-size: 9pt; }
p { margin: 0 0 7pt; }
.time { color: #666; font-variant-numeric: tabular-nums; margin-right: 0.4em; }
.label { font-weight: 700; margin-right: 0.4em; }
.important { font-weight: 700; }
.important::before { content: "★"; margin-right: 0.4em; }
.label.me { color: #1b5e20; }
.label.others { color: #0d47a1; }
figure { margin: 8pt 0 10pt; break-inside: avoid; page-break-inside: avoid; }
figure img { display: block; max-width: 100%; height: auto; border: 0.5pt solid #ccc; }
figcaption { color: #555; font-size: 9pt; margin-top: 3pt; }
.marker { color: #666; font-style: italic; text-align: center; margin: 6pt 0; }
h2.cut { font-size: 9.5pt; color: #666; font-variant-numeric: tabular-nums; border-bottom: 0.5pt solid #ccc; padding-bottom: 2pt; margin: 16pt 0 8pt; break-after: avoid; page-break-after: avoid; }
"#;

pub fn render(session: &Session, opts: &ExportOptions) -> String {
    render_linking(session, opts, &|shot| shot.file.replace('\\', "/"))
}

/// `render` with each screenshot's `src` where `link` says.
pub fn render_linking(session: &Session, opts: &ExportOptions, link: &dyn Fn(&Screenshot) -> String) -> String {
    let start = session_start(session);
    let labels = opts.show_labels(session);
    let mut body = String::new();
    for block in blocks(session, opts) {
        match block {
            Block::Paragraph { t_ms, source, text, important } => {
                body.push_str(if important { "<p class=\"important\">" } else { "<p>" });
                if opts.timestamps {
                    body.push_str(&format!("<span class=\"time\">[{}]</span>", clock(session, t_ms)));
                }
                if labels {
                    let class = if source == crate::session::model::SourceId::Mic { "me" } else { "others" };
                    body.push_str(&format!(
                        "<span class=\"label {class}\">{}</span>",
                        escape(session.label_for(source))
                    ));
                }
                body.push_str(&escape(&text));
                body.push_str("</p>\n");
            }
            Block::Screenshot(shot) => {
                body.push_str(&format!(
                    "<figure><img src=\"{}\" alt=\"{}\" width=\"{}\" height=\"{}\">",
                    escape(&link(shot)),
                    escape(&image_alt(session, shot)),
                    shot.width,
                    shot.height,
                ));
                let caption = shot
                    .caption
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(escape)
                    .unwrap_or_else(|| format!("{} のスクリーンショット", clock(session, shot.t_ms)));
                body.push_str(&format!("<figcaption>{caption}</figcaption></figure>\n"));
            }
            Block::Marker(marker) if marker.kind == MarkerKind::Unprocessed => {
                body.push_str(&format!("<p class=\"marker\">（{}）</p>\n", escape(&marker_text(session, marker))));
            }
            Block::Marker(marker) if marker.kind == MarkerKind::Cut => {
                body.push_str(&format!("<h2 class=\"cut\">{}</h2>\n", clock(session, marker.t_ms)));
            }
            Block::Marker(marker) => {
                body.push_str(&format!("<p class=\"marker\">— {} —</p>\n", escape(&marker_text(session, marker))));
            }
        }
    }
    if session.unprocessed_ms > 0 && !super::plaintext::has_unprocessed_marker(session) {
        body.push_str(&format!(
            "<p class=\"marker\">{}</p>\n",
            escape(&super::plaintext::unprocessed_line(session.unprocessed_ms))
        ));
    }
    let meta = format!(
        "{} · {} · {}",
        start.format("%Y-%m-%d %H:%M"),
        hms(session.duration_ms),
        source_description(session).join(", ")
    );
    format!(
        "<!doctype html>\n<html lang=\"ja\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<header><h1>{title}</h1><div class=\"meta\">{meta}</div></header>\n<main>\n{body}</main>\n</body>\n</html>\n",
        title = escape(&session.title),
        meta = escape(&meta),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::test_support::*;

    #[test]
    fn print_html_snapshot() {
        insta::assert_snapshot!(render(&meeting(), &ExportOptions::default()));
    }

    #[test]
    fn escapes_text() {
        assert_eq!(escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }
}
