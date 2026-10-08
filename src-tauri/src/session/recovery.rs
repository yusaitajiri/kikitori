//! Session listing, snapshots and crash recovery (FR-61, FR-62).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::levels;
use super::log::{self, LogEvent, SessionLog};
use super::model::{Session, SourceId, TimelineItem};
use crate::export::markdown::{self, MarkdownMeta};
use crate::export::{ExportOptions, join_text, session_start};

pub const SNAPSHOT_FILE: &str = "session.json";

/// `session.json`: the session plus the hashes of files Kikitori wrote (so a transcript the
/// user edited is never overwritten).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(flatten)]
    pub session: Session,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub written: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub folder: String,
    pub segments: usize,
    pub screenshots: usize,
    /// No `session_stopped` in the log: the app crashed while recording.
    pub recoverable: bool,
    pub sources: Vec<SourceId>,
    /// The opening words, for the history list and its search.
    pub preview: String,
    /// The session's line in the history list.
    pub activity: Activity,
    /// Its project's ID (FR-64).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// The session's line in `ACTIVITY_SLICES` equal slices: how loud each side was (its `Sound`), or
/// for a session without `levels.bin`, when each side spoke: the share of each slice (0..100)
/// covered by 相手's lines (`others`) and 自分's (`me`), as `shapeOf` in `src/lib/line.ts`
/// computes it; and the slices holding a screenshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub others: Vec<u8>,
    pub me: Vec<u8>,
    pub shots: Vec<u16>,
}

/// How loud each side was in `ACTIVITY_SLICES` equal slices of the session (0..100), from its
/// `levels.bin`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub others: Vec<u8>,
    pub me: Vec<u8>,
}

pub const ACTIVITY_SLICES: usize = 128;

const PREVIEW_CHARS: usize = 120;

fn summarize(session: &Session, folder: &Path, recoverable: bool) -> SessionSummary {
    SessionSummary {
        id: session.id.clone(),
        title: session.title.clone(),
        started_at: session.started_at.clone(),
        duration_ms: session.duration_ms,
        folder: folder.to_string_lossy().into_owned(),
        segments: session.segments().count(),
        screenshots: session.screenshots().count(),
        recoverable,
        sources: session.sources.iter().map(|s| s.id).collect(),
        preview: preview(session),
        activity: activity(session, folder),
        project: session.project.clone(),
    }
}

/// Where a session's slices end: its duration, its last line or its last level reading.
fn session_end(session: &Session, readings: usize) -> u64 {
    let last_line = session.segments().map(|s| s.t_end_ms).max().unwrap_or(0);
    last_line.max(session.duration_ms).max(readings as u64 * levels::STEP_MS).max(1)
}

/// How the session sounded, for its line; `None` when the session has no `levels.bin`.
pub fn sound(session: &Session, folder: &Path) -> Option<Sound> {
    let readings = levels::read(folder).filter(|r| !r.is_empty())?;
    let mut sides = levels::slices(&readings, session_end(session, readings.len()), ACTIVITY_SLICES);
    levels::fit(&mut sides);
    let [others, me] = sides;
    Some(Sound { others, me })
}

fn activity(session: &Session, folder: &Path) -> Activity {
    let n = ACTIVITY_SLICES;
    let readings = levels::read(folder).filter(|r| !r.is_empty());
    let end = session_end(session, readings.as_ref().map_or(0, Vec::len));
    let span = end as f64 / n as f64;
    let slice = |t: f64| ((t / span) as usize).min(n - 1);
    let (others, me) = match readings {
        Some(readings) => {
            let mut sides = levels::slices(&readings, end, n);
            levels::fit(&mut sides);
            let [others, me] = sides;
            (others, me)
        }
        None => {
            let mut others = vec![0f64; n];
            let mut me = vec![0f64; n];
            for s in session.segments() {
                let side = if s.source == SourceId::Mic { &mut me } else { &mut others };
                let (a, b) = (s.t_start_ms as f64, s.t_end_ms.max(s.t_start_ms) as f64);
                for (k, share) in side.iter_mut().enumerate().take(slice(b) + 1).skip(slice(a)) {
                    let cover = ((k + 1) as f64 * span).min(b) - (k as f64 * span).max(a);
                    if cover > 0.0 {
                        *share += cover / span;
                    }
                }
            }
            let percent = |v: Vec<f64>| v.into_iter().map(|x| (x.min(1.0) * 100.0).round() as u8).collect();
            (percent(others), percent(me))
        }
    };
    let mut shots: Vec<u16> = session.screenshots().map(|s| slice(s.t_ms as f64) as u16).collect();
    shots.sort_unstable();
    shots.dedup();
    Activity { others, me, shots }
}

fn preview(session: &Session) -> String {
    let mut text = String::new();
    for item in session.ordered() {
        if let TimelineItem::Segment(s) = item {
            text = join_text(&text, &s.text);
            if text.chars().count() >= PREVIEW_CHARS {
                break;
            }
        }
    }
    text.chars().take(PREVIEW_CHARS).collect()
}

