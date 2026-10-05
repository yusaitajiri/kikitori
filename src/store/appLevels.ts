// The source picker's live waves: how loud each app it shows is, from the peak meters of the app's
// audio sessions (`audio://app-levels`). Each wave asks for its app with `watchApp`; the backend
// reads only those apps' meters, and sends anything only while one of them makes a sound. Kept
// outside React, so a wave and the same app's row in the list share one history.

import { commands } from "../ipc/commands";
import type { AppLevelsPayload } from "../ipc/types";
import { heightOf, settle } from "../lib/line";

/** One step of a wave: ten a second, like the recording's line. */
export const STEP_MS = 100;
/** Steps across a wave: two seconds. */
export const WAVE_STEPS = 20;
/** Reports come ten times a second while there is sound and stop in silence; this late, it has stopped. */
const STALE_MS = 400;
/** A height this low is flat. */
const FLAT = 0.02;

export type Track = {
  /** Heights 0..1, one per step, newest last; the oldest has just left the wave's left end. */
  heights: number[];
  /** When the newest step was taken (`performance.now()`). */
  at: number;
  /** The last reported height, and when it came. */
  level: number;
  heardAt: number;
};

export function newTrack(now: number): Track {
  return { heights: new Array<number>(WAVE_STEPS + 1).fill(0), at: now, level: 0, heardAt: -Infinity };
}

/** Brings a track up to `now`: a step every 100 ms, at the reported height while reports come, else flat. */
export function advance(track: Track, now: number): Track {
  // Nothing of an older sound would be left on the wave: start again from here.
  if (now - track.at > (WAVE_STEPS + 1) * STEP_MS) {
    track.heights.fill(0);
    track.at = now;
  }
  while (now - track.at >= STEP_MS) {
    track.at += STEP_MS;
    const level = track.at - track.heardAt <= STALE_MS ? track.level : 0;
    track.heights.push(settle(track.heights[track.heights.length - 1], level));
    track.heights.shift();
  }
  return track;
}

/** Nothing to show: the wave is flat all along and no sound has been reported lately. */
export function isQuiet(track: Track, now: number): boolean {
  const hearing = now - track.heardAt <= STALE_MS && track.level > FLAT;
  return !hearing && track.heights.every((h) => h <= FLAT);
}

type Watched = { track: Track; waves: number; wake: Set<() => void> };

const watched = new Map<number, Watched>();
let sent = "";
let syncing = false;

/** Tells the backend which apps to read, once for several waves that mount or leave together. */
function sync() {
  if (syncing) return;
  syncing = true;
  queueMicrotask(() => {
    syncing = false;
    const pids = [...watched.keys()].sort((a, b) => a - b);
    const key = pids.join(",");
    if (key === sent) return;
    sent = key;
    commands.watchAppLevels(pids).catch(() => {});
  });
}

/** Watches an app (its root PID) for a wave: `onSound` runs whenever a report says it plays. Gives its track and the way to stop. */
export function watchApp(pid: number, onSound: () => void): { track: Track; stop: () => void } {
  let w = watched.get(pid);
  if (!w) {
    w = { track: newTrack(performance.now()), waves: 0, wake: new Set() };
    watched.set(pid, w);
    sync();
  }
  const mine = w;
  mine.waves++;
  mine.wake.add(onSound);
  return {
    track: mine.track,
    stop: () => {
      mine.wake.delete(onSound);
      if (--mine.waves === 0 && watched.get(pid) === mine) {
        watched.delete(pid);
        sync();
      }
    },
  };
}

/** A report from the backend. */
export function hearAppLevels(p: AppLevelsPayload, now = performance.now()) {
  for (const { rootPid, dbfs } of p.levels) {
    const w = watched.get(rootPid);
    if (!w) continue;
    w.track.level = heightOf(dbfs);
    w.track.heardAt = now;
    if (w.track.level > FLAT) for (const wake of w.wake) wake();
  }
}
