//! The live session: receives ASR results, writes the event log, keeps the in-memory
//! timeline, applies the echo guard and emits UI events.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use super::log::{self, LogEvent, SessionLog};
use super::model::{MarkerKind, Session, SourceId, TimelineItem};
use crate::asr::echo_guard::{self, Line};
use crate::asr::worker::{FinalResult, SegmentSink};
use crate::audio::dsp::SessionIds;
use crate::error::is_disk_full;
use crate::events::{self, EventSink, PartialPayload, RemovedPayload};

pub struct SessionStore {
    pub id: String,
    pub folder: PathBuf,
    pub log: SessionLog,
    pub session: Mutex<Session>,
    pub ids: Arc<SessionIds>,
    events: Arc<dyn EventSink>,
    echo_guard: bool,
    /// Set when a write failed because the disk is full; the recorder stops capture.
    pub disk_full: AtomicBool,
    /// Marks waiting for their line (FR-07): (utterance ID, when the mark was made).
    pending_marks: Mutex<Vec<(String, u64)>>,
}

/// Which of an utterance's segments `(id, start, end)` a mark made at `t_ms` belongs to: the one
/// being said then, else the last that had started, else the first.
pub fn pick_segment<'a>(segments: &[(&'a str, u64, u64)], t_ms: u64) -> Option<&'a str> {
    segments
        .iter()
        .find(|(_, s, e)| *s <= t_ms && t_ms <= *e)
        .or_else(|| segments.iter().rfind(|(_, s, _)| *s <= t_ms))
        .or_else(|| segments.first())
        .map(|(id, _, _)| *id)
}

impl SessionStore {
    pub fn new(
        folder: &Path,
        log: SessionLog,
        session: Session,
        ids: Arc<SessionIds>,
        events: Arc<dyn EventSink>,
        echo_guard: bool,
    ) -> Self {
        Self {
            id: session.id.clone(),
            folder: folder.to_path_buf(),
            log,
            session: Mutex::new(session),
            ids,
            events,
            echo_guard,
            disk_full: AtomicBool::new(false),
            pending_marks: Mutex::new(Vec::new()),
        }
    }

    /// Appends to the log and applies to the timeline. Logs first: the log is the truth.
    pub fn record(&self, event: LogEvent) -> bool {
        if let Err(err) = self.log.append(&event) {
            tracing::error!("session log write failed: {err}");
            if is_disk_full(&err) {
                self.disk_full.store(true, Ordering::SeqCst);
            }
            return false;
        }
        log::apply(&mut self.session.lock(), event);
        true
    }

    pub fn snapshot(&self) -> Session {
        self.session.lock().clone()
    }

    pub fn add_marker(&self, t_ms: u64, kind: MarkerKind, detail: Option<String>) {
        let id = self.ids.next_marker();
        if self.record(LogEvent::Marker { id: id.clone(), t_ms, kind, detail: detail.clone() }) {
            events::emit(
                self.events.as_ref(),
                events::TRANSCRIPT_MARKER,
                &serde_json::json!({ "kind": "marker", "id": id, "tMs": t_ms, "type": kind, "detail": detail }),
            );
        }
    }

    /// Marks a line important, or takes the mark off, and tells the UI.
    pub fn set_important(&self, segment_id: &str, important: bool) -> bool {
        if !self.record(LogEvent::SegmentMarked { id: segment_id.to_string(), important }) {
            return false;
        }
        let seg = self.session.lock().segments().find(|s| s.id == segment_id).cloned();
        if let Some(seg) = seg {
            events::emit(self.events.as_ref(), events::TRANSCRIPT_SEGMENT_UPDATED, &seg);
        }
        true
    }

    /// Marks the lines of `utterances` that were being said at `t_ms` (FR-07). An utterance whose
    /// text has not arrived yet keeps the mark until it does.
    pub fn mark_utterances(&self, utterances: &[String], t_ms: u64) {
        for utt in utterances {
            match self.segment_of(utt, t_ms) {
                Some(id) => {
                    self.set_important(&id, true);
                }
                None => self.pending_marks.lock().push((utt.clone(), t_ms)),
            }
        }
    }

