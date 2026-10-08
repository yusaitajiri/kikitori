// The source as a native menu: the compact window's way to switch it while recording (FR-17).
// A native menu may reach past the window, which at 170 px has no room for a dropdown.

import i18n from "../i18n";
import { useSettings } from "../store/settings";
import { useSource } from "../store/source";
import { reportError } from "./actions";
import { menuText, popupMenu, type MenuEntry } from "./popupMenu";

/** Opens the menu; a pick changes the choice, which `useLiveSource` turns into a switch. */
export async function sourceMenu() {
  const t = (k: string) => i18n.t(k);
  await useSource
    .getState()
    .refreshApps()
    .catch(() => {});
  const s = useSource.getState();
  const appSupported = useSettings.getState().info?.appLoopbackSupported ?? true;
  const isApp = (exe: string, pid: number) => s.mode === "app" && (s.app?.rootPid ? s.app.rootPid === pid : s.app?.exe.toLowerCase() === exe.toLowerCase());
  const apps = [...s.apps.filter((a) => a.hasSession), ...s.apps.filter((a) => !a.hasSession)];
  const entries: MenuEntry[] = [
    { text: t("srcSystem"), checked: s.mode === "system", action: () => s.setMode("system") },
    { text: t("srcMicOnly"), checked: s.mode === "mic", action: () => s.setMode("mic") },
    "separator",
    ...apps.map((a) => ({
      text: menuText(a.name),
      checked: isApp(a.exe, a.rootPid),
      enabled: appSupported,
      action: () => {
        s.setApp(a);
        s.setMode("app");
      },
    })),
    ...(apps.length ? (["separator"] as MenuEntry[]) : []),
    { text: t("conversation"), checked: s.mode !== "mic" && s.includeMic, enabled: s.mode !== "mic", action: () => s.setIncludeMic(!s.includeMic) },
  ];
  await popupMenu(entries).catch(reportError);
}
