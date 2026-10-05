// The live line's history, kept outside React so a recording keeps its line while History or
// Settings is open. App fills it from the backend's events; the deck only draws it.

import type { LevelsPayload } from "../ipc/types";
import { heightOf, settle, type Sample } from "../lib/line";

/** Ten minutes of steps; older ones have long scrolled out of view. */
const MAX = 6000;
/** A pause leaves this many empty steps in the line (0.8 s). */
const GAP = 8;

class LineHistory {
  sessionId?: string;
  samples: Sample[] = [];
  /** When the newest step arrived (`performance.now()`), so the pen can glide between steps. */
  at = 0;
  private o = 0;
  private m = 0;

  /** A new recording starts an empty line; the same one keeps its own. */
  start(sessionId: string) {
    if (this.sessionId === sessionId) return;
    this.sessionId = sessionId;
    this.samples = [];
    this.o = 0;
    this.m = 0;
    this.at = performance.now();
  }

  /** One step from the levels (10 Hz): 相手 is the louder of app and system, 自分 the mic. */
  push(levels: LevelsPayload) {
    this.o = settle(this.o, Math.max(heightOf(levels.app), heightOf(levels.system)));
    this.m = settle(this.m, heightOf(levels.mic));
    this.samples.push({ o: this.o, m: this.m });
    if (this.samples.length > MAX) this.samples.splice(0, this.samples.length - MAX);
    this.at = performance.now();
  }

  /** A screenshot was taken: a tick at the newest step. */
  shot() {
    const s = this.samples[this.samples.length - 1];
    if (s) s.shot = true;
  }

  /** Recording resumed after a pause: a dotted gap in the line. */
  gap() {
    for (let i = 0; i < GAP; i++) this.samples.push({ o: 0, m: 0, gap: true });
    this.o = 0;
    this.m = 0;
    this.at = performance.now();
  }
}

export const lineHistory = new LineHistory();
