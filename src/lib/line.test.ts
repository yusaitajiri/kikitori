import { describe, expect, it } from "vitest";
import type { TimelineItem } from "../ipc/types";
import { blobPath, heightOf, liveLine, mapLine, settle, shapeFromActivity, shapeOf, smoothPath, type Geometry, type Sample } from "./line";

const G: Geometry = { base: 30, up: 20, down: 15, step: 2, startX: 48, endPad: 40 };
const ends = (d: string) => [...d.matchAll(/(-?[\d.]+) (-?[\d.]+)(?=C|M|L|$)/g)].map((m) => [Number(m[1]), Number(m[2])]);
const quiet = (n: number): Sample[] => Array.from({ length: n }, () => ({ o: 0, m: 0 }));

describe("heightOf and settle", () => {
  it("keeps room noise flat and loud speech high", () => {
    expect(heightOf(-58)).toBe(0);
    expect(heightOf(undefined)).toBe(0);
    expect(heightOf(-20)).toBeGreaterThan(0.5);
    expect(heightOf(0)).toBe(1);
  });

  it("rises faster than it falls", () => {
    expect(settle(0, 1)).toBeGreaterThan(1 - settle(1, 0));
  });
});

describe("smoothPath", () => {
  it("passes through every point", () => {
    const pts: [number, number][] = [[0, 0], [10, 5], [20, 0]];
    expect(ends(smoothPath(pts))).toEqual(pts);
    expect(smoothPath([])).toBe("");
  });
});

describe("liveLine", () => {
  it("moves the pen right from the start until it nears the end, then scrolls", () => {
    expect(liveLine([], 0, 0, 400, G, true).pen).toEqual([48, 30]);
    expect(liveLine(quiet(11), 0, 0, 400, G, true).pen[0]).toBe(68);
    const full = liveLine(quiet(1000), 0.5, 0, 400, G, true);
    expect(full.pen[0]).toBe(360);
    expect(full.x0).toBe(0);
  });

  it("draws 相手 above the line and 自分 below it", () => {
    const s: Sample[] = [...quiet(5), { o: 1, m: 0 }, ...quiet(5), { o: 0, m: 1 }, ...quiet(5)];
    const l = liveLine(s, 0, 0, 400, G, true);
    expect(Math.min(...ends(l.o).map((p) => p[1]))).toBeLessThan(30);
    expect(Math.max(...ends(l.o).map((p) => p[1]))).toBe(30);
    expect(Math.max(...ends(l.m).map((p) => p[1]))).toBeGreaterThan(30);
  });

  it("puts a single source above the line, whichever side it is", () => {
    const l = liveLine([...quiet(3), { o: 0, m: 1 }, ...quiet(3)], 0, 0, 400, G, false);
    expect(l.m).toBe("");
    expect(Math.min(...ends(l.o).map((p) => p[1]))).toBeLessThan(30);
  });

  it("leaves a gap for a pause and a tick for a screenshot", () => {
    const s: Sample[] = [...quiet(4), { o: 0, m: 0, shot: true }, ...quiet(2), ...Array.from({ length: 3 }, () => ({ o: 0, m: 0, gap: true })), ...quiet(4)];
    const l = liveLine(s, 0, 0, 400, G, true);
    expect(l.ticks).toEqual([56]);
    expect(l.gaps).toEqual([[62, 68]]);
    expect(l.o.match(/M/g)).toHaveLength(2);
  });

  it("draws the steps still being written apart, with the pen where the written part ends", () => {
    const l = liveLine(quiet(20), 0, 5, 400, G, true);
    expect(l.pending).not.toBe("");
    expect(l.pen[0]).toBe(48 + 14 * 2);
  });

  it("rises from the baseline where it starts and goes back down to it before a pause", () => {
    const loud = (n: number): Sample[] => Array.from({ length: n }, () => ({ o: 1, m: 1 }));
    const s: Sample[] = [...loud(5), ...Array.from({ length: 3 }, () => ({ o: 0, m: 0, gap: true })), ...loud(5)];
    const l = liveLine(s, 0, 0, 400, G, true);
    const [before, after] = l.o.split("M").filter(Boolean).map((d) => ends(`M${d}`));
    // Steps at 48, 50 … 56, the pause from 58, steps again from 64.
    expect(before[0]).toEqual([46, 30]);
    expect(before.at(-1)).toEqual([58, 30]);
    expect(after[0]).toEqual([62, 30]);
    expect(ends(l.m)[0]).toEqual([46, 30]);
  });

  it("fills the tint up to the line and down its slope, just past the baseline", () => {
    const l = liveLine([...quiet(5), { o: 1, m: 0 }], 0, 0, 400, G, true);
    const pts = [...l.fill.matchAll(/(-?[\d.]+) (-?[\d.]+)/g)].map((m) => [Number(m[1]), Number(m[2])]);
    expect(l.fill).toMatch(/^M46 31L46 30/);
    expect(l.fill).toMatch(/L70 31Z$/);
    expect(Math.min(...pts.map((p) => p[1]))).toBeLessThan(30);
    expect(Math.max(...pts.map((p) => p[1]))).toBe(31);
  });

  it("carries on unwritten from the end of the line, back down to the baseline, to the edge", () => {
    expect(liveLine(quiet(6), 0, 0, 400, G, true).ahead).toBe("M58 30H400");
    const l = liveLine([...quiet(5), { o: 1, m: 0 }], 0, 0, 400, G, true);
    expect(l.ahead.startsWith(`M${l.pen[0]} ${l.pen[1]}C`)).toBe(true);
    expect(l.ahead).toMatch(/ 70 30H400$/);
    expect(l.pen[1]).toBeLessThan(30);
  });
});

