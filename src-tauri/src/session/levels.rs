//! The session's sound, kept for its picture (`levels.bin`): each side's level ten times a second,
//! the readings the live line is drawn from, so History and the finished session show the real
//! sound instead of only when each side spoke. After a 4-byte header, two bytes per 100 ms of
//! session time: 相手 (the louder of app and system sound), then 自分 (the mic), each the peak in
//! quarter dB above −60 dBFS. Paused time is silence.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::audio::level::FLOOR_DBFS;

pub const LEVELS_FILE: &str = "levels.bin";
const MAGIC: &[u8; 4] = b"KKL1";
/// Session time per reading.
pub const STEP_MS: u64 = 100;
/// Up to this many readings missed is a late tick, which repeats the last reading; more is a pause.
const LATE_TICKS: u64 = 5;
/// How much a quiet session's picture may be raised (`fit`).
const MAX_GAIN: f32 = 2.0;

fn encode(dbfs: f32) -> u8 {
    ((dbfs - FLOOR_DBFS) * 4.0).round().clamp(0.0, 240.0) as u8
}

fn decode(byte: u8) -> f32 {
    FLOOR_DBFS + f32::from(byte) / 4.0
}

/// How high the line stands for a level, 0..1 (`heightOf` in `src/lib/line.ts`).
pub fn height(dbfs: f32) -> f32 {
    let level = ((dbfs + 60.0) / 60.0).clamp(0.0, 1.0);
    ((level - 0.1) / 0.9).clamp(0.0, 1.0).powf(1.15)
}

/// Appends a recording's readings to its `levels.bin`.
pub struct LevelWriter {
    out: BufWriter<File>,
    /// Readings written so far: the index of the next one.
    next: u64,
    last: [u8; 2],
}

impl LevelWriter {
    pub fn create(folder: &Path) -> std::io::Result<Self> {
        let mut out = BufWriter::new(File::create(folder.join(LEVELS_FILE))?);
        out.write_all(MAGIC)?;
        Ok(Self { out, next: 0, last: [0, 0] })
    }

    /// The levels heard at session time `t_ms`, in dBFS (`FLOOR_DBFS` for a side not recorded).
    pub fn push(&mut self, t_ms: u64, others: f32, me: f32) -> std::io::Result<()> {
        let index = t_ms / STEP_MS;
        if index < self.next {
            return Ok(());
        }
        let missed = if index - self.next <= LATE_TICKS { self.last } else { [0, 0] };
        while self.next < index {
            self.out.write_all(&missed)?;
            self.next += 1;
        }
        self.last = [encode(others), encode(me)];
        self.out.write_all(&self.last)?;
        self.next += 1;
        Ok(())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// A session's readings as (相手, 自分) in dBFS; `None` for a session recorded before they were kept.
pub fn read(folder: &Path) -> Option<Vec<[f32; 2]>> {
    let bytes = std::fs::read(folder.join(LEVELS_FILE)).ok()?;
    let body = bytes.strip_prefix(MAGIC.as_slice())?;
    Some(body.as_chunks::<2>().0.iter().map(|&[others, me]| [decode(others), decode(me)]).collect())
}

/// How loud each side was in `n` equal slices of the session's first `end_ms`: the mean height of
/// its readings, 0..100. A slice shorter than a reading takes the reading it falls in.
pub fn slices(readings: &[[f32; 2]], end_ms: u64, n: usize) -> [Vec<u8>; 2] {
    let mut out = [vec![0; n], vec![0; n]];
    let per = end_ms.max(1) as f64 / STEP_MS as f64 / n.max(1) as f64;
    for k in 0..n {
        let a = (k as f64 * per) as usize;
        let b = (((k + 1) as f64 * per) as usize).max(a + 1);
        let Some(range) = readings.get(a..b.min(readings.len())) else { continue };
        if range.is_empty() {
            continue;
        }
        for (side, slot) in out.iter_mut().enumerate() {
            let sum: f32 = range.iter().map(|r| height(r[side])).sum();
            slot[k] = (sum / range.len() as f32 * 100.0).round() as u8;
        }
    }
    out
}

/// Raises a session's slices together so its loudest one stands full height, at most `MAX_GAIN`
/// times: the picture shows the shape of the session, not how loud it was recorded.
pub fn fit(sides: &mut [Vec<u8>; 2]) {
    let peak = sides.iter().flatten().copied().max().unwrap_or(0);
    if peak == 0 {
        return;
    }
    let gain = (100.0 / f32::from(peak)).min(MAX_GAIN);
    for v in sides.iter_mut().flatten() {
        *v = (f32::from(*v) * gain).round().min(100.0) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_round_trip_with_late_ticks_filled_and_pauses_silent() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = LevelWriter::create(dir.path()).unwrap();
        w.push(0, -20.0, FLOOR_DBFS).unwrap();
        w.push(150, -30.0, -10.0).unwrap();
        // Twice in one step: the first reading stays.
        w.push(160, -5.0, -5.0).unwrap();
        // Two readings late: steps 2 and 3 repeat the last one.
        w.push(400, -12.0, -60.0).unwrap();
        // After a pause: steps 5 to 19 are silent.
        w.push(2_000, -6.0, -6.0).unwrap();
        drop(w);
        let r = read(dir.path()).unwrap();
        assert_eq!(r.len(), 21);
        assert_eq!(r[0], [-20.0, FLOOR_DBFS]);
        assert_eq!(r[1], [-30.0, -10.0]);
        assert_eq!((r[2], r[3]), (r[1], r[1]));
        assert_eq!(r[4], [-12.0, -60.0]);
        assert!(r[5..20].iter().all(|x| *x == [FLOOR_DBFS, FLOOR_DBFS]));
        assert_eq!(r[20], [-6.0, -6.0]);
    }

    #[test]
    fn only_a_file_with_the_header_is_read() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_none());
        std::fs::write(dir.path().join(LEVELS_FILE), [1u8, 2, 3, 4]).unwrap();
        assert!(read(dir.path()).is_none());
    }

    #[test]
    fn slices_take_the_mean_height_and_never_run_dry() {
        let loud = [-6.0, FLOOR_DBFS];
        let quiet = [FLOOR_DBFS, FLOOR_DBFS];
        // 2 s of sound, then 2 s of silence, in 4 slices.
        let r: Vec<[f32; 2]> = std::iter::repeat_n(loud, 20).chain(std::iter::repeat_n(quiet, 20)).collect();
        let [o, m] = slices(&r, 4_000, 4);
        let full = (height(-6.0) * 100.0).round() as u8;
        assert_eq!(o, vec![full, full, 0, 0]);
        assert_eq!(m, vec![0; 4]);
        // Half sound, half silence: half as high.
        let [o, _] = slices(&r, 4_000, 1);
        assert_eq!(o, vec![(height(-6.0) * 50.0).round() as u8]);
        // More slices than readings: each slice takes the reading it falls in.
        let [o, _] = slices(&r[..3], 300, 6);
        assert_eq!(o, vec![full; 6]);
    }

    #[test]
    fn fit_raises_both_sides_together_up_to_twice() {
        let mut sides = [vec![0, 25, 50], vec![10, 0, 0]];
        fit(&mut sides);
        assert_eq!(sides, [vec![0, 50, 100], vec![20, 0, 0]]);
        let mut quiet = [vec![10, 20], vec![0, 0]];
        fit(&mut quiet);
        assert_eq!(quiet, [vec![20, 40], vec![0, 0]]);
        let mut silent = [vec![0, 0], vec![0, 0]];
        fit(&mut silent);
        assert_eq!(silent, [vec![0, 0], vec![0, 0]]);
    }
}
