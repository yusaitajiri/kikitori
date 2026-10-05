//! 「Markdown + 画像（ZIP）」: the transcript and its screenshots in one file, to share or drop into
//! Notion or Obsidian. `transcript.md` sits at the top with `images/` beside it, so its image links
//! work once unpacked. The PDF is not included.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Component, Path};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::ExportOptions;
use super::markdown::{self, MarkdownMeta};
use crate::session::model::Session;

/// Writes the archive to `out` through a temporary file beside it, so a failure leaves nothing
/// half-written. Returns how many screenshots went in; one that is missing is left out.
pub fn write(
    session: &Session,
    opts: &ExportOptions,
    folder: &Path,
    out: &Path,
    app_version: &str,
) -> anyhow::Result<usize> {
    let part = out.with_extension("zip.part");
    let written = (|| -> anyhow::Result<usize> {
        let mut zip = ZipWriter::new(BufWriter::new(File::create(&part)?));
        zip.start_file(
            markdown::TRANSCRIPT_FILE,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        zip.write_all(markdown::document(session, opts, &MarkdownMeta { app_version }).as_bytes())?;
        // Screenshots are PNG, already compressed.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let mut shots = 0;
        for shot in session.screenshots() {
            let rel = Path::new(&shot.file);
            if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
                continue;
            }
            let bytes = match std::fs::read(folder.join(rel)) {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::warn!("ZIP export: {}: {e}", shot.file);
                    continue;
                }
            };
            zip.start_file(shot.file.replace('\\', "/"), stored)?;
            zip.write_all(&bytes)?;
            shots += 1;
        }
        zip.finish()?.flush()?;
        Ok(shots)
    })();
    match written {
        Ok(shots) => {
            std::fs::rename(&part, out)?;
            Ok(shots)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::export::test_support::*;
    use crate::session::model::TimelineItem;

    #[test]
    fn holds_the_transcript_and_its_screenshots() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("2026-10-02_1513_Zoom");
        std::fs::create_dir_all(folder.join("images")).unwrap();
        std::fs::write(folder.join("images/0001_151603.png"), b"png bytes").unwrap();
        std::fs::write(folder.join("transcript.pdf"), b"not wanted").unwrap();

        let out = dir.path().join("out.zip");
        let s = meeting();
        assert_eq!(write(&s, &ExportOptions::default(), &folder, &out, "0.1.0").unwrap(), 1);
        assert!(!dir.path().join("out.zip.part").exists());
        let mut zip = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let mut names: Vec<String> = zip.file_names().map(str::to_string).collect();
        names.sort();
        assert_eq!(names, ["images/0001_151603.png", "transcript.md"]);
        let mut md = String::new();
        zip.by_name("transcript.md").unwrap().read_to_string(&mut md).unwrap();
        assert!(md.contains("images/0001_151603.png") && md.contains("それでは始めます。"), "{md}");
    }

    #[test]
    fn leaves_out_missing_and_outside_screenshots() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = meeting();
        s.items.push(TimelineItem::Screenshot(crate::session::model::Screenshot {
            id: "img_0002".into(),
            t_ms: at(15, 30, 0),
            file: "../secret.png".into(),
            width: 1,
            height: 1,
            caption: None,
        }));
        std::fs::write(dir.path().join("secret.png"), b"x").unwrap();
        let folder = dir.path().join("session");
        std::fs::create_dir(&folder).unwrap();
        let out = dir.path().join("out.zip");
        assert_eq!(write(&s, &ExportOptions::default(), &folder, &out, "0.1.0").unwrap(), 0);
        let zip = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        assert_eq!(zip.file_names().collect::<Vec<_>>(), ["transcript.md"]);
    }
}
