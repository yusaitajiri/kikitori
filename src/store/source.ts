import { create } from "zustand";
import { commands } from "../ipc/commands";
import type { AudioApp, Device, Settings, SourceConfig, SourceMode } from "../ipc/types";

type SourceStore = {
  mode: SourceMode;
  /** The chosen app: a running one from the list, or the last-used one by executable. */
  app: { exe: string; name: string; rootPid: number; iconDataUrl?: string } | null;
  includeMic: boolean;
  micDeviceId: string | null;
  apps: AudioApp[];
  devices: Device[];
  loadingApps: boolean;
  initFrom: (s: Settings) => void;
  setMode: (m: SourceMode) => void;
  setApp: (a: AudioApp) => void;
  setIncludeMic: (v: boolean) => void;
  setMicDevice: (id: string | null) => void;
  refreshApps: () => Promise<void>;
  refreshDevices: () => Promise<void>;
  config: () => SourceConfig;
};

export const useSource = create<SourceStore>((set, get) => ({
  mode: "system",
  app: null,
  includeMic: true,
  micDeviceId: null,
  apps: [],
  devices: [],
  loadingApps: false,
  initFrom: (s) =>
    set({
      mode: s.source.mode,
      includeMic: s.source.includeMic,
      micDeviceId: s.source.micDeviceId,
      app: s.source.app ? { exe: s.source.app.exe, name: s.source.app.name, rootPid: 0 } : null,
    }),
  setMode: (mode) => set({ mode }),
  setApp: (a) => set({ app: { exe: a.exe, name: a.name, rootPid: a.rootPid, iconDataUrl: a.iconDataUrl } }),
  setIncludeMic: (includeMic) => set({ includeMic }),
  setMicDevice: (micDeviceId) => set({ micDeviceId }),
  refreshApps: async () => {
    set({ loadingApps: true });
    try {
      const apps = await commands.listAudioApps();
      const current = get().app;
      // Re-bind the chosen app to its current process.
      const match = current ? apps.find((a) => a.exe.toLowerCase() === current.exe.toLowerCase()) : undefined;
      set({
        apps,
        app: match ? { exe: match.exe, name: match.name, rootPid: match.rootPid, iconDataUrl: match.iconDataUrl } : current,
      });
    } finally {
      set({ loadingApps: false });
    }
  },
  refreshDevices: async () => {
    const devices = await commands.listMicDevices();
    set({ devices });
  },
  config: () => {
    const s = get();
    return {
      mode: s.mode,
      app: s.mode === "app" && s.app ? { exe: s.app.exe, rootPid: s.app.rootPid, name: s.app.name } : undefined,
      includeMic: s.mode === "mic" ? false : s.includeMic,
      micDeviceId: s.micDeviceId ?? undefined,
    };
  },
}));
