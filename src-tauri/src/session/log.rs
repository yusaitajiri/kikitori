//! Append-only event log (`session.jsonl`), the source of truth while recording.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::model::{
    Marker, MarkerKind, ModelRef, SCHEMA_VERSION, Screenshot, Segment, Session, SourceId, SourceInfo, TimelineItem,
};

pub const LOG_FILE: &str = "session.jsonl";
const SYNC_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum LogEvent {
    SessionStarted {
        id: String,
        title: String,
        started_at: String,
        sources: Vec<SourceInfo>,
        model: ModelRef,
        language: String,
        gpu: bool,
        app_version: String,
    },
    Segment {
        id: String,
        utterance_id: String,
        source: SourceId,
        t_start_ms: u64,
        t_end_ms: u64,
        text: String,
        no_speech: f32,
    },
    SegmentRemoved {
        id: String,
        reason: String,
    },
    SegmentEdited {
        id: String,
        text: String,
    },
    Screenshot {
        id: String,
        t_ms: u64,
        file: String,
        width: u32,
        height: u32,
        target: String,
    },
    ScreenshotDeleted {
        id: String,
    },
    CaptionSet {
        id: String,
        caption: String,
    },
    Marker {
        id: String,
        t_ms: u64,
        kind: MarkerKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    TitleChanged {
        title: String,
    },
    /// The detected language once `auto` locks it (section 8).
    LanguageDetected {
        language: String,
    },
    SessionStopped {
        ended_at: String,
        duration_ms: u64,
        unprocessed_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovered: Option<bool>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct LogLine {
    v: u32,
    #[serde(flatten)]
    event: LogEvent,
}

pub fn encode_line(event: &LogEvent) -> String {
    let line = LogLine { v: SCHEMA_VERSION, event: event.clone() };
    let mut s = serde_json::to_string(&line).expect("log events always serialize");
    s.push('\n');
    s
}

/// Appends events and flushes each one; `sync_data` runs at most every 5 s and on close.
pub struct SessionLog {
    path: PathBuf,
    inner: Mutex<LogInner>,
}

struct LogInner {
    file: File,
    last_sync: Instant,
    dirty: bool,
}

impl SessionLog {
    pub fn create(folder: &Path) -> io::Result<Self> {
        let path = folder.join(LOG_FILE);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { path, inner: Mutex::new(LogInner { file, last_sync: Instant::now(), dirty: false }) })
    }

    /// Opens an existing log to append after-the-fact events (edits, recovery).
    pub fn open_append(folder: &Path) -> io::Result<Self> {
        let path = folder.join(LOG_FILE);
        let mut file = OpenOptions::new().append(true).open(&path)?;
        // A crash can leave a truncated last line; start ours on a fresh line.
        if let Ok(meta) = file.metadata()
            && meta.len() > 0
            && !ends_with_newline(&path)?
        {
            file.write_all(b"\n")?;
        }
        Ok(Self { path, inner: Mutex::new(LogInner { file, last_sync: Instant::now(), dirty: false }) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, event: &LogEvent) -> io::Result<()> {
        let line = encode_line(event);
        let mut inner = self.inner.lock();
        inner.file.write_all(line.as_bytes())?;
        inner.file.flush()?;
        inner.dirty = true;
        if inner.last_sync.elapsed() >= SYNC_EVERY {
            inner.file.sync_data()?;
            inner.last_sync = Instant::now();
            inner.dirty = false;
        }
        Ok(())
    }

    /// Called from the 1 Hz ticker so a quiet session still reaches the disk every 5 s.
    pub fn sync_if_due(&self) -> io::Result<()> {
        let mut inner = self.inner.lock();
        if inner.dirty && inner.last_sync.elapsed() >= SYNC_EVERY {
            inner.file.sync_data()?;
            inner.last_sync = Instant::now();
            inner.dirty = false;
        }
        Ok(())
    }

    pub fn sync(&self) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.file.sync_data()?;
        inner.last_sync = Instant::now();
        inner.dirty = false;
        Ok(())
    }
}

fn ends_with_newline(path: &Path) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    f.seek(SeekFrom::Start(len - 1))?;
    let mut b = [0u8; 1];
    f.read_exact(&mut b)?;
    Ok(b[0] == b'\n')
}

#[derive(Debug)]
pub struct Replay {
    pub session: Session,
    /// True when a `session_stopped` event exists.
    pub stopped: bool,
    /// Lines that could not be parsed (excluding a truncated last line).
    pub bad_lines: usize,
}

/// Rebuilds a session from its event log. A truncated last line is ignored.
pub fn replay(folder: &Path) -> io::Result<Replay> {
    let path = folder.join(LOG_FILE);
    let reader = BufReader::new(File::open(&path)?);
    let lines: Vec<String> = reader
        .split(b'\n')
        .map(|l| l.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
        .collect::<io::Result<_>>()?;
    replay_lines(&lines).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "log has no session_started"))
}

pub fn replay_lines(lines: &[String]) -> Option<Replay> {
    let mut session: Option<Session> = None;
    let mut stopped = false;
    let mut bad_lines = 0;
    let last_nonempty = lines.iter().rposition(|l| !l.trim().is_empty());

    for (idx, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: LogLine = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(err) => {
                if Some(idx) != last_nonempty {
                    tracing::warn!("skipping unreadable log line {}: {}", idx + 1, err);
                    bad_lines += 1;
                }
                continue;
            }
        };
        match parsed.event {
            LogEvent::SessionStarted { id, title, started_at, sources, model, language, gpu, .. } => {
                session = Some(Session {
                    v: SCHEMA_VERSION,
                    id,
                    title,
                    started_at,
                    ended_at: None,
                    duration_ms: 0,
                    sources,
                    model,
                    language,
                    gpu,
                    items: Vec::new(),
                    unprocessed_ms: 0,
                });
            }
            event => {
                let Some(s) = session.as_mut() else { continue };
                if let LogEvent::SessionStopped { .. } = event {
                    stopped = true;
                }
                apply(s, event);
            }
        }
    }
    session.map(|session| Replay { session, stopped, bad_lines })
}