    fn segment_of(&self, utterance_id: &str, t_ms: u64) -> Option<String> {
        let session = self.session.lock();
        let segments: Vec<(&str, u64, u64)> = session
            .segments()
            .filter(|s| s.utterance_id.as_deref() == Some(utterance_id))
            .map(|s| (s.id.as_str(), s.t_start_ms, s.t_end_ms))
            .collect();
        pick_segment(&segments, t_ms).map(String::from)
    }

    /// Places the marks waiting for these utterances; one that produced no line drops its mark.
    fn resolve_marks(&self, utterances: &[String]) {
        let due: Vec<(String, u64)> = {
            let mut pending = self.pending_marks.lock();
            let (due, keep) = pending.drain(..).partition(|(u, _)| utterances.contains(u));
            *pending = keep;
            due
        };
        for (utt, t_ms) in due {
            if let Some(id) = self.segment_of(&utt, t_ms) {
                self.set_important(&id, true);
            }
        }
    }

    fn lines_of(session: &Session, mic: bool) -> Vec<(String, u64, u64, String)> {
        session
            .segments()
            .filter(|s| (s.source == SourceId::Mic) == mic)
            .map(|s| (s.id.clone(), s.t_start_ms, s.t_end_ms, s.text.clone()))
            .collect()
    }

    fn remove_segment(&self, id: &str, reason: &str) {
        if self.record(LogEvent::SegmentRemoved { id: id.to_string(), reason: reason.to_string() }) {
            events::emit(
                self.events.as_ref(),
                events::TRANSCRIPT_SEGMENT_REMOVED,
                &RemovedPayload { id: id.to_string(), reason: reason.to_string() },
            );
        }
    }
}

impl SegmentSink for SessionStore {
    fn on_final(&self, result: FinalResult) {
        let mut finalized: std::collections::HashSet<String> = std::collections::HashSet::new();
        for seg in result.segments {
            finalized.insert(seg.utterance_id.clone());
            let guard = self.echo_guard && self.session.lock().two_sides();

            // A 自分 line that arrives after its 相手 match is dropped before it is written.
            if guard && result.source == SourceId::Mic {
                let others = Self::lines_of(&self.session.lock(), false);
                let others: Vec<Line> =
                    others.iter().map(|(id, s, e, t)| Line { id, t_start_ms: *s, t_end_ms: *e, text: t }).collect();
                let mine = Line { id: "", t_start_ms: seg.t_start_ms, t_end_ms: seg.t_end_ms, text: &seg.text };
                if echo_guard::is_echo_of_any(&mine, &others) {
                    tracing::info!("echo guard dropped a mic segment ({} chars)", seg.text.chars().count());
                    continue;
                }
            }

            let id = self.ids.next_segment();
            let event = LogEvent::Segment {
                id: id.clone(),
                utterance_id: seg.utterance_id.clone(),
                source: result.source,
                t_start_ms: seg.t_start_ms,
                t_end_ms: seg.t_end_ms,
                text: seg.text.clone(),
                no_speech: (seg.no_speech * 1000.0).round() / 1000.0,
            };
            if !self.record(event) {
                continue;
            }
            let payload = {
                let session = self.session.lock();
                session.items.iter().rev().find(|i| i.id() == id).cloned()
            };
            if let Some(TimelineItem::Segment(s)) = payload {
                events::emit(self.events.as_ref(), events::TRANSCRIPT_SEGMENT, &s);
            }

            // A 相手 line was finalized: remove 自分 lines near it that now look like echoes of
            // the 相手 lines overlapping them (this one included).
            if guard && result.source != SourceId::Mic {
                let (mine, others) = {
                    let session = self.session.lock();
                    (Self::lines_of(&session, true), Self::lines_of(&session, false))
                };
                let mine_lines: Vec<Line> =
                    mine.iter().map(|(id, s, e, t)| Line { id, t_start_ms: *s, t_end_ms: *e, text: t }).collect();
                let other_lines: Vec<Line> =
                    others.iter().map(|(id, s, e, t)| Line { id, t_start_ms: *s, t_end_ms: *e, text: t }).collect();
                let new_line = Line { id: &id, t_start_ms: seg.t_start_ms, t_end_ms: seg.t_end_ms, text: &seg.text };
                let echoes: Vec<String> = echo_guard::echoes_of_others(&new_line, &mine_lines, &other_lines)
                    .into_iter()
                    .map(String::from)
                    .collect();
                for echo_id in echoes {
                    self.remove_segment(&echo_id, "echo");
                }
            }
        }
        let done: Vec<String> = result.pieces.iter().map(|p| p.utterance_id.clone()).collect();
        self.resolve_marks(&done);
        // Clear provisional text for utterances that produced no line at all.
        for piece in result.pieces {
            if !finalized.contains(&piece.utterance_id) {
                events::emit(
                    self.events.as_ref(),
                    events::TRANSCRIPT_PARTIAL,
                    &PartialPayload {
                        source: result.source,
                        utterance_id: piece.utterance_id,
                        text: String::new(),
                        t_start_ms: None,
                    },
                );
            }
        }
    }

