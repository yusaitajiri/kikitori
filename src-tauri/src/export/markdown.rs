//! `transcript.md` (section 11).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Block, ExportOptions, blocks, clock, hms, marker_text, session_start, source_description};
use crate::session::model::{MarkerKind, Screenshot, Session};

pub const TRANSCRIPT_FILE: &str = "transcript.md";

pub struct MarkdownMeta<'a> {
    pub app_version: &'a str,
}

/// Full document with front matter.
pub fn document(session: &Session, opts: &ExportOptions, meta: &MarkdownMeta) -> String {
    document_linking(session, opts, meta, &session_link)
}

/// A screenshot's link inside the session folder: `images/0001_151603.png`.
fn session_link(shot: &Screenshot) -> String {
    shot.file.replace('\\', "/")
}

/// `document` with each screenshot linked where `link` says (an export saved elsewhere).
pub fn document_linking(
    session: &Session,
    opts: &ExportOptions,
    meta: &MarkdownMeta,
    link: &dyn Fn(&Screenshot) -> String,
) -> String {
    let mut out = front_matter(session, meta);
    out.push('\n');
    out.push_str(&body_linking(session, opts, link));
    out
}

pub fn front_matter(session: &Session, meta: &MarkdownMeta) -> String {
    let start = session_start(session);
    let sources: Vec<String> = source_description(session).iter().map(|s| yaml_flow_item(s)).collect();
    format!(
        "---\ntitle: {}\ndate: {}\nduration: {}\nsources: [{}]\nmodel: {}\napp: {}\n---\n",
        yaml_scalar(&session.title),
        start.format("%Y-%m-%d %H:%M"),
        hms(session.duration_ms),
        sources.join(", "),
        yaml_scalar(&session.model.id),
        yaml_scalar(&format!("Kikitori {}", meta.app_version)),
    )
}

/// Heading and paragraphs, no front matter (「Markdownでコピー」).
pub fn body(session: &Session, opts: &ExportOptions) -> String {
    body_linking(session, opts, &session_link)
}

fn body_linking(session: &Session, opts: &ExportOptions, link: &dyn Fn(&Screenshot) -> String) -> String {
    let start = session_start(session);
    let mut parts: Vec<String> =
        vec![format!("# {} — {}", escape_inline(&session.title), start.format("%Y-%m-%d %H:%M"))];
    let labels = opts.show_labels(session);
    for block in blocks(session, opts) {
        match block {
            Block::Paragraph { t_ms, source, text } => {
                let prefix = match (opts.timestamps, labels) {
                    (true, true) => {
                        format!("**[{}] {}:** ", clock(session, t_ms), session.label_for(source))
                    }
                    (true, false) => format!("**[{}]** ", clock(session, t_ms)),
                    (false, true) => format!("**{}:** ", session.label_for(source)),
                    (false, false) => String::new(),
                };
                let text =
                    if prefix.is_empty() { escape_block_start(&escape_inline(&text)) } else { escape_inline(&text) };
                parts.push(format!("{prefix}{text}"));
            }
            Block::Screenshot(shot) => parts.push(image_block(session, shot, &link(shot))),
            Block::Marker(marker) if marker.kind == MarkerKind::Unprocessed => {
                parts.push(format!("（{}）", marker_text(session, marker)));
            }
            // A cut opens a section, so a reader (or an agent) sees the session's parts.
            Block::Marker(marker) if marker.kind == MarkerKind::Cut => {
                parts.push(format!("## {}", clock(session, marker.t_ms)));
            }
            Block::Marker(marker) => parts.push(format!("*— {} —*", marker_text(session, marker))),
        }
    }
    if session.unprocessed_ms > 0 && !super::plaintext::has_unprocessed_marker(session) {
        parts.push(super::plaintext::unprocessed_line(session.unprocessed_ms));
    }
    let mut out = parts.join("\n\n");
    out.push('\n');
    out
}

pub fn image_alt(session: &Session, shot: &Screenshot) -> String {
    match &shot.caption {
        Some(c) if !c.trim().is_empty() => c.trim().to_string(),
        _ => format!("{} のスクリーンショット", clock(session, shot.t_ms)),
    }
}

fn image_block(session: &Session, shot: &Screenshot, link: &str) -> String {
    let alt = image_alt(session, shot).replace('[', "\\[").replace(']', "\\]");
    let mut s = format!("![{alt}]({link})");
    if let Some(c) = shot.caption.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        s.push_str(&format!("\n*{}*", escape_inline(c)));
    }
    s
}

fn escape_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Keeps a paragraph that starts with `#`, `-`, `+` or `1.` from turning into a heading or list.
fn escape_block_start(text: &str) -> String {
    let first = text.chars().next();
    let numbered = {
        let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
        !digits.is_empty() && text[digits.len()..].starts_with(['.', ')'])
    };
    if matches!(first, Some('#' | '-' | '+' | '=' | '|')) || numbered { format!("\\{text}") } else { text.to_string() }
}

