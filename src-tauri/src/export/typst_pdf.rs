//! PDF and Typst export (section 11). The session becomes a Typst document: the preamble in
//! `transcript.typ` (the one place the look is decided) followed by calls such as
//! `#entry(time: "15:15:58", label: "相手", "…")`, with all session text in strings so none of it
//! is read as markup. The PDF compiles that document in process (Yu Gothic, `typst-pdf`); the
//! Typst export saves it next to `images/`, where `typst compile` makes the same PDF.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration, Smart};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use super::{Block, ExportOptions, blocks, clock, hms, image_number, marker_text, session_start, source_description};
use crate::session::model::{MarkerKind, Screenshot, Session, SourceId};

const PREAMBLE: &str = include_str!("transcript.typ");

/// Japanese fonts in `%WINDIR%\Fonts`, first set found wins: Yu Gothic (regular and bold) ships
/// with Windows 10 and 11; Meiryo and MS Gothic are older fallbacks.
const FONT_SETS: &[&[&str]] = &[&["YuGothR.ttc", "YuGothB.ttc"], &["meiryo.ttc", "meiryob.ttc"], &["msgothic.ttc"]];

/// For the PDF, screenshots are scaled down to this width and stored as JPEG, so it stays small.
const IMAGE_MAX_WIDTH: u32 = 1600;
const IMAGE_QUALITY: u8 = 85;
/// The text width of an A4 page with 18 mm margins, and how tall a screenshot may stand.
const TEXT_WIDTH_MM: f64 = 174.0;
const IMAGE_MAX_HEIGHT_MM: f64 = 150.0;

/// The session as the document lays it out.
#[derive(Debug, PartialEq)]
struct Doc {
    title: String,
    meta: String,
    blocks: Vec<DocBlock>,
}

#[derive(Debug, PartialEq)]
enum DocBlock {
    Line {
        time: Option<String>,
        label: Option<String>,
        me: bool,
        text: String,
    },
    /// `file` is relative to the document.
    Image {
        file: String,
        width_mm: f64,
        caption: String,
    },
    Note {
        text: String,
    },
    /// A new part of the session, headed by its time.
    Cut {
        time: String,
    },
}

/// A screenshot's path inside the session folder; nothing may point outside it.
fn shot_path(folder: &Path, shot: &Screenshot) -> Option<PathBuf> {
    let rel = Path::new(&shot.file);
    rel.components().all(|c| matches!(c, Component::Normal(_))).then(|| folder.join(rel))
}

/// The session as a document. `image` gives the path the document uses for a screenshot, or
/// `None` when it cannot be had (a note takes its place).
fn doc(session: &Session, opts: &ExportOptions, mut image: impl FnMut(&Screenshot) -> Option<String>) -> Doc {
    let labels = opts.show_labels(session);
    let mut out = Vec::new();
    for block in blocks(session, opts) {
        out.push(match block {
            Block::Paragraph { t_ms, source, text } => DocBlock::Line {
                time: opts.timestamps.then(|| clock(session, t_ms)),
                label: labels.then(|| session.label_for(source).to_string()),
                me: source == SourceId::Mic,
                text,
            },
            Block::Screenshot(shot) => {
                let time = clock(session, shot.t_ms);
                match image(shot) {
                    Some(file) => {
                        let aspect =
                            if shot.height > 0 { f64::from(shot.width) / f64::from(shot.height) } else { 16.0 / 9.0 };
                        let caption = shot
                            .caption
                            .as_deref()
                            .map(str::trim)
                            .filter(|c| !c.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("{time} のスクリーンショット"));
                        DocBlock::Image { file, width_mm: TEXT_WIDTH_MM.min(IMAGE_MAX_HEIGHT_MM * aspect), caption }
                    }
                    None => DocBlock::Note {
                        text: format!("（{time} のスクリーンショットが見つかりません）")
                    },
                }
            }
            Block::Marker(marker) if marker.kind == MarkerKind::Unprocessed => {
                DocBlock::Note { text: format!("（{}）", marker_text(session, marker)) }
            }
            Block::Marker(marker) if marker.kind == MarkerKind::Cut => {
                DocBlock::Cut { time: clock(session, marker.t_ms) }
            }
            Block::Marker(marker) => DocBlock::Note { text: format!("— {} —", marker_text(session, marker)) },
        });
    }
    if session.unprocessed_ms > 0 && !super::plaintext::has_unprocessed_marker(session) {
        out.push(DocBlock::Note { text: super::plaintext::unprocessed_line(session.unprocessed_ms) });
    }
    let meta = format!(
        "{} · {} · {}",
        session_start(session).format("%Y-%m-%d %H:%M"),
        hms(session.duration_ms),
        source_description(session).join(", ")
    );
    Doc { title: session.title.clone(), meta, blocks: out }
}

