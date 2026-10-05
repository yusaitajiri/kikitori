import { create } from "zustand";
import type { Notice } from "../ipc/types";

export type View = "main" | "settings" | "history" | "models" | "session";
/** A section of 設定, or `index`: the list of sections. */
export type SettingsTab = "index" | "general" | "audio" | "transcription" | "models" | "screenshots" | "hotkeys" | "output";

export type Toast = { id: number; text: string; level: "info" | "warn" | "error" };
export type Banner = Notice & { id: number };

type UiStore = {
  view: View;
  settingsTab: SettingsTab;
  viewingSessionId?: string;
  /** Session the 書き出し dialog is open for. */
  exportFor?: string;
  banners: Banner[];
  toasts: Toast[];
  flash: number;
  confirmQuit: boolean;
  go: (view: View, opts?: { tab?: SettingsTab; sessionId?: string }) => void;
  pushBanner: (n: Notice) => void;
  dismissBanner: (id: number) => void;
  toast: (text: string, level?: Toast["level"]) => void;
  flashBorder: () => void;
  setConfirmQuit: (v: boolean) => void;
  setExportFor: (id?: string) => void;
};

let nextId = 1;

export const useUi = create<UiStore>((set) => ({
  view: "main",
  settingsTab: "index",
  banners: [],
  toasts: [],
  flash: 0,
  confirmQuit: false,
  go: (view, opts) =>
    set((s) => ({
      view,
      // 設定 opens at its list unless a section is asked for.
      settingsTab: opts?.tab ?? (view === "settings" ? "index" : s.settingsTab),
      viewingSessionId: opts?.sessionId ?? s.viewingSessionId,
    })),
  pushBanner: (n) =>
    set((s) => ({
      // One banner per code; the newest replaces an older one.
      banners: [...s.banners.filter((b) => b.code !== n.code), { ...n, id: nextId++ }].slice(-3),
    })),
  dismissBanner: (id) => set((s) => ({ banners: s.banners.filter((b) => b.id !== id) })),
  toast: (text, level = "info") => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts, { id, text, level }].slice(-3) }));
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), 2000);
  },
  flashBorder: () => set((s) => ({ flash: s.flash + 1 })),
  setConfirmQuit: (confirmQuit) => set({ confirmQuit }),
  setExportFor: (exportFor) => set({ exportFor }),
}));