    fn on_partial(&self, source: SourceId, utterance_id: &str, start_ms: u64, text: &str) {
        events::emit(
            self.events.as_ref(),
            events::TRANSCRIPT_PARTIAL,
            &PartialPayload {
                source,
                utterance_id: utterance_id.to_string(),
                text: text.to_string(),
                t_start_ms: Some(start_ms),
            },
        );
    }

    fn on_language(&self, language: &str) {
        tracing::info!("language locked: {language}");
        self.record(LogEvent::LanguageDetected { language: language.to_string() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::worker::{FinalSegment, PieceInfo};
    use crate::events::test_sink::Recorded;
    use crate::session::model::{ModelRef, SourceInfo};

    fn store(dir: &Path, two: bool, echo: bool) -> (SessionStore, Arc<Recorded>) {
        let mut sources =
            vec![SourceInfo { id: SourceId::App, label: "相手".into(), exe: None, device: None, name: None }];
        if two {
            sources.push(SourceInfo { id: SourceId::Mic, label: "自分".into(), exe: None, device: None, name: None });
        }
        let session = Session {
            v: 1,
            id: "01J".into(),
            title: "t".into(),
            started_at: "2026-10-02T15:13:05+09:00".into(),
            ended_at: None,
            duration_ms: 0,
            sources,
            model: ModelRef::default(),
            language: "ja".into(),
            gpu: false,
            items: vec![],
            unprocessed_ms: 0,
        };
        let log = SessionLog::create(dir).unwrap();
        let rec = Arc::new(Recorded::default());
        (SessionStore::new(dir, log, session, Arc::new(SessionIds::default()), rec.clone(), echo), rec)
    }

    fn result(source: SourceId, utt: &str, s: u64, e: u64, text: &str) -> FinalResult {
        FinalResult {
            source,
            pieces: vec![PieceInfo { utterance_id: utt.into(), start_ms: s, end_ms: e }],
            segments: vec![FinalSegment {
                utterance_id: utt.into(),
                t_start_ms: s,
                t_end_ms: e,
                text: text.into(),
                no_speech: 0.01,
            }],
        }
    }

    #[test]
    fn segments_are_logged_numbered_and_emitted() {
        let dir = tempfile::tempdir().unwrap();
        let (st, rec) = store(dir.path(), false, true);
        st.on_final(result(SourceId::App, "utt_000001", 1000, 2000, "一つ目"));
        st.on_final(result(SourceId::App, "utt_000002", 3000, 4000, "二つ目"));
        let emitted = rec.named(events::TRANSCRIPT_SEGMENT);
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[1]["id"], "seg_000002");
        assert_eq!(emitted[1]["utteranceId"], "utt_000002");
        // This log has no session_started, so it cannot be replayed on its own.
        assert!(log::replay(dir.path()).is_err());
        assert_eq!(st.snapshot().segments().count(), 2);
    }

    #[test]
    fn echo_removed_when_others_line_arrives_later() {
        let dir = tempfile::tempdir().unwrap();
        let (st, rec) = store(dir.path(), true, true);
        st.on_final(result(SourceId::Mic, "utt_000001", 10_200, 12_900, "資料を共有します"));
        st.on_final(result(SourceId::App, "utt_000002", 10_000, 13_000, "資料を共有します。"));
        let removed = rec.named(events::TRANSCRIPT_SEGMENT_REMOVED);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0]["id"], "seg_000001");
        assert_eq!(removed[0]["reason"], "echo");
        let s = st.snapshot();
        assert_eq!(s.segments().count(), 1);
        assert_eq!(s.segments().next().unwrap().source, SourceId::App);
    }