/// `text` as a Typst string literal: whatever it holds stays text.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The document's Typst source: the preamble, then the session.
fn source(doc: &Doc, first_line: &str) -> String {
    let mut out = format!("{first_line}\n\n{PREAMBLE}\n");
    out.push_str(&format!("#set document(title: {})\n", string(&doc.title)));
    out.push_str(&format!("#header({}, {})\n\n", string(&doc.title), string(&doc.meta)));
    for block in &doc.blocks {
        match block {
            DocBlock::Line { time, label, me, text } => {
                out.push_str("#entry(");
                if let Some(time) = time {
                    out.push_str(&format!("time: {}, ", string(time)));
                }
                if let Some(label) = label {
                    out.push_str(&format!("label: {}, ", string(label)));
                }
                if *me {
                    out.push_str("me: true, ");
                }
                out.push_str(&format!("{})\n", string(text)));
            }
            DocBlock::Image { file, width_mm, caption } => {
                out.push_str(&format!("#shot({}, {width_mm:.1}mm, {})\n", string(file), string(caption)));
            }
            DocBlock::Note { text } => out.push_str(&format!("#note({})\n", string(text))),
            DocBlock::Cut { time } => out.push_str(&format!("#cut({})\n", string(time))),
        }
    }
    out
}

/// The session as a Typst file to save: `links` gives each screenshot's path from the file (its
/// copy beside it, `saving::copy_images`), so `typst compile` there makes the same PDF as the app.
pub fn typst_source(
    session: &Session,
    opts: &ExportOptions,
    links: &HashMap<String, String>,
    app_version: &str,
) -> String {
    let doc = doc(session, opts, |shot| links.get(&shot.file).cloned());
    let first_line = format!(
        "// Kikitori {app_version} で書き出し。画像のフォルダと一緒に置いたまま `typst compile` すると PDF になります。"
    );
    source(&doc, &first_line)
}

/// The first set of Japanese fonts found in `%WINDIR%\Fonts`.
fn load_fonts() -> anyhow::Result<Vec<Font>> {
    let dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    for set in FONT_SETS {
        let mut fonts = Vec::new();
        for name in *set {
            if let Ok(data) = std::fs::read(dir.join(name)) {
                fonts.extend(Font::iter(Bytes::new(data)));
            }
        }
        if !fonts.is_empty() {
            return Ok(fonts);
        }
    }
    anyhow::bail!("no Japanese font found in {}", dir.display())
}

fn file_id(path: &str) -> FileId {
    RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).expect("valid virtual path")).intern()
}

