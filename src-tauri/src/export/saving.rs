//! Exports are saved where the user chooses, never in the session folder. Markdown goes into a new
//! folder of its own (`transcript.md` and `images/`, as in the session folder and the ZIP). A Typst
//! file links its screenshots from `<name>_images/` beside it: a folder named after the file, so an
//! `images/` already there is left alone.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::ExportOptions;
use super::markdown::{self, MarkdownMeta};
use crate::session::model::Session;
use crate::session::{paths, recovery};

/// Where `out`'s screenshots go: `report.md` → `report_images/`.
pub fn images_dir(out: &Path) -> (PathBuf, String) {
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "transcript".into());
    let name = format!("{stem}_images");
    (out.with_file_name(&name), name)
}

/// Copies the session's screenshots next to `out` and returns, for each screenshot file copied,
/// its path relative to `out` (forward slashes). A screenshot missing from the session folder, or
/// pointing outside it, is left out.
pub fn copy_images(session: &Session, folder: &Path, out: &Path) -> std::io::Result<HashMap<String, String>> {
    let (dir, name) = images_dir(out);
    copy_images_into(session, folder, &dir, &name)
}

/// Copies the session's screenshots into `dir` and returns each copied file's link, `prefix/name`.
fn copy_images_into(
    session: &Session,
    folder: &Path,
    dir: &Path,
    prefix: &str,
) -> std::io::Result<HashMap<String, String>> {
    let mut links = HashMap::new();
    for shot in session.screenshots() {
        let rel = Path::new(&shot.file);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            continue;
        }
        let (src, Some(file_name)) = (folder.join(rel), rel.file_name()) else { continue };
        if !src.is_file() {
            continue;
        }
        std::fs::create_dir_all(dir)?;
        std::fs::copy(&src, dir.join(file_name))?;
        links.insert(shot.file.clone(), format!("{prefix}/{}", file_name.to_string_lossy()));
    }
    Ok(links)
}

/// 書き出し → Markdown without ZIP: a new folder in `parent`, named after the session folder (with
/// `-2` and so on if taken), holding `transcript.md` and `images/`. Returns the new folder.
pub fn markdown_folder(
    session: &Session,
    opts: &ExportOptions,
    folder: &Path,
    parent: &Path,
    app_version: &str,
) -> std::io::Result<PathBuf> {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "transcript".into());
    let out = paths::unique_folder(parent, &name);
    std::fs::create_dir_all(&out)?;
    let links = copy_images_into(session, folder, &out.join("images"), "images")?;
    let doc = markdown::document_linking(session, opts, &MarkdownMeta { app_version }, &|shot| {
        markdown_target(links.get(&shot.file).map_or(shot.file.as_str(), String::as_str))
    });
    recovery::write_atomic(&out.join(markdown::TRANSCRIPT_FILE), doc.as_bytes())?;
    Ok(out)
}

/// A relative path as a Markdown link target: spaces and brackets would end the link.
pub fn markdown_target(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '(' => "%28".to_string(),
            ')' => "%29".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// A file as a `file:///` URL, for the HTML fallback opened from wherever it was saved.
pub fn file_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    format!("file:///{}", markdown_target(p.trim_start_matches('/')).replace('#', "%23"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::test_support::*;

    #[test]
    fn screenshots_go_beside_the_file_in_a_folder_named_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("session");
        std::fs::create_dir_all(folder.join("images")).unwrap();
        std::fs::write(folder.join("images/0001_151603.png"), b"png").unwrap();
        let out = dir.path().join("out").join("My Talk.md");
        std::fs::create_dir(dir.path().join("out")).unwrap();
        let links = copy_images(&meeting(), &folder, &out).unwrap();
        assert_eq!(links["images/0001_151603.png"], "My Talk_images/0001_151603.png");
        assert_eq!(std::fs::read(dir.path().join("out/My Talk_images/0001_151603.png")).unwrap(), b"png");
        assert_eq!(markdown_target("My Talk_images/a (1).png"), "My%20Talk_images/a%20%281%29.png");
    }

    #[test]
    fn markdown_gets_a_folder_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("2026-10-02_1513_Zoom");
        std::fs::create_dir_all(folder.join("images")).unwrap();
        std::fs::write(folder.join("images/0001_151603.png"), b"png").unwrap();
        let parent = dir.path().join("exports");
        std::fs::create_dir_all(parent.join("2026-10-02_1513_Zoom")).unwrap();
        let out = markdown_folder(&meeting(), &ExportOptions::default(), &folder, &parent, "0.1.0").unwrap();
        // The name was taken, so it gets a suffix; nothing in the existing folder is touched.
        assert_eq!(out, parent.join("2026-10-02_1513_Zoom-2"));
        assert_eq!(std::fs::read(out.join("images/0001_151603.png")).unwrap(), b"png");
        let md = std::fs::read_to_string(out.join("transcript.md")).unwrap();
        assert!(md.contains("](images/0001_151603.png)"), "{md}");
        assert_eq!(std::fs::read_dir(parent.join("2026-10-02_1513_Zoom")).unwrap().count(), 0);
    }

    #[test]
    fn no_folder_without_screenshots() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("t.md");
        assert!(copy_images(&meeting(), &dir.path().join("missing"), &out).unwrap().is_empty());
        assert!(!dir.path().join("t_images").exists());
        assert_eq!(file_url(Path::new("C:\\a b\\#1.png")), "file:///C:/a%20b/%231.png");
    }
}