pub fn read_snapshot(folder: &Path) -> Option<Snapshot> {
    let text = std::fs::read_to_string(folder.join(SNAPSHOT_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes `session.json` atomically, keeping the `written` map from the previous snapshot.
pub fn write_snapshot(folder: &Path, session: &Session, written: &BTreeMap<String, String>) -> std::io::Result<()> {
    let snapshot = Snapshot { session: session.clone(), written: written.clone() };
    let json = serde_json::to_string_pretty(&snapshot).map_err(std::io::Error::other)?;
    write_atomic(&folder.join(SNAPSHOT_FILE), json.as_bytes())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Loads a session: the log is the source of truth; the snapshot only supplies `written`.
pub fn load(folder: &Path) -> std::io::Result<(Session, bool, BTreeMap<String, String>)> {
    let written = read_snapshot(folder).map(|s| s.written).unwrap_or_default();
    match log::replay(folder) {
        Ok(r) => Ok((r.session, r.stopped, written)),
        Err(err) => match read_snapshot(folder) {
            // A folder with only a snapshot (log deleted by hand) still opens.
            Some(snap) => Ok((snap.session, true, written)),
            None => Err(err),
        },
    }
}

/// Regenerates `transcript.md` (unless the user edited it) and `session.json`.
pub fn save_outputs(
    folder: &Path,
    session: &Session,
    opts: &ExportOptions,
    app_version: &str,
) -> std::io::Result<PathBuf> {
    let mut written = read_snapshot(folder).map(|s| s.written).unwrap_or_default();
    let project = super::projects::name_for(folder, session);
    let doc = markdown::document(session, opts, &MarkdownMeta { app_version, project: project.as_deref() });
    let path = markdown::choose_output_path(folder, markdown::TRANSCRIPT_FILE, &written);
    write_atomic(&path, doc.as_bytes())?;
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    written.insert(name, markdown::sha256_hex(doc.as_bytes()));
    write_snapshot(folder, session, &written)?;
    Ok(path)
}

/// Every session folder under the output root, newest first.
pub fn list(root: &Path) -> Vec<SessionSummary> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<SessionSummary> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join(log::LOG_FILE).exists() || p.join(SNAPSHOT_FILE).exists())
        .filter_map(|folder| {
            // A snapshot exists only once a session stopped (or was recovered).
            if let Some(snap) = read_snapshot(&folder) {
                return Some(summarize(&snap.session, &folder, false));
            }
            let r = log::replay(&folder).ok()?;
            Some(summarize(&r.session, &folder, !r.stopped))
        })
        .collect();
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out
}

pub fn find_folder(root: &Path, session_id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for folder in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if let Some(snap) = read_snapshot(&folder)
            && snap.session.id == session_id
        {
            return Some(folder);
        }
        if let Ok(first) = first_line(&folder.join(log::LOG_FILE))
            && first.contains(session_id)
            && log::replay(&folder).is_ok_and(|r| r.session.id == session_id)
        {
            return Some(folder);
        }
    }
    None
}

fn first_line(path: &Path) -> std::io::Result<String> {
    use std::io::BufRead;
    let f = std::fs::File::open(path)?;
    let mut line = String::new();
    std::io::BufReader::new(f).read_line(&mut line)?;
    Ok(line)
}

/// Sessions with no end marker, except the one recording now.
pub fn recoverable(root: &Path, active_session: Option<&str>) -> Vec<SessionSummary> {
    list(root).into_iter().filter(|s| s.recoverable && Some(s.id.as_str()) != active_session).collect()
}

/// Flow D: replay the log, write the outputs, append `session_stopped` with `recovered`.
pub fn recover(folder: &Path, opts: &ExportOptions, app_version: &str) -> std::io::Result<PathBuf> {
    let replay = log::replay(folder)?;
    let mut session = replay.session;
    if !replay.stopped {
        let ended = session_start(&session) + chrono::Duration::milliseconds(session.duration_ms as i64);
        let event = LogEvent::SessionStopped {
            ended_at: ended.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            duration_ms: session.duration_ms,
            unprocessed_ms: 0,
            recovered: Some(true),
        };
        let writer = SessionLog::open_append(folder)?;
        writer.append(&event)?;
        writer.sync()?;
        log::apply(&mut session, event);
    }
    save_outputs(folder, &session, opts, app_version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::log::encode_line;
    use crate::session::model::{ModelRef, SourceId, SourceInfo};

    fn write_log(folder: &Path, stopped: bool) {
        std::fs::create_dir_all(folder).unwrap();
        let mut lines = vec![
            encode_line(&LogEvent::SessionStarted {
                id: "01JTEST".into(),
                title: "Zoom".into(),
                started_at: "2026-10-02T15:13:05+09:00".into(),
                sources: vec![SourceInfo {
                    id: SourceId::App,
                    label: "相手".into(),
                    exe: None,
                    device: None,
                    name: Some("Zoom".into()),
                }],
                model: ModelRef { id: "turbo-q5".into(), sha256: "x".into() },
                language: "ja".into(),
                gpu: false,
                app_version: "0.1.0".into(),
            }),
            encode_line(&LogEvent::Segment {
                id: "seg_000001".into(),
                utterance_id: "utt_000001".into(),
                source: SourceId::App,
                t_start_ms: 1000,
                t_end_ms: 4000,
                text: "それでは始めます。".into(),
                no_speech: 0.01,
            }),
        ];
        if stopped {
            lines.push(encode_line(&LogEvent::SessionStopped {
                ended_at: "2026-10-02T15:14:05+09:00".into(),
                duration_ms: 60_000,
                unprocessed_ms: 0,
                recovered: None,
            }));
        } else {
            lines.push("{\"v\":1,\"type\":\"segment\",\"id\":\"seg_0000".into()); // crash mid-write
        }
        std::fs::write(folder.join(log::LOG_FILE), lines.concat()).unwrap();
    }

    #[test]
    fn crashed_session_is_listed_and_recovered() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("2026-10-02_1513_Zoom");
        write_log(&folder, false);
        let rec = recoverable(root.path(), None);
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].id, "01JTEST");
        assert_eq!(rec[0].sources, [SourceId::App]);
        assert_eq!(rec[0].preview, "それでは始めます。");
        // One segment from 1 s to 4 s of a session that has run 4 s so far: 相手 only.
        let a = &rec[0].activity;
        assert_eq!((a.others.len(), a.me.len()), (ACTIVITY_SLICES, ACTIVITY_SLICES));
        let n = ACTIVITY_SLICES;
        assert_eq!((a.others[0], a.others[n / 4 - 1], a.others[n / 4], a.others[n - 1]), (0, 0, 100, 100));
        assert!(a.me.iter().all(|&v| v == 0) && a.shots.is_empty());
        assert!(recoverable(root.path(), Some("01JTEST")).is_empty());

        let path = recover(&folder, &ExportOptions::default(), "0.1.0").unwrap();
        assert_eq!(path.file_name().unwrap(), "transcript.md");
        let md = std::fs::read_to_string(&path).unwrap();
        assert!(md.contains("**[15:13:06]** それでは始めます。"), "{md}");
        let r = log::replay(&folder).unwrap();
        assert!(r.stopped);
        assert!(std::fs::read_to_string(folder.join(log::LOG_FILE)).unwrap().contains("\"recovered\":true"));
        assert!(recoverable(root.path(), None).is_empty());
        assert_eq!(find_folder(root.path(), "01JTEST").unwrap(), folder);
    }

    #[test]
    fn a_session_with_levels_is_drawn_from_its_sound() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("2026-10-02_1513_Zoom");
        write_log(&folder, true);
        let session = log::replay(&folder).unwrap().session;
        assert!(sound(&session, &folder).is_none());
        // A minute: 相手 speaks for the first half, then silence.
        let mut w = levels::LevelWriter::create(&folder).unwrap();
        for i in 0..600 {
            w.push(i * levels::STEP_MS, if i < 300 { -6.0 } else { -60.0 }, -60.0).unwrap();
        }
        drop(w);
        let s = sound(&session, &folder).unwrap();
        let n = ACTIVITY_SLICES;
        assert!(s.others[..n / 2].iter().all(|&v| v == 100) && s.others[n / 2..].iter().all(|&v| v == 0));
        assert!(s.me.iter().all(|&v| v == 0));
        // History draws the same, not when each side spoke (one line, 1 s to 4 s).
        let listed = list(root.path());
        assert_eq!((&listed[0].activity.others, &listed[0].activity.me), (&s.others, &s.me));
    }

    #[test]
    fn stopped_session_is_not_recoverable() {
        let root = tempfile::tempdir().unwrap();
        write_log(&root.path().join("a"), true);
        assert!(recoverable(root.path(), None).is_empty());
        assert_eq!(list(root.path()).len(), 1);
    }

    #[test]
    fn snapshot_keeps_written_hashes() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("s");
        write_log(&folder, true);
        let (session, stopped, _) = load(&folder).unwrap();
        assert!(stopped);
        save_outputs(&folder, &session, &ExportOptions::default(), "0.1.0").unwrap();
        let snap = read_snapshot(&folder).unwrap();
        assert!(snap.written.contains_key("transcript.md"));
        // User edits the transcript; the next save goes to "transcript (2).md".
        std::fs::write(folder.join("transcript.md"), "edited").unwrap();
        let p = save_outputs(&folder, &session, &ExportOptions::default(), "0.1.0").unwrap();
        assert_eq!(p.file_name().unwrap(), "transcript (2).md");
        assert_eq!(std::fs::read_to_string(folder.join("transcript.md")).unwrap(), "edited");
    }
}
