// Typed wrappers for the Rust commands (section 14).

import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  AppInfo,
  AudioApp,
  BenchmarkResult,
  DeepPartial,
  Device,
  Layout,
  MicTestResult,
  ModelEntry,
  Segment,
  SessionSummary,
  SessionView,
  Settings,
  SourceConfig,
} from "./types";

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export const commands = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (partial: DeepPartial<Settings>) => invoke<Settings>("set_settings", { partial }),
  listAudioApps: () => invoke<AudioApp[]>("list_audio_apps"),
  listMicDevices: () => invoke<Device[]>("list_mic_devices"),
  watchAppLevels: (rootPids: number[]) => invoke<void>("watch_app_levels", { rootPids }),
  startRecording: (source: SourceConfig, title?: string) =>
    invoke<{ sessionId: string; folder: string }>("start_recording", { source, title }),
  stopRecording: () => invoke<void>("stop_recording"),
  cancelFinishing: () => invoke<void>("cancel_finishing"),
  pauseRecording: () => invoke<void>("pause_recording"),
  resumeRecording: () => invoke<void>("resume_recording"),
  switchToSystem: () => invoke<void>("switch_to_system"),
  takeScreenshot: () => invoke<{ id: string; tMs: number }>("take_screenshot"),
  addCut: () => invoke<void>("add_cut"),
  getSession: (sessionId: string) => invoke<SessionView>("get_session", { sessionId }),
  copyTranscript: (sessionId: string, format: "plain" | "agent" | "markdown") =>
    invoke<{ chars: number }>("copy_transcript", { sessionId, format }),
  /**
   * Each export asks where to save it; `null` when the dialog is cancelled. Markdown without
   * `zip` is a folder (Markdown and images) in a directory the user picks.
   */
  exportMarkdown: (sessionId: string, zip: boolean) => invoke<{ path: string } | null>("export_markdown", { sessionId, zip }),
  exportPdf: (sessionId: string) => invoke<{ path: string } | null>("export_pdf", { sessionId }),
  exportTypst: (sessionId: string) => invoke<{ path: string } | null>("export_typst", { sessionId }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openMicPrivacy: () => invoke<void>("open_mic_privacy"),
  openLogs: () => invoke<void>("open_logs"),
  listRecoverable: () => invoke<SessionSummary[]>("list_recoverable"),
  recoverSession: (sessionId: string) => invoke<{ path: string }>("recover_session", { sessionId }),
  listSessions: () => invoke<SessionSummary[]>("list_sessions"),
  deleteSession: (sessionId: string) => invoke<void>("delete_session", { sessionId }),
  deleteSessions: (sessionIds: string[]) => invoke<{ deleted: number }>("delete_sessions", { sessionIds }),
  renameSession: (sessionId: string, title: string) => invoke<void>("rename_session", { sessionId, title }),
  updateSegment: (sessionId: string, segmentId: string, text: string) =>
    invoke<Segment>("update_segment", { sessionId, segmentId, text }),
  deleteSegment: (sessionId: string, segmentId: string) => invoke<void>("delete_segment", { sessionId, segmentId }),
  deleteScreenshot: (sessionId: string, id: string) => invoke<void>("delete_screenshot", { sessionId, id }),
  setCaption: (sessionId: string, id: string, caption: string) =>
    invoke<void>("set_caption", { sessionId, id, caption }),
  modelsList: () => invoke<ModelEntry[]>("models_list"),
  modelDownload: (id: string) => invoke<void>("model_download", { id }),
  modelCancel: (id: string) => invoke<void>("model_cancel", { id }),
  modelSelect: (id: string) => invoke<void>("model_select", { id }),
  modelDelete: (id: string) => invoke<void>("model_delete", { id }),
  modelImport: (id: string, path: string) => invoke<void>("model_import", { id, path }),
  runBenchmark: (modelId: string) => invoke<BenchmarkResult>("run_benchmark", { modelId }),
  retryGpu: () => invoke<void>("retry_gpu"),
  micTest: (seconds?: number) => invoke<MicTestResult>("mic_test", { seconds }),
  completeSetup: () => invoke<void>("complete_setup"),
  setWindowLayout: (layout: Layout, persist: boolean) => invoke<void>("set_window_layout", { layout, persist }),
  windowAction: (action: "minimize" | "hide" | "close" | "show") => invoke<void>("window_action", { action }),
  quitApp: () => invoke<void>("quit_app"),
  installUpdate: () => invoke<void>("install_update"),
  pickFolder: () => invoke<string | null>("pick_folder"),
  pickModelFile: () => invoke<string | null>("pick_model_file"),
};
