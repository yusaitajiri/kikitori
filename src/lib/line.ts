// The line across the window. While recording it is drawn from the real levels: 相手 rises above
// it, 自分 dips below it, screenshots are ticks and a pause leaves a dotted gap. A finished
// session's line is its whole timeline at once, built from when each side spoke. Pure geometry.

import type { TimelineItem } from "../ipc/types";
import { levelFraction } from "./format";

/** One step of the live line (ten a second): each side's height, 0..1. */
export type Sample = { o: number; m: number; shot?: boolean; gap?: boolean };

/** A whole session: each side's height per slice (0..1) and the slices holding a screenshot. */
export type Shape = { o: number[]; m: number[]; shots: number[] };

/** Slices in a session's shape; the same count as `SessionSummary.activity` from Rust. */
export const SLICES = 128;

const r1 = (v: number) => Math.round(v * 10) / 10;

/** A smooth path through the points (Catmull-Rom as cubic Béziers). */
export function smoothPath(pts: readonly (readonly [number, number])[]): string {
  if (pts.length === 0) return "";
  let d = `M${r1(pts[0][0])} ${r1(pts[0][1])}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] ?? pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] ?? p2;
    d +=
      `C${r1(p1[0] + (p2[0] - p0[0]) / 6)} ${r1(p1[1] + (p2[1] - p0[1]) / 6)} ` +
      `${r1(p2[0] - (p3[0] - p1[0]) / 6)} ${r1(p2[1] - (p3[1] - p1[1]) / 6)} ${r1(p2[0])} ${r1(p2[1])}`;
  }
  return d;
}

/** A level in dBFS as the line's height, 0..1; room noise stays flat. */
export function heightOf(dbfs: number | undefined): number {
  const l = levelFraction(dbfs);
  return Math.pow(Math.min(1, Math.max(0, (l - 0.1) / 0.9)), 1.15);
}

/** One step toward a new height: quick to rise, slower to fall, like a meter. */
export function settle(cur: number, target: number): number {
  return cur + (target - cur) * (target > cur ? 0.6 : 0.3);
}

/** Sizes in px: the baseline's y, how far each side reaches, one step's width, where the pen starts and stops. */
export type Geometry = { base: number; up: number; down: number; step: number; startX: number; endPad: number };

export type LiveLine = {
  /** 相手's line, 自分's line, and the steps still being written (both sides). */
  o: string;
  m: string;
  pending: string;
  /** The tint under the line, up to its top edge: closed shapes that end just below the baseline. */
  fill: string;
  /** The line not written yet: from the end of the line, back down to the baseline, to `width`. */
  ahead: string;
  /** Pauses as [from, to] on the baseline, screenshots as x positions. */
  gaps: [number, number][];
  ticks: number[];
  /** Where the visible line starts, and the pen tip. */
  x0: number;
  pen: [number, number];
};

/** How far (px) the line not written yet takes to come back down from the end of the line to the baseline. */
const SLOPE = 12;

/** The area under a smooth path from `smoothPath`, closed 1 px below the baseline so it overlaps the tint below. */
function under(curve: string, xa: number, xb: number, base: number, tail = ""): string {
  if (!curve) return "";
  return `M${r1(xa)} ${r1(base + 1)}L${curve.slice(1)}${tail}L${r1(xb)} ${r1(base + 1)}Z`;
}

/** From the end of the line at `y` back down to the baseline over `SLOPE` px: the curve alone, after an M or L at the end. */
function slopeDown(x: number, y: number, base: number): string {
  if (base - y < 0.5) return "";
  const h = SLOPE / 2;
  return `C${r1(x + h)} ${r1(y)} ${r1(x + h)} ${r1(base)} ${r1(x + SLOPE)} ${r1(base)}`;
}

/**
 * The live line in a box `width` wide. The pen moves right from `startX` until it is `endPad`
 * from the end; after that the line scrolls under it. `frac` (0..1) is how far the pen has moved
 * past the newest step; the last `pending` steps are still being written. With one source its
 * line rises above the baseline whichever side it is.
 */
export function liveLine(samples: readonly Sample[], frac: number, pending: number, width: number, g: Geometry, two: boolean): LiveLine {
  const n = samples.length;
  const t = Math.max(0, n - 1 + frac);
  const px = Math.min(width - g.endPad, g.startX + t * g.step);
  const xi = (i: number) => px - (t - i) * g.step;
  const lo = Math.max(0, Math.floor(t - (px + 4 * g.step) / g.step));
  const written = n - Math.max(0, Math.min(pending, n));
  const up = (s: Sample) => (two ? s.o : Math.max(s.o, s.m));
  // A light 1-2-1 blur, so a step that stands alone does not look like a spike.
  const soft = (i: number, f: (s: Sample) => number) => {
    const s = samples[i];
    const a = samples[i - 1];
    const c = samples[i + 1];
    const v = f(s);
    return ((a && !a.gap ? f(a) : v) + 2 * v + (c && !c.gap ? f(c) : v)) / 4;
  };

  const runs: number[][] = [[]];
  const gaps: [number, number][] = [];
  const ticks: number[] = [];
  let gapFrom: number | null = null;
  for (let i = lo; i < n; i++) {
    const s = samples[i];
    if (s.gap) {
      if (gapFrom === null) {
        gapFrom = xi(i);
        runs.push([]);
      }
      continue;
    }
    if (gapFrom !== null) {
      gaps.push([gapFrom, xi(i)]);
      gapFrom = null;
    }
    runs[runs.length - 1].push(i);
    if (s.shot) ticks.push(xi(i));
  }
  if (gapFrom !== null) gaps.push([gapFrom, px]);

  let o = "";
  let m = "";
  let pend = "";
  let fill = "";
  const yo = (i: number) => g.base - soft(i, up) * g.up;
  const ym = (i: number) => g.base + soft(i, (s) => s.m) * g.down;
  const last = samples[n - 1];
  // The line ends at the newest step, or past it at the pen while it moves; what is not written
  // yet comes back down to the baseline from there.
  const end: [number, number] = last && !last.gap ? [px, yo(n - 1)] : [px, g.base];
  const slope = slopeDown(...end, g.base);
  const slopeEnd = end[0] + (slope ? SLOPE : 0);
  for (const run of runs) {
    if (!run.length) continue;
    const first = run[0];
    const final = run[run.length - 1];
    // A stretch rises from the baseline where it starts (the start, or the end of a pause) and goes
    // back down to it before a pause, so the line and the tint under it never end in mid-air.
    const head: [number, number] | null = first === 0 || samples[first - 1]?.gap ? [xi(first) - g.step, g.base] : null;
    const foot: [number, number] | null = samples[final + 1]?.gap ? [xi(final) + g.step, g.base] : null;
    const path = (idx: number[], y: (i: number) => number) => {
      const pts = idx.map((i): [number, number] => [xi(i), y(i)]);
      if (head && idx[0] === first) pts.unshift(head);
      if (foot && idx[idx.length - 1] === final) pts.push(foot);
      return smoothPath(pts);
    };
    const done = run.filter((i) => i < written);
    const open = run.filter((i) => i >= written - 1);
    if (done.length) {
      o += path(done, yo);
      if (two) m += path(done, ym);
    }
    if (written < n && open.length > 1) {
      pend += path(open, yo);
      if (two) pend += path(open, ym);
    }
    // The tint rises to the top of the line, written or not, and down its slope at the end.
    const ends = final === n - 1;
    const xa = head ? head[0] : xi(first);
    const xb = ends ? slopeEnd : foot ? foot[0] : xi(final);
    const tail = ends ? `${frac > 0 ? `L${r1(end[0])} ${r1(end[1])}` : ""}${slope}` : "";
    fill += under(path(run, yo), xa, xb, g.base, tail);
  }

  let pen: [number, number] = [px, g.base];
  if (last && !last.gap && written === n) {
    // The pen is a little past the newest step; carry the newest heights to it.
    const xl = xi(n - 1);
    pen = end;
    if (frac > 0) {
      o += `M${r1(xl)} ${r1(yo(n - 1))}L${r1(px)} ${r1(yo(n - 1))}`;
      if (two) m += `M${r1(xl)} ${r1(ym(n - 1))}L${r1(px)} ${r1(ym(n - 1))}`;
    }
  } else if (written < n && written > 0) {
    // While finishing, the pen waits where the written part ends.
    const i = written - 1;
    pen = [xi(i), samples[i].gap ? g.base : yo(i)];
  }
  const ahead = `M${r1(end[0])} ${r1(end[1])}${slope}H${r1(Math.max(width, slopeEnd))}`;
  return { o, m, pending: pend, fill, ahead, gaps, ticks, x0: n ? Math.max(0, xi(lo)) : px, pen };
}

export type MapLine = { o: string; m: string; fill: string; ticks: number[]; x0: number; x1: number };

/** A whole session across `width`, `pad` from each end; heights scaled by `up` and `down`. */
export function mapLine(shape: Shape, width: number, pad: number, base: number, up: number, down: number, two: boolean): MapLine {
  const n = Math.max(shape.o.length, shape.m.length, 2);
  const x = (i: number) => pad + (i / (n - 1)) * (width - 2 * pad);
  const at = (a: number[], i: number) => a[i] ?? 0;
  const top = (i: number) => (two ? at(shape.o, i) : Math.max(at(shape.o, i), at(shape.m, i)));
  const blur = (f: (i: number) => number, i: number) => (f(Math.max(0, i - 1)) + 2 * f(i) + f(Math.min(n - 1, i + 1))) / 4;
  const idx = Array.from({ length: n }, (_, i) => i);
  const o = smoothPath(idx.map((i) => [x(i), base - blur(top, i) * up]));
  return {
    o,
    m: two ? smoothPath(idx.map((i) => [x(i), base + blur((k) => at(shape.m, k), i) * down])) : "",
    fill: under(o, x(0), x(n - 1), base),
    ticks: shape.shots.map(x),
    x0: x(0),
    x1: x(n - 1),
  };
}

/** A session's shape from its timeline: the share of each slice covered by each side's lines. */
export function shapeOf(items: readonly TimelineItem[], durationMs: number | undefined, n = SLICES): Shape {
  let end = durationMs ?? 0;
  for (const i of items) if (i.kind === "segment") end = Math.max(end, i.tEndMs);
  const span = Math.max(end, 1) / n;
  const o: number[] = new Array(n).fill(0);
  const m: number[] = new Array(n).fill(0);
  for (const i of items) {
    if (i.kind !== "segment") continue;
    const side = i.source === "mic" ? m : o;
    const a = i.tStartMs;
    const b = Math.max(i.tEndMs, a);
    for (let k = Math.min(n - 1, Math.floor(a / span)); k <= Math.min(n - 1, Math.floor(b / span)); k++) {
      const cover = Math.min((k + 1) * span, b) - Math.max(k * span, a);
      if (cover > 0) side[k] += cover / span;
    }
  }
  const shots = new Set<number>();
  for (const i of items) if (i.kind === "screenshot") shots.add(Math.min(n - 1, Math.floor(i.tMs / span)));
  return { o: o.map((v) => Math.min(1, v)), m: m.map((v) => Math.min(1, v)), shots: [...shots].sort((a, b) => a - b) };
}

/** A shape from `SessionSummary.activity` (percentages). */
export function shapeFromActivity(a: { others: number[]; me: number[]; shots: number[] }): Shape {
  return { o: a.others.map((v) => v / 100), m: a.me.map((v) => v / 100), shots: a.shots };
}

/**
 * A finished session's shape: how loud each side was (`sound`, percentages) when the levels were
 * kept, else when each side spoke; the screenshots always from its timeline, so deleting one shows.
 */
export function sessionShape(items: readonly TimelineItem[], durationMs: number | undefined, sound?: { others: number[]; me: number[] }): Shape {
  const n = sound ? Math.max(sound.others.length, sound.me.length, 1) : SLICES;
  const s = shapeOf(items, durationMs, n);
  return sound ? { o: sound.others.map((v) => v / 100), m: sound.me.map((v) => v / 100), shots: s.shots } : s;
}

/** A disc whose rim is a gentle wave: `lobes` bumps of height `amp` around radius `r`, turned by `phase`. */
export function blobPath(cx: number, cy: number, r: number, amp: number, lobes: number, phase: number): string {
  if (r <= 0) return "";
  const n = 96;
  let d = "";
  for (let i = 0; i < n; i++) {
    const th = (i / n) * Math.PI * 2;
    const rr = r + amp * Math.sin(lobes * th + phase);
    d += `${i ? "L" : "M"}${r1(cx + rr * Math.cos(th))} ${r1(cy + rr * Math.sin(th))}`;
  }
  return `${d}Z`;
}