fn yaml_needs_quotes(s: &str, flow: bool) -> bool {
    if s.is_empty() || s.trim() != s {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off") {
        return true;
    }
    if s.parse::<f64>().is_ok() {
        return true;
    }
    let first = s.chars().next().unwrap();
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) {
        return true;
    }
    if s.contains(": ") || s.contains(" #") || s.contains('"') || s.contains('\\') || s.contains('\n') {
        return true;
    }
    flow && s.contains([',', '[', ']', '{', '}'])
}

fn yaml_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn yaml_scalar(s: &str) -> String {
    if yaml_needs_quotes(s, false) { yaml_quote(s) } else { s.to_string() }
}

fn yaml_flow_item(s: &str) -> String {
    if yaml_needs_quotes(s, true) { yaml_quote(s) } else { s.to_string() }
}

/// Where to write so a transcript the user edited is never overwritten (section 11).
///
/// `written` maps file names Kikitori wrote to their SHA-256. The first candidate that does
/// not exist, or still has the hash Kikitori last wrote, wins.
pub fn choose_output_path(folder: &Path, base: &str, written: &BTreeMap<String, String>) -> PathBuf {
    let (stem, ext) = base.rsplit_once('.').unwrap_or((base, ""));
    for n in 1u32.. {
        let name = if n == 1 { base.to_string() } else { format!("{stem} ({n}).{ext}") };
        let path = folder.join(&name);
        match std::fs::read(&path) {
            Err(_) => return path,
            Ok(bytes) => {
                if written.get(&name).is_some_and(|h| *h == sha256_hex(&bytes)) {
                    return path;
                }
            }
        }
    }
    unreachable!()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::LabelMode;
    use crate::export::test_support::*;
    use crate::session::model::{Marker, SourceId, TimelineItem, tests::seg};

    const META: MarkdownMeta = MarkdownMeta { app_version: "0.1.0" };

    #[test]
    fn document_matches_spec_example() {
        insta::assert_snapshot!(document(&meeting(), &ExportOptions::default(), &META));
    }

    #[test]
    fn body_without_timestamps_with_caption() {
        let mut s = meeting();
        for item in s.items.iter_mut() {
            if let TimelineItem::Screenshot(shot) = item {
                shot.caption = Some("一枚目のスライド".into());
            }
        }
        let opts = ExportOptions { timestamps: false, labels: LabelMode::On, ..Default::default() };
        insta::assert_snapshot!(body(&s, &opts));
    }

    #[test]
    fn escapes_markdown_in_text() {
        let s = session(vec![seg("seg_000001", SourceId::App, 0, 100, "# 見出し*ではない*")], false);
        let opts = ExportOptions { timestamps: false, labels: LabelMode::Off, ..Default::default() };
        assert!(body(&s, &opts).contains("\n\n\\# 見出し\\*ではない\\*\n"));
    }

    #[test]
    fn a_cut_opens_a_section() {
        let mut s = meeting();
        s.items.push(TimelineItem::Marker(Marker {
            id: "mk_0002".into(),
            t_ms: at(15, 16, 5),
            kind: MarkerKind::Cut,
            detail: None,
        }));
        let md = body(&s, &ExportOptions::default());
        assert!(
            md.contains(
                "

## 15:16:05

**[15:16:05] 自分:** よろしくお願いします。
"
            ),
            "{md}"
        );
    }

    #[test]
    fn yaml_quoting() {
        assert_eq!(yaml_scalar("Zoom"), "Zoom");
        assert_eq!(yaml_scalar("ゼミ: 第3回"), "\"ゼミ: 第3回\"");
        assert_eq!(yaml_scalar("2026"), "\"2026\"");
        assert_eq!(yaml_flow_item("相手 (a, b)"), "\"相手 (a, b)\"");
        assert_eq!(yaml_flow_item("相手 (Zoom)"), "相手 (Zoom)");
    }

    #[test]
    fn an_export_saved_elsewhere_links_its_copied_images() {
        let link = |_: &Screenshot| crate::export::saving::markdown_target("My Talk_images/0001_151603.png");
        let md = document_linking(&meeting(), &ExportOptions::default(), &META, &link);
        assert!(md.contains("](My%20Talk_images/0001_151603.png)"), "{md}");
        assert!(!md.contains("](images/"));
    }

    #[test]
    fn never_overwrites_user_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut written = BTreeMap::new();
        let first = choose_output_path(dir.path(), TRANSCRIPT_FILE, &written);
        assert_eq!(first.file_name().unwrap(), "transcript.md");
        std::fs::write(&first, "ours").unwrap();
        written.insert("transcript.md".to_string(), sha256_hex(b"ours"));
        // Unchanged: overwrite in place.
        assert_eq!(choose_output_path(dir.path(), TRANSCRIPT_FILE, &written), first);
        // The user edits it: write next to it.
        std::fs::write(&first, "user edit").unwrap();
        let second = choose_output_path(dir.path(), TRANSCRIPT_FILE, &written);
        assert_eq!(second.file_name().unwrap(), "transcript (2).md");
    }
}