    #[test]
    fn echo_dropped_when_mine_arrives_after_match() {
        let dir = tempfile::tempdir().unwrap();
        let (st, rec) = store(dir.path(), true, true);
        st.on_final(result(SourceId::App, "utt_000001", 10_000, 13_000, "資料を共有します"));
        st.on_final(result(SourceId::Mic, "utt_000002", 10_300, 13_100, "資料を共有します"));
        assert_eq!(rec.named(events::TRANSCRIPT_SEGMENT).len(), 1);
        assert_eq!(st.snapshot().segments().count(), 1);
    }

    #[test]
    fn echo_guard_off_keeps_both() {
        let dir = tempfile::tempdir().unwrap();
        let (st, _) = store(dir.path(), true, false);
        st.on_final(result(SourceId::App, "utt_000001", 10_000, 13_000, "資料を共有します"));
        st.on_final(result(SourceId::Mic, "utt_000002", 10_300, 13_100, "資料を共有します"));
        assert_eq!(st.snapshot().segments().count(), 2);
    }

    #[test]
    fn a_mark_picks_the_segment_being_said() {
        let segs = [("a", 1000, 2000), ("b", 2000, 3000), ("c", 3500, 4000)];
        assert_eq!(pick_segment(&segs, 2500), Some("b"));
        // Between segments: the last one that had started.
        assert_eq!(pick_segment(&segs, 3200), Some("b"));
        assert_eq!(pick_segment(&segs, 500), Some("a"));
        assert_eq!(pick_segment(&[], 500), None);
    }

    #[test]
    fn a_mark_waits_for_its_line_and_lands_when_it_arrives() {
        let dir = tempfile::tempdir().unwrap();
        let (st, rec) = store(dir.path(), false, true);
        st.on_final(result(SourceId::App, "utt_000001", 1000, 2000, "一つ目"));
        // Marked while utt_000002 is still being said: nothing to mark yet.
        st.mark_utterances(&["utt_000002".into()], 3500);
        assert!(st.snapshot().segments().all(|s| !s.important));
        st.on_final(result(SourceId::App, "utt_000002", 3000, 4000, "二つ目"));
        let s = st.snapshot();
        let marked: Vec<&str> = s.segments().filter(|s| s.important).map(|s| s.text.as_str()).collect();
        assert_eq!(marked, ["二つ目"]);
        let updated = rec.named(events::TRANSCRIPT_SEGMENT_UPDATED);
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0]["important"], true);
        // A line already there is marked at once, and the mark can be taken off.
        st.mark_utterances(&["utt_000001".into()], 2100);
        assert_eq!(st.snapshot().segments().filter(|s| s.important).count(), 2);
        assert!(st.set_important("seg_000001", false));
        assert_eq!(st.snapshot().segments().filter(|s| s.important).count(), 1);
    }

    #[test]
    fn a_mark_on_an_utterance_without_a_line_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (st, _) = store(dir.path(), false, true);
        st.mark_utterances(&["utt_000001".into()], 500);
        st.on_final(FinalResult {
            source: SourceId::App,
            pieces: vec![PieceInfo { utterance_id: "utt_000001".into(), start_ms: 0, end_ms: 900 }],
            segments: vec![],
        });
        assert!(st.pending_marks.lock().is_empty());
    }

    #[test]
    fn empty_result_clears_partial() {
        let dir = tempfile::tempdir().unwrap();
        let (st, rec) = store(dir.path(), false, true);
        st.on_final(FinalResult {
            source: SourceId::App,
            pieces: vec![PieceInfo { utterance_id: "utt_000009".into(), start_ms: 0, end_ms: 100 }],
            segments: vec![],
        });
        let partials = rec.named(events::TRANSCRIPT_PARTIAL);
        assert_eq!(partials.len(), 1);
        assert_eq!(partials[0]["text"], "");
    }
}
