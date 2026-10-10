import { create } from "zustand";
import type { EngineStatus, LagPayload, ProgressPayload, RecordedSources, StatePayload, UiState, WindowInfo } from "../ipc/types";

type RecordingStore = {
  state: UiState;
  sessionId?: string;
  sources?: RecordedSources;
  folder?: string;
  /** The window screenshots take, when one was picked for this recording (FR-34). */
  shotWindow?: WindowInfo;
  /** Transcription waits for Stop (FR-25). */
  deferred: boolean;
  /** Elapsed time at `elapsedAt` (ms since epoch); the UI ticks locally between 1 Hz updates. */
  elapsedMs: number;
  elapsedAt: number;
  lag: LagPayload | null;
  finishing: ProgressPayload | null;
  engine: EngineStatus | null;
  lastSavedFolder?: string;
  applyState: (p: StatePayload) => void;
  setLag: (l: LagPayload) => void;
  setFinishing: (p: ProgressPayload | null) => void;
  setEngine: (e: EngineStatus) => void;
  setSaved: (folder: string) => void;
};

export const useRecording = create<RecordingStore>((set) => ({
  state: "ready",
  elapsedMs: 0,
  elapsedAt: Date.now(),
  deferred: false,
  lag: null,
  finishing: null,
  engine: null,
  applyState: (p) =>
    set((s) => ({
      state: p.state,
      sessionId: p.sessionId ?? (p.state === "finishing" ? s.sessionId : undefined),
      sources: p.sources ?? (p.state === "finishing" ? s.sources : undefined),
      folder: p.folder ?? s.folder,
      shotWindow: p.shotWindow,
      deferred: !!p.deferred,
      elapsedMs: p.elapsedMs ?? (p.state === "recording" || p.state === "paused" || p.state === "finishing" ? s.elapsedMs : 0),
      elapsedAt: Date.now(),
      lag: p.state === "recording" || p.state === "paused" ? s.lag : null,
      finishing: p.state === "finishing" ? s.finishing : null,
    })),
  setLag: (lag) => set({ lag }),
  setFinishing: (finishing) => set({ finishing }),
  setEngine: (engine) => set({ engine }),
  setSaved: (folder) => set({ lastSavedFolder: folder }),
}));

export const isBusy = (s: UiState) => s === "recording" || s === "paused" || s === "finishing";
