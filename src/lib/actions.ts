// User actions shared by buttons, menus and dialogs, with error reporting.

import { commands, isAppError } from "../ipc/commands";
import type { Notice } from "../ipc/types";
import i18n from "../i18n";
import { useRecording } from "../store/recording";
import { useSource } from "../store/source";
import { useTranscript } from "../store/transcript";
import { useUi } from "../store/ui";

const t = (k: string, p?: Record<string, unknown>) => i18n.t(k, p ?? {});

/** Shows a failed command as a banner (with an action when one helps) or a toast. */
export function reportError(e: unknown, fallbackKey = "failed") {
  const ui = useUi.getState();
  if (!isAppError(e)) {
    ui.toast(t(fallbackKey, { detail: String(e) }), "error");
    return;
  }
  switch (e.code) {
    case "E_MIC_DENIED":
      ui.pushBanner({ level: "error", code: e.code, message: "micDenied", action: { label: "openSettings", command: "open_mic_privacy" } });
      break;
    case "E_MODEL_MISSING":
      ui.pushBanner({ level: "error", code: e.code, message: "modelMissing", action: { label: "openModels", command: "open_models" } });
      break;
    case "E_NETWORK":
      ui.toast(t("networkError"), "error");
      break;
    case "E_MODEL_CHECKSUM":
      ui.toast(t("checksumError"), "error");
      break;
    case "E_DISK_FULL":
      ui.pushBanner({ level: "error", code: e.code, message: "diskFull" });
      break;
    case "E_APP_LOOPBACK_UNSUPPORTED":
      ui.toast(t("win11Required"), "error");
      break;
    default:
      ui.toast(t(fallbackKey, { detail: e.message }), "error");
  }
}

/** Runs the action a notice offers. */
export async function runNoticeAction(command: string) {
  const ui = useUi.getState();
  try {
    switch (command) {
      case "open_mic_privacy":
        await commands.openMicPrivacy();
        break;
      case "switch_to_system":
        await commands.switchToSystem();
        break;
      case "open_models":
        ui.go("settings", { tab: "models" });
        break;
      case "open_hotkey_settings":
        ui.go("settings", { tab: "hotkeys" });
        break;
      case "install_update":
        await installUpdate();
        break;
    }
  } catch (e) {
    reportError(e);
  }
}

/** Downloads and runs the update; the installer closes the app and starts it again. */
async function installUpdate() {
  const ui = useUi.getState();
  ui.toast(t("updateDownloading"));
  try {
    await commands.installUpdate();
  } catch (e) {
    if (isAppError(e) && e.message === "updateWhileRecording") ui.toast(t("updateWhileRecording"), "warn");
    else if (isAppError(e) && e.code === "E_NETWORK") ui.toast(t("networkError"), "error");
    else ui.toast(t("updateFailed", { detail: isAppError(e) ? e.message : String(e) }), "error");
  }
}

export function showNotice(n: Notice) {
  const ui = useUi.getState();
  const text = t(n.message, n.params);
  if (n.code === "shot_added") {
    ui.flashBorder();
    ui.toast(n.params?.fallback ? `${text}（${t("fallbackCursorMonitor")}）` : text);
    return;
  }
  if (n.toast) ui.toast(text, n.level);
  else ui.pushBanner(n);
}

export async function toggleRecording() {
  const state = useRecording.getState().state;
  try {
    if (state === "recording" || state === "paused") {
      await commands.stopRecording();
    } else if (state === "ready") {
      const cfg = useSource.getState().config();
      if (cfg.mode === "app" && !cfg.app) {
        useUi.getState().toast(t("chooseApp"), "warn");
        return;
      }
      await commands.startRecording(cfg);
    }
  } catch (e) {
    reportError(e, state === "ready" ? "startFailed" : undefined);
  }
}

export async function takeScreenshot() {
  try {
    await commands.takeScreenshot();
  } catch (e) {
    // "not recording" and debounced repeats are reported by the backend as notices.
    if (isAppError(e) && e.code !== "E_INTERNAL") reportError(e);
  }
}

/** The session the copy/export actions act on: the live one or the last one shown. */
export function currentSessionId(): string | undefined {
  return useRecording.getState().sessionId ?? useTranscript.getState().sessionId;
}

/** Clears the finished session from the main view; its line flattens back into the start dot. */
export function newRecording() {
  if (useRecording.getState().state !== "ready") return;
  useTranscript.getState().clear();
  useUi.getState().go("main");
}

export async function copy(format: "plain" | "agent" | "markdown", id = currentSessionId()) {
  if (!id) return;
  try {
    await commands.copyTranscript(id, format);
    useUi.getState().toast(t("copied"));
  } catch (e) {
    reportError(e);
  }
}

export type ExportKind = "markdown" | "pdf" | "typst";

/** Runs an export; `zip` is for Markdown (a ZIP instead of a folder). */
export async function exportAs(kind: ExportKind, id = currentSessionId(), zip = false) {
  if (!id) return;
  try {
    // Saved where the user chose (the PDF also opens); a cancelled dialog says nothing.
    const r = kind === "markdown" ? await commands.exportMarkdown(id, zip) : kind === "pdf" ? await commands.exportPdf(id) : await commands.exportTypst(id);
    if (r) useUi.getState().toast(t("savedAs", { name: r.path.split(/[\\/]/).pop() ?? "" }));
  } catch (e) {
    if (isAppError(e) && e.code === "E_PDF_FAILED") return; // the backend already opened the HTML fallback
    reportError(e, "exportFailed");
  }
}

export async function openFolder(folder = useRecording.getState().folder ?? useTranscript.getState().folder ?? useRecording.getState().lastSavedFolder) {
  if (folder) await commands.openPath(folder).catch((e) => reportError(e));
}

/** Starts a new part of the session (FR-06); the transcript shows it as a cut. */
export async function addCut() {
  try {
    await commands.addCut();
    useUi.getState().toast(t("cutAdded"));
  } catch (e) {
    reportError(e);
  }
}

export async function pauseOrResume() {
  const state = useRecording.getState().state;
  try {
    if (state === "recording") await commands.pauseRecording();
    else if (state === "paused") await commands.resumeRecording();
  } catch (e) {
    reportError(e);
  }
}
