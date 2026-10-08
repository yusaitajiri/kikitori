import { createContext, useContext } from "react";
import { create, type StoreApi, type UseBoundStore } from "zustand";
import type {
  AudioFile,
  MarkerPayload,
  PartialPayload,
  ScreenshotPayload,
  Segment,
  SessionView,
  Sound,
  SourceId,
  SourceInfo,
  TimelineItem,
} from "../ipc/types";
import type { Continuation } from "../lib/format";

export type PartialLine = { utteranceId: string; text: string; tStartMs?: number };

export type TranscriptState = {
  sessionId?: string;
  title?: string;
  startedAt?: string;
  folder?: string;
  durationMs?: number;
  modelId?: string;
  gpu?: boolean;
  /** The project's ID (FR-64). */
  project?: string;
  /** Later recordings onto the session (FR-08), for its clock times. */
  continued: Continuation[];
  /** Each recording's sound (FR-09). */
  audio: AudioFile[];
  /** A moment to play from (ms into the session), asked for by a line; `n` counts requests. */
  play?: { ms: number; n: number };
  /** Where the sound being played is, or `null` when nothing plays. */
  playingMs: number | null;
  sources: SourceInfo[];
  /** How the finished session sounded, for its line; missing when the session has no `levels.bin`. */
  sound?: Sound;
  items: TimelineItem[];
  partials: Partial<Record<SourceId, PartialLine>>;
  /** Screenshot id → thumbnail data URL (live captures). */
  thumbs: Record<string, string>;
  /** Items that arrived live a moment ago; they animate in. Each is forgotten after 1.5 s. */
  fresh: Record<string, true>;
  /** A moment to show (ms into the session), set by pointing at the session's line; `n` counts requests. */
  seek?: { ms: number; n: number };
  load: (s: SessionView) => void;
  clear: () => void;
  addSegment: (seg: Segment) => void;
  removeItem: (id: string) => void;
  updateSegment: (seg: Segment) => void;
  addScreenshot: (p: ScreenshotPayload) => void;
  setCaption: (id: string, caption: string) => void;
  addMarker: (p: MarkerPayload) => void;
  setPartial: (p: PartialPayload) => void;
  setTitle: (title: string) => void;
  setProject: (project?: string) => void;
  playFrom: (ms: number) => void;
  setPlaying: (ms: number | null) => void;
  seekTo: (ms: number) => void;
};

export type TranscriptStore = UseBoundStore<StoreApi<TranscriptState>>;

const empty = {
  sessionId: undefined,
  title: undefined,
  startedAt: undefined,
  folder: undefined,
  durationMs: undefined,
  modelId: undefined,
  gpu: undefined,
  project: undefined,
  continued: [],
  audio: [],
  play: undefined,
  playingMs: null,
  sources: [],
  sound: undefined,
  items: [],
  partials: {},
  thumbs: {},
  fresh: {},
  seek: undefined,
};

export function createTranscriptStore(): TranscriptStore {
  return create<TranscriptState>((set) => {
    const arrived = (fresh: Record<string, true>, id: string) => {
      setTimeout(
        () =>
          set((s) => {
            if (!s.fresh[id]) return s;
            const next = { ...s.fresh };
            delete next[id];
            return { fresh: next };
          }),
        1500,
      );
      return { ...fresh, [id]: true as const };
    };
    return {
      ...empty,
      load: (s) =>
        set((prev) => ({
          sessionId: s.id,
          title: s.title,
          startedAt: s.startedAt,
          folder: s.folder,
          durationMs: s.durationMs,
          modelId: s.model.id,
          gpu: s.gpu,
          project: s.project,
          continued: s.continued ?? [],
          audio: s.audio ?? [],
          sources: s.sources,
          sound: s.sound,
          // Keep live items that may have arrived while loading.
          items: mergeItems(s.items, prev.sessionId === s.id ? prev.items : []),
          partials: prev.sessionId === s.id ? prev.partials : {},
          thumbs: prev.sessionId === s.id ? prev.thumbs : {},
          fresh: prev.sessionId === s.id ? prev.fresh : {},
        })),
      clear: () => set(empty),
      addSegment: (seg) =>
        set((s) => {
          if (s.items.some((i) => i.id === seg.id)) return s;
          const partials = { ...s.partials };
          const p = partials[seg.source];
          if (p && seg.utteranceId && p.utteranceId === seg.utteranceId) delete partials[seg.source];
          return { items: [...s.items, { kind: "segment", ...seg }], partials, fresh: arrived(s.fresh, seg.id) };
        }),
      removeItem: (id) => set((s) => ({ items: s.items.filter((i) => i.id !== id) })),
      updateSegment: (seg) =>
        set((s) => ({ items: s.items.map((i) => (i.kind === "segment" && i.id === seg.id ? { kind: "segment", ...seg } : i)) })),
      addScreenshot: (p) =>
        set((s) => {
          if (s.items.some((i) => i.id === p.id)) return s;
          return {
            items: [...s.items, { kind: "screenshot", id: p.id, tMs: p.tMs, file: p.file, width: p.width, height: p.height }],
            thumbs: p.thumbDataUrl ? { ...s.thumbs, [p.id]: p.thumbDataUrl } : s.thumbs,
            fresh: arrived(s.fresh, p.id),
          };
        }),
      setCaption: (id, caption) =>
        set((s) => ({ items: s.items.map((i) => (i.kind === "screenshot" && i.id === id ? { ...i, caption: caption || undefined } : i)) })),
      addMarker: (p) =>
        set((s) =>
          s.items.some((i) => i.id === p.id)
            ? s
            : {
                items: [...s.items, { kind: "marker", id: p.id, tMs: p.tMs, type: p.type, detail: p.detail }],
                fresh: arrived(s.fresh, p.id),
              },
        ),
      setPartial: (p) =>
        set((s) => {
          const partials = { ...s.partials };
          // A partial for an utterance that already has its final line is stale.
          const finalized = s.items.some((i) => i.kind === "segment" && i.utteranceId === p.utteranceId);
          if (!p.text || finalized) {
            if (partials[p.source]?.utteranceId === p.utteranceId) delete partials[p.source];
          } else {
            partials[p.source] = { utteranceId: p.utteranceId, text: p.text, tStartMs: p.tStartMs };
          }
          return { partials };
        }),
      setTitle: (title) => set({ title }),
      setProject: (project) => set({ project }),
      playFrom: (ms) => set((s) => ({ play: { ms, n: (s.play?.n ?? 0) + 1 } })),
      setPlaying: (playingMs) => set({ playingMs }),
      seekTo: (ms) => set((s) => ({ seek: { ms, n: (s.seek?.n ?? 0) + 1 } })),
    };
  });
}

function mergeItems(loaded: TimelineItem[], live: TimelineItem[]): TimelineItem[] {
  const ids = new Set(loaded.map((i) => i.id));
  return [...loaded, ...live.filter((i) => !ids.has(i.id))];
}

/** The live (or last recorded) session; backend events feed it. */
export const useTranscript = createTranscriptStore();

/** Which transcript the components below show: the live one unless a viewer provides its own. */
export const TranscriptContext = createContext<TranscriptStore>(useTranscript);

export function useTranscriptStore(): TranscriptStore {
  return useContext(TranscriptContext);
}
