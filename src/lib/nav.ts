// The three places of the expanded window, named in its title bar: 録音, 履歴 and 設定.

import type { SettingsTab, View } from "../store/ui";

export const PLACES = ["main", "history", "settings"] as const;
export type Place = (typeof PLACES)[number];

/** Which place a view belongs to: a past session sits under 履歴, the model list under 設定. */
export function placeOf(view: View): Place {
  if (view === "history" || view === "session") return "history";
  if (view === "settings" || view === "models") return "settings";
  return "main";
}

/** Where the title bar's back arrow leads: a past session back to 履歴, a section of 設定 back to its list. */
export function backOf(view: View, settingsTab: SettingsTab): View | undefined {
  if (view === "session") return "history";
  if (view === "settings" && settingsTab !== "index") return "settings";
  return undefined;
}