describe("shapes", () => {
  const seg = (source: "app" | "mic", a: number, b: number): TimelineItem => ({ kind: "segment", id: `${source}${a}`, source, tStartMs: a, tEndMs: b, text: "", edited: false });

  it("measures how much of each slice each side spoke", () => {
    const items: TimelineItem[] = [seg("app", 0, 2000), seg("mic", 3000, 3500), { kind: "screenshot", id: "s", tMs: 3900, file: "", width: 1, height: 1 }];
    const s = shapeOf(items, 4000, 4);
    expect(s.o).toEqual([1, 1, 0, 0]);
    expect(s.m).toEqual([0, 0, 0, 0.5]);
    expect(s.shots).toEqual([3]);
  });

  it("reads the summary's percentages", () => {
    expect(shapeFromActivity({ others: [100, 50], me: [0, 25], shots: [1] })).toEqual({ o: [1, 0.5], m: [0, 0.25], shots: [1] });
  });

  it("spreads a shape across the width between its pads", () => {
    const l = mapLine({ o: [0, 1, 0], m: [0, 0, 1], shots: [1] }, 200, 10, 30, 20, 10, true);
    expect([l.x0, l.x1]).toEqual([10, 190]);
    expect(l.ticks).toEqual([100]);
    expect(Math.min(...ends(l.o).map((p) => p[1]))).toBeLessThan(30);
    expect(Math.max(...ends(l.m).map((p) => p[1]))).toBeGreaterThan(30);
    expect(l.fill).toMatch(/^M10 31L10 /);
    expect(l.fill).toMatch(/L190 31Z$/);
  });
});

describe("blobPath", () => {
  it("stays within the rim's waves around the centre", () => {
    const pts = [...blobPath(50, 50, 30, 2, 7, 0.4).matchAll(/[ML](-?[\d.]+) (-?[\d.]+)/g)].map((m) => Math.hypot(Number(m[1]) - 50, Number(m[2]) - 50));
    expect(pts).toHaveLength(96);
    expect(Math.min(...pts)).toBeGreaterThan(27.9);
    expect(Math.max(...pts)).toBeLessThan(32.1);
    expect(blobPath(0, 0, 0, 2, 7, 0)).toBe("");
  });
});
