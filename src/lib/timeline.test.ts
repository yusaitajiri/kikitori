import { describe, expect, it } from "vitest";
import type { MarkerType, SourceId, TimelineItem } from "../ipc/types";
import { orderTimeline, type PartialItem } from "./timeline";

const seg = (id: string, source: SourceId, s: number, e: number): TimelineItem => ({
  kind: "segment",
  id,
  source,
  tStartMs: s,
  tEndMs: e,
  text: id,
  edited: false,
});
const shot = (id: string, t: number): TimelineItem => ({ kind: "screenshot", id, tMs: t, file: `${id}.png`, width: 1, height: 1 });
const marker = (id: string, t: number, type: MarkerType): TimelineItem => ({ kind: "marker", id, tMs: t, type });
const partial = (source: SourceId, tStartMs?: number): PartialItem => ({ kind: "partial", id: `~partial-${source}`, source, text: "…", tStartMs });
const ids = (items: (TimelineItem | PartialItem)[]) => orderTimeline(items).map((i) => i.id);

// The same cases as `session::model::tests` in Rust.
describe("orderTimeline", () => {
  it("keeps a screenshot outside sentences at its time", () => {
    expect(ids([seg("seg_000001", "app", 1000, 2000), shot("img_0001", 2500), seg("seg_000002", "app", 3000, 4000)])).toEqual([
      "seg_000001",
      "img_0001",
      "seg_000002",
    ]);
  });

  it("sorts a screenshot inside a sentence at its own time (spec example)", () => {
    expect(ids([shot("img_0001", 3000), seg("seg_000002", "mic", 5000, 7000), seg("seg_000001", "app", 1000, 6000)])).toEqual([
      "seg_000001",
      "img_0001",
      "seg_000002",
    ]);
  });

  it("never puts a screenshot after a sentence that started later", () => {
    expect(
      ids([seg("seg_000001", "app", 1000, 4000), seg("seg_000002", "app", 4000, 9000), seg("seg_000003", "mic", 2000, 9500), shot("img_0001", 3000)]),
    ).toEqual(["seg_000001", "seg_000003", "img_0001", "seg_000002"]);
  });

  it("keeps capture order for several screenshots", () => {
    expect(
      ids([shot("img_0002", 4000), seg("seg_000001", "app", 1000, 6000), shot("img_0001", 2000), shot("img_0003", 5000), shot("img_0004", 5000)]),
    ).toEqual(["seg_000001", "img_0001", "img_0002", "img_0003", "img_0004"]);
  });

  it("reorders when a late segment arrives", () => {
    const items = [shot("img_0001", 3000)];
    expect(ids(items)).toEqual(["img_0001"]);
    items.push(seg("seg_000001", "app", 1000, 5000));
    expect(ids(items)).toEqual(["seg_000001", "img_0001"]);
  });

  it("breaks ties: segments first, others before me, then id", () => {
    expect(
      ids([shot("img_0001", 1000), seg("seg_000003", "mic", 1000, 1000), seg("seg_000002", "app", 1000, 1000), seg("seg_000001", "app", 1000, 1000)]),
    ).toEqual(["seg_000001", "seg_000002", "seg_000003", "img_0001"]);
  });

  it("puts markers on a tie around the lines they describe", () => {
    expect(
      ids([
        seg("seg_000002", "app", 5000, 6000),
        marker("mk_0002", 5000, "resumed"),
        marker("mk_0001", 2000, "paused"),
        seg("seg_000001", "app", 2000, 2000),
        seg("seg_000003", "app", 8000, 9000),
        marker("mk_0003", 8000, "cut"),
      ]),
    ).toEqual(["seg_000001", "mk_0001", "mk_0002", "seg_000002", "mk_0003", "seg_000003"]);
  });

  it("places provisional text at its utterance start", () => {
    expect(ids([seg("seg_000001", "app", 1000, 2000), shot("img_0001", 4000), partial("app", 3000)])).toEqual([
      "seg_000001",
      "~partial-app",
      "img_0001",
    ]);
  });

  it("keeps provisional text without a start time at the end", () => {
    expect(ids([partial("mic"), seg("seg_000001", "app", 1000, 2000), shot("img_0001", 4000)])).toEqual(["seg_000001", "img_0001", "~partial-mic"]);
  });
});