/// Everything Typst may read: the document and the screenshots.
struct Transcript {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl World for Transcript {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() { Ok(self.main.clone()) } else { Err(not_found(id)) }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or_else(|| not_found(id))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

fn not_found(id: FileId) -> FileError {
    FileError::NotFound(id.vpath().get_without_slash().into())
}

/// The session as a PDF.
pub fn render(session: &Session, opts: &ExportOptions, folder: &Path, app_version: &str) -> anyhow::Result<Vec<u8>> {
    let fonts = load_fonts()?;
    let mut files = HashMap::new();
    let doc = doc(session, opts, |shot| {
        let path = shot_path(folder, shot)?;
        let jpeg = crate::screenshot::jpeg_for_print(&path, IMAGE_MAX_WIDTH, IMAGE_QUALITY)
            .inspect_err(|e| tracing::warn!("PDF image {}: {e:#}", shot.file))
            .ok()?;
        let file = format!("images/{}.jpg", image_number(shot));
        files.insert(file_id(&format!("/{file}")), Bytes::new(jpeg));
        Some(file)
    });
    let world = Transcript {
        library: LazyHash::new(Library::default()),
        book: LazyHash::new(FontBook::from_fonts(&fonts)),
        fonts,
        main: Source::new(file_id("/transcript.typ"), source(&doc, "// Kikitori")),
        files,
    };
    let errors = |e: typst::ecow::EcoVec<typst::diag::SourceDiagnostic>| {
        anyhow::anyhow!("{}", e.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("; "))
    };
    let result = typst::compile::<PagedDocument>(&world);
    let document = result.output.map_err(errors)?;
    let options = typst_pdf::PdfOptions {
        ident: Smart::Custom(session.id.clone()),
        creator: Smart::Custom(Some(format!("Kikitori {app_version}"))),
        ..Default::default()
    };
    let pdf = typst_pdf::pdf(&document, &options).map_err(errors);
    // Typst memoizes layout work across runs; an export is rare, so let it all go.
    typst::comemo::evict(0);
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::test_support::*;
    use crate::session::model::{Marker, TimelineItem};

    fn images(d: &Doc) -> Vec<&DocBlock> {
        d.blocks.iter().filter(|b| matches!(b, DocBlock::Image { .. })).collect()
    }

    #[test]
    fn doc_follows_the_export_options() {
        let s = meeting();
        let d = doc(&s, &ExportOptions::default(), |_| None);
        assert_eq!(d.title, "Zoom");
        assert_eq!(d.meta, "2026-10-02 15:13 · 01:02:15 · 相手 (Zoom), 自分 (マイク)");
        let DocBlock::Line { time, label, me, text } = &d.blocks[0] else { panic!("{:?}", d.blocks[0]) };
        assert_eq!((time.as_deref(), label.as_deref(), *me), (Some("15:15:58"), Some("相手"), false));
        assert!(text.starts_with("それでは始めます。"));
        assert!(d.blocks.iter().any(|b| matches!(b, DocBlock::Line { me: true, .. })));
        // No screenshot to be had: a note takes its place.
        assert!(d.blocks.contains(&DocBlock::Note {
            text: "（15:16:03 のスクリーンショットが見つかりません）".into()
        }));
        assert_eq!(d.blocks.last(), Some(&DocBlock::Note { text: "— 15:40:12 音声ソース再接続 —".into() }));

        let bare = ExportOptions { timestamps: false, labels: crate::export::LabelMode::Off, ..Default::default() };
        let d = doc(&s, &bare, |_| None);
        assert!(matches!(&d.blocks[0], DocBlock::Line { time: None, label: None, .. }));
    }

    #[test]
    fn session_text_is_only_ever_a_string() {
        assert_eq!(string("a\\b\"c\nd #x *y* $z$"), "\"a\\\\b\\\"c\\nd #x *y* $z$\"");
        assert_eq!(string("\u{7}"), "\"\\u{7}\"");
        let d = Doc {
            title: "T\"#".into(),
            meta: "m".into(),
            blocks: vec![
                DocBlock::Line {
                    time: Some("15:00:00".into()),
                    label: Some("自分".into()),
                    me: true,
                    text: "]) #evil".into(),
                },
                DocBlock::Image { file: "images/1.png".into(), width_mm: 112.5, caption: "c".into() },
                DocBlock::Note { text: "n".into() },
                DocBlock::Cut { time: "15:10:00".into() },
            ],
        };
        let src = source(&d, "// first");
        assert!(src.starts_with("// first\n\n// The look of a Kikitori transcript"));
        assert!(src.ends_with(
            "#set document(title: \"T\\\"#\")\n#header(\"T\\\"#\", \"m\")\n\n\
             #entry(time: \"15:00:00\", label: \"自分\", me: true, \"]) #evil\")\n\
             #shot(\"images/1.png\", 112.5mm, \"c\")\n#note(\"n\")\n#cut(\"15:10:00\")\n"
        ));
    }

    #[test]
    fn the_typst_file_links_the_copied_screenshots_and_nothing_outside_the_folder() {
        let mut s = meeting();
        for item in &mut s.items {
            if let TimelineItem::Screenshot(shot) = item {
                (shot.width, shot.height) = (2400, 3200);
            }
        }
        let links: HashMap<String, String> =
            [("images/0001_151603.png".to_string(), "talk_images/0001_151603.png".to_string())].into();
        // A tall shot is narrowed so it stands at most 150 mm high.
        let src = typst_source(&s, &ExportOptions::default(), &links, "0.1.0");
        assert!(
            src.contains("#shot(\"talk_images/0001_151603.png\", 112.5mm, \"15:16:03 のスクリーンショット\")"),
            "{src}"
        );
        assert!(src.starts_with("// Kikitori 0.1.0 で書き出し。"));

        let dir = tempfile::tempdir().unwrap();
        for item in &mut s.items {
            if let TimelineItem::Screenshot(shot) = item {
                shot.file = "../outside.png".into();
            }
        }
        std::fs::write(dir.path().join("outside.png"), b"x").unwrap();
        let d = doc(&s, &ExportOptions::default(), |shot| {
            shot_path(dir.path(), shot).filter(|p| p.is_file()).map(|_| shot.file.clone())
        });
        assert!(images(&d).is_empty());
    }

    #[test]
    fn renders_a_pdf_with_the_japanese_font() {
        if load_fonts().is_err() {
            eprintln!("skipping: no Japanese font on this machine");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("images")).unwrap();
        image::RgbImage::new(2400, 1350).save(dir.path().join("images/0001_151603.png")).unwrap();
        // With a cut, so its heading is laid out too.
        let mut s = meeting();
        s.items.push(TimelineItem::Marker(Marker {
            id: "mk_0002".into(),
            t_ms: at(15, 16, 5),
            kind: MarkerKind::Cut,
            detail: None,
        }));
        assert!(
            doc(&s, &ExportOptions::default(), |_| None).blocks.contains(&DocBlock::Cut { time: "15:16:05".into() })
        );
        let pdf = render(&s, &ExportOptions::default(), dir.path(), "0.1.0").unwrap();
        assert!(pdf.starts_with(b"%PDF-") && pdf.len() > 10_000, "{} bytes", pdf.len());
    }
}