/// Applies one event to an in-memory session. Shared by replay and the live recorder.
pub fn apply(s: &mut Session, event: LogEvent) {
    match event {
        LogEvent::SessionStarted { .. } => {}
        LogEvent::Segment { id, utterance_id, source, t_start_ms, t_end_ms, text, .. } => {
            s.items.push(TimelineItem::Segment(Segment {
                id,
                source,
                t_start_ms,
                t_end_ms,
                text,
                text_original: None,
                edited: false,
                utterance_id: Some(utterance_id),
            }));
            if t_end_ms > s.duration_ms {
                s.duration_ms = t_end_ms;
            }
        }
        LogEvent::SegmentRemoved { id, .. } | LogEvent::ScreenshotDeleted { id } => {
            s.items.retain(|i| i.id() != id);
        }
        LogEvent::SegmentEdited { id, text } => {
            for item in s.items.iter_mut() {
                if let TimelineItem::Segment(seg) = item
                    && seg.id == id
                {
                    if seg.text_original.is_none() {
                        seg.text_original = Some(seg.text.clone());
                    }
                    if seg.text_original.as_deref() == Some(text.as_str()) {
                        seg.text_original = None;
                        seg.edited = false;
                    } else {
                        seg.edited = true;
                    }
                    seg.text = text;
                    break;
                }
            }
        }
        LogEvent::Screenshot { id, t_ms, file, width, height, .. } => {
            s.items.push(TimelineItem::Screenshot(Screenshot { id, t_ms, file, width, height, caption: None }));
            if t_ms > s.duration_ms {
                s.duration_ms = t_ms;
            }
        }
        LogEvent::CaptionSet { id, caption } => {
            for item in s.items.iter_mut() {
                if let TimelineItem::Screenshot(shot) = item
                    && shot.id == id
                {
                    let caption = caption.trim().to_string();
                    shot.caption = if caption.is_empty() { None } else { Some(caption) };
                    break;
                }
            }
        }
        LogEvent::Marker { id, t_ms, kind, detail } => {
            s.items.push(TimelineItem::Marker(Marker { id, t_ms, kind, detail }));
            if t_ms > s.duration_ms {
                s.duration_ms = t_ms;
            }
        }
        LogEvent::TitleChanged { title } => s.title = title,
        // The session keeps the configured language (`auto`); the detected one is only logged.
        LogEvent::LanguageDetected { .. } => {}
        LogEvent::SessionStopped { ended_at, duration_ms, unprocessed_ms, .. } => {
            s.ended_at = Some(ended_at);
            s.duration_ms = s.duration_ms.max(duration_ms);
            s.unprocessed_ms = unprocessed_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> LogEvent {
        LogEvent::SessionStarted {
            id: "01JABCDXYZ".into(),
            title: "Zoom".into(),
            started_at: "2026-10-02T15:13:05+09:00".into(),
            sources: vec![SourceInfo {
                id: SourceId::App,
                label: "相手".into(),
                exe: Some("Zoom.exe".into()),
                device: None,
                name: None,
            }],
            model: ModelRef { id: "turbo-q5".into(), sha256: "abc".into() },
            language: "ja".into(),
            gpu: true,
            app_version: "0.1.0".into(),
        }
    }

    fn segment(id: &str, start: u64, end: u64, text: &str) -> LogEvent {
        LogEvent::Segment {
            id: id.into(),
            utterance_id: "utt_000001".into(),
            source: SourceId::App,
            t_start_ms: start,
            t_end_ms: end,
            text: text.into(),
            no_speech: 0.02,
        }
    }

    #[test]
    fn line_format_matches_spec() {
        let line = encode_line(&segment("seg_000123", 172340, 176020, "それでは始めます。"));
        assert_eq!(
            line,
            "{\"v\":1,\"type\":\"segment\",\"id\":\"seg_000123\",\"utteranceId\":\"utt_000001\",\"source\":\"app\",\"tStartMs\":172340,\"tEndMs\":176020,\"text\":\"それでは始めます。\",\"noSpeech\":0.02}\n"
        );
        let marker = encode_line(&LogEvent::Marker {
            id: "mk_0001".into(),
            t_ms: 905000,
            kind: MarkerKind::SourceReattached,
            detail: None,
        });
        assert_eq!(
            marker,
            "{\"v\":1,\"type\":\"marker\",\"id\":\"mk_0001\",\"tMs\":905000,\"kind\":\"source_reattached\"}\n"
        );
    }

    #[test]
    fn replay_builds_session_and_ignores_truncated_last_line() {
        let mut lines: Vec<String> = [
            started(),
            segment("seg_000001", 1000, 2000, "一"),
            segment("seg_000002", 3000, 4000, "二"),
            LogEvent::SegmentRemoved { id: "seg_000001".into(), reason: "echo".into() },
            LogEvent::SegmentEdited { id: "seg_000002".into(), text: "二です".into() },
        ]
        .iter()
        .map(|e| encode_line(e).trim_end().to_string())
        .collect();
        lines.push("{\"v\":1,\"type\":\"segm".into());
        let replay = replay_lines(&lines).unwrap();
        assert!(!replay.stopped);
        assert_eq!(replay.bad_lines, 0);
        let segs: Vec<_> = replay.session.segments().collect();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "二です");
        assert_eq!(segs[0].text_original.as_deref(), Some("二"));
        assert!(segs[0].edited);
        assert_eq!(replay.session.duration_ms, 4000);
    }

    #[test]
    fn replay_counts_bad_middle_lines_and_detects_stop() {
        let lines = vec![
            encode_line(&started()),
            "garbage".to_string(),
            encode_line(&LogEvent::SessionStopped {
                ended_at: "2026-10-02T16:15:20+09:00".into(),
                duration_ms: 3_735_000,
                unprocessed_ms: 0,
                recovered: None,
            }),
        ];
        let replay = replay_lines(&lines).unwrap();
        assert!(replay.stopped);
        assert_eq!(replay.bad_lines, 1);
        assert_eq!(replay.session.duration_ms, 3_735_000);
        assert_eq!(replay.session.ended_at.as_deref(), Some("2026-10-02T16:15:20+09:00"));
    }

    #[test]
    fn editing_back_to_original_clears_edited_flag() {
        let lines: Vec<String> = [
            started(),
            segment("seg_000001", 0, 10, "元"),
            LogEvent::SegmentEdited { id: "seg_000001".into(), text: "新".into() },
            LogEvent::SegmentEdited { id: "seg_000001".into(), text: "元".into() },
        ]
        .iter()
        .map(encode_line)
        .collect();
        let s = replay_lines(&lines).unwrap().session;
        let seg = s.segments().next().unwrap();
        assert!(!seg.edited);
        assert_eq!(seg.text_original, None);
    }

    #[test]
    fn writer_appends_and_open_append_repairs_truncated_tail() {
        let dir = tempfile::tempdir().unwrap();
        {
            let log = SessionLog::create(dir.path()).unwrap();
            log.append(&started()).unwrap();
            log.append(&segment("seg_000001", 0, 10, "a")).unwrap();
            log.sync().unwrap();
        }
        // Simulate a crash mid-write.
        {
            let mut f = OpenOptions::new().append(true).open(dir.path().join(LOG_FILE)).unwrap();
            f.write_all(b"{\"v\":1,\"type\":\"seg").unwrap();
        }
        {
            let log = SessionLog::open_append(dir.path()).unwrap();
            log.append(&segment("seg_000002", 20, 30, "b")).unwrap();
        }
        let replay = replay(dir.path()).unwrap();
        assert_eq!(replay.session.segments().count(), 2);
        assert_eq!(replay.bad_lines, 1);
    }
}
