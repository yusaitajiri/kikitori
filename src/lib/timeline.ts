// Mirror of Rust `order_timeline` (section 9): strictly by time, so the timestamps shown never
// go backwards. A segment sorts at its start, a screenshot or marker at its own time.

import type { MarkerType, SourceId, TimelineItem } from "../ipc/types";

/** Provisional text of an utterance that is still being transcribed. */
export type PartialItem = { kind: "partial"; id: string; source: SourceId; text: string; tStartMs?: number };

type Orderable = TimelineItem | PartialItem;

const tieRank = (s: SourceId) => (s === "mic" ? 1 : 0); // 相手 before 自分
// On a tie a resume or reattach marker comes before the lines it precedes; pause and
// unprocessed markers come after the lines they follow.
const markerRank = (t: MarkerType) => (t === "resumed" || t === "source_reattached" ? 0 : 3);

function key(i: Orderable): [time: number, kind: number, source: number] {
  switch (i.kind) {
    case "segment":
      return [i.tStartMs, 1, tieRank(i.source)];
    case "partial":
      // Without a start time, provisional text stays at the end.
      return [i.tStartMs ?? Number.MAX_SAFE_INTEGER, 1, tieRank(i.source)];
    case "screenshot":
      return [i.tMs, 2, 0];
    case "marker":
      return [i.tMs, markerRank(i.type), 0];
  }
}

export function compareTimeline(a: Orderable, b: Orderable): number {
  const [ta, ka, sa] = key(a);
  const [tb, kb, sb] = key(b);
  return ta - tb || ka - kb || sa - sb || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

export function orderTimeline<T extends Orderable>(items: readonly T[]): T[] {
  return [...items].sort(compareTimeline);
}
