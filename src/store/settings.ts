import { create } from "zustand";
import { commands } from "../ipc/commands";
import type { AppInfo, DeepPartial, ModelEntry, Settings } from "../ipc/types";
import i18n from "../i18n";

type SettingsStore = {
  settings: Settings | null;
  info: AppInfo | null;
  /** The model catalog, for showing model names. */
  models: ModelEntry[];
  load: () => Promise<void>;
  refreshInfo: () => Promise<void>;
  refreshModels: () => Promise<void>;
  update: (partial: DeepPartial<Settings>) => Promise<Settings>;
};

export const useSettings = create<SettingsStore>((set) => ({
  settings: null,
  info: null,
  models: [],
  load: async () => {
    const [settings, info] = await Promise.all([commands.getSettings(), commands.getAppInfo()]);
    await i18n.changeLanguage(settings.locale);
    set({ settings, info });
  },
  refreshInfo: async () => {
    const info = await commands.getAppInfo();
    set({ info });
  },
  refreshModels: async () => {
    const models = await commands.modelsList();
    set({ models });
  },
  update: async (partial) => {
    const settings = await commands.setSettings(partial);
    if (settings.locale !== i18n.language) await i18n.changeLanguage(settings.locale);
    set({ settings });
    return settings;
  },
}));
