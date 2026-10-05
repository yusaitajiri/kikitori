//! Session folder naming (section 10).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

const MAX_TITLE_CHARS: usize = 40;
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Makes a title safe as part of a Windows file name.
pub fn sanitize_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let mut capped: String = replaced.trim_start().chars().take(MAX_TITLE_CHARS).collect();
    while capped.ends_with('.') || capped.ends_with(' ') {
        capped.pop();
    }
    let stem = capped.split('.').next().unwrap_or("").trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        capped.insert(stem.len(), '_');
    }
    capped
}

/// `YYYY-MM-DD_HHMM_<title>`.
pub fn folder_name(started: &DateTime<Local>, title: &str) -> String {
    let title = sanitize_title(title);
    let stamp = started.format("%Y-%m-%d_%H%M");
    if title.is_empty() { stamp.to_string() } else { format!("{stamp}_{title}") }
}

/// Picks a folder under `root` that does not exist yet, appending `-2`, `-3`, ... on collision.
pub fn unique_folder(root: &Path, name: &str) -> PathBuf {
    let first = root.join(name);
    if !first.exists() {
        return first;
    }
    (2u32..).map(|n| root.join(format!("{name}-{n}"))).find(|p| !p.exists()).expect("an unused suffix always exists")
}

/// Expands `output.titleTemplate`: `{app}` is the source name, `{date}` the start time.
pub fn expand_title(template: &str, app: &str, started: &DateTime<Local>) -> String {
    let date = started.format("%Y-%m-%d %H:%M").to_string();
    let out = template.replace("{app}", app).replace("{date}", &date);
    let out = out.trim();
    if out.is_empty() { app.to_string() } else { out.to_string() }
}

/// Converts a path inside the session folder to the forward-slash form used in Markdown.
pub fn to_forward_slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn replaces_forbidden_and_control_characters() {
        assert_eq!(sanitize_title("a<b>c:d\"e/f\\g|h?i*j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_title("tab\there\u{7}"), "tab_here_");
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_title("meeting. . "), "meeting");
        assert_eq!(sanitize_title("  lead"), "lead");
    }

    #[test]
    fn caps_at_forty_characters() {
        let long = "あ".repeat(60);
        assert_eq!(sanitize_title(&long).chars().count(), 40);
    }

    #[test]
    fn avoids_reserved_names() {
        assert_eq!(sanitize_title("CON"), "CON_");
        assert_eq!(sanitize_title("com1"), "com1_");
        assert_eq!(sanitize_title("nul.txt"), "nul_.txt");
        assert_eq!(sanitize_title("CONSOLE"), "CONSOLE");
    }

    #[test]
    fn folder_name_format() {
        let t = Local.with_ymd_and_hms(2026, 10, 2, 15, 13, 5).unwrap();
        assert_eq!(folder_name(&t, "Zoom"), "2026-10-02_1513_Zoom");
        assert_eq!(folder_name(&t, "..."), "2026-10-02_1513");
    }

    #[test]
    fn collisions_get_numbered_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let a = unique_folder(dir.path(), "x");
        assert_eq!(a, dir.path().join("x"));
        std::fs::create_dir(&a).unwrap();
        let b = unique_folder(dir.path(), "x");
        assert_eq!(b, dir.path().join("x-2"));
        std::fs::create_dir(&b).unwrap();
        assert_eq!(unique_folder(dir.path(), "x"), dir.path().join("x-3"));
    }

    #[test]
    fn title_template_expansion() {
        let t = Local.with_ymd_and_hms(2026, 10, 2, 15, 13, 5).unwrap();
        assert_eq!(expand_title("{app}", "Zoom", &t), "Zoom");
        assert_eq!(expand_title("{app} {date}", "Zoom", &t), "Zoom 2026-10-02 15:13");
        assert_eq!(expand_title("  ", "Zoom", &t), "Zoom");
        assert_eq!(expand_title("ゼミ", "Zoom", &t), "ゼミ");
    }
}
