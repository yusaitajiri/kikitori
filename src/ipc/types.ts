// Mirrors the Rust IPC types (section 14). Field names are camelCase on both sides.

export type SourceId = "mic" | "app" | "system";
export type SourceMode = "mic" | "system" | "app";
export type UiState = "needs_model" | "ready" | "recording" | "paused" | "finishing";
export type Language = "ja" | "en" | "auto";
export type Locale = "ja" | "en";
export type Layout = "compact" | "expanded";
export type LabelMode = "auto" | "on" | "off";
export type ScreenshotTarget = "cursorMonitor" | "allMonitors" | "appWindow";

export type ErrorCode =
  | "E_MIC_DENIED"
  | "E_APP_LOOPBACK_UNSUPPORTED"
  | "E_SOURCE_LOST"
  | "E_SOURCE_SILENT"
  | "E_MODEL_MISSING"
  | "E_MODEL_CHECKSUM"
  | "E_GPU_FALLBACK"
  | "E_DISK_FULL"
  | "E_HOTKEY_TAKEN"
  | "E_PDF_FAILED"
  | "E_NETWORK"
  | "E_INTERNAL";

export type AppError = { code: ErrorCode; message: string };

export type AppRef = { exe: string; rootPid: number; name?: string };

export type SourceConfig = {
  mode: SourceMode;
  app?: AppRef;
  includeMic: boolean;
  micDeviceId?: string;
};

export type AudioApp = {
  rootPid: number;
  exe: string;
  name: string;
  iconDataUrl?: string;
  active: boolean;
  hasSession: boolean;
};

export type Device = { id: string; name: string; isDefault: boolean };

export type Settings = {
  locale: Locale;
  window: { alwaysOnTop: boolean; layout: Layout };
  updates: { check: boolean };
  source: {
    mode: SourceMode;
    includeMic: boolean;
    micDeviceId: string | null;
    app: { exe: string; name: string } | null;
  };
  echoGuard: boolean;
  vad: { startThreshold: number; hangoverMs: number };
  language: Language;
  modelId: string;
  useGpu: boolean;
  accuracyFirst: boolean;
  partials: boolean;
  vocabulary: string[];
  hallucinationFilter: boolean;
  audioCtxExperimental: boolean;
  screenshot: { target: ScreenshotTarget; excludeSelf: boolean; sound: boolean };
  hotkeys: { toggle: string; screenshot: string };
  output: { root: string; titleTemplate: string };
  export: { timestamps: boolean; labels: LabelMode; mergeParagraphs: boolean };
  copy: { screenshotMarkers: boolean };
  autoCopyOnStop: boolean;
};

export type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] };

export type EngineState = "missing" | "loading" | "ready" | "failed";

export type EngineStatus = {
  state: EngineState;
  gpu: boolean;
  deviceName: string | null;
  modelId: string | null;
  error: string | null;
};

/** What is being recorded: the app or the whole system first, then the mic; named by the UI. */
export type RecordedSources = { ids: SourceId[]; appName?: string };

export type StatePayload = {
  state: UiState;
  sessionId?: string;
  elapsedMs?: number;
  sources?: RecordedSources;
  folder?: string;
};

export type AppInfo = {
  version: string;
  osBuild: number;
  appLoopbackSupported: boolean;
  device: "gpu" | "cpu" | "loading";
  deviceName: string | null;
  engine: EngineStatus;
  gpuCompiled: boolean;
  hotkeyErrors: Record<string, string>;
  state: StatePayload;
  modelsDir: string;
  logDir: string | null;
  recommendedModel: string;
  totalRamBytes: number;
  setupDone: boolean;
  gpuFailed: boolean;
};

export type Segment = {
  id: string;
  source: SourceId;
  tStartMs: number;
  tEndMs: number;
  text: string;
  textOriginal?: string;
  edited: boolean;
  utteranceId?: string;
};

export type Screenshot = {
  id: string;
  tMs: number;
  file: string;
  width: number;
  height: number;
  caption?: string;
};

export type MarkerType = "paused" | "resumed" | "source_reattached" | "unprocessed" | "cut";

export type Marker = { id: string; tMs: number; type: MarkerType; detail?: string };

export type TimelineItem =
  | ({ kind: "segment" } & Segment)
  | ({ kind: "screenshot" } & Screenshot)
  | ({ kind: "marker" } & Marker);

export type SourceInfo = { id: SourceId; label: string; exe?: string; device?: string; name?: string };

export type Session = {
  v: 1;
  id: string;
  title: string;
  startedAt: string;
  endedAt?: string;
  durationMs: number;
  sources: SourceInfo[];
  model: { id: string; sha256: string };
  language: Language;
  gpu: boolean;
  items: TimelineItem[];
  unprocessedMs?: number;
};

/**
 * How each side sounded, in equal slices of a session (0..100): from the levels the line was drawn
 * from while recording (`levels.bin`), so the session's line is its real sound.
 */
export type Sound = { others: number[]; me: number[] };

/** A session to show; `sound` is missing when the session has no `levels.bin`. */
export type SessionView = Session & { folder: string; sound?: Sound };

export type SessionSummary = {
  id: string;
  title: string;
  startedAt: string;
  durationMs: number;
  folder: string;
  segments: number;
  screenshots: number;
  recoverable: boolean;
  sources: SourceId[];
  /** The opening words, for the history list and its search. */
  preview: string;
  /**
   * The session's line in History, in equal slices: how loud each side was (its `Sound`), or for
   * a session without `levels.bin`, the share of each slice covered by 相手's and 自分's lines
   * (0..100); and the slices with a screenshot.
   */
  activity?: { others: number[]; me: number[]; shots: number[] };
};

export type BenchmarkRecord = { seconds: number; tier: "comfortable" | "ok" | "heavy"; gpu: boolean };

export type ModelEntry = {
  id: string;
  nameJa: string;
  nameEn: string;
  descriptionJa: string;
  descriptionEn: string;
  repo: string;
  file: string;
  revision: string;
  sizeBytes: number;
  sha256: string;
  license: string;
  speedTier: string;
  hidden: boolean;
  installed: boolean;
  selected: boolean;
  recommended: boolean;
  partialBytes: number;
  downloading: boolean;
  benchmark: BenchmarkRecord | null;
};

export type BenchmarkResult = { seconds: number; tier: "comfortable" | "ok" | "heavy"; gpu: boolean; text: string };

export type MicTestResult = { peakDbfs: number; text: string; device: string | null };

// Event payloads
export type LevelsPayload = Partial<Record<SourceId, number>>;
/** How loud each watched app is (dBFS, the loudest of its audio sessions). */
export type AppLevelsPayload = { levels: { rootPid: number; dbfs: number }[] };
/** `tStartMs` is the utterance start, used to place the provisional text on the timeline. */
export type PartialPayload = { source: SourceId; utteranceId: string; text: string; tStartMs?: number };
export type RemovedPayload = { id: string; reason: "echo" | "user" };
export type ScreenshotPayload = { id: string; tMs: number; thumbDataUrl: string; width: number; height: number; file: string };
export type MarkerPayload = { kind: "marker"; id: string; tMs: number; type: MarkerType; detail?: string };
export type LagPayload = { lagMs: number; queued: number; device: "gpu" | "cpu" | "loading" };
export type ProgressPayload = { done: number; total: number };
export type DownloadPayload = {
  id: string;
  received: number;
  total: number;
  bytesPerSec: number;
  phase: "downloading" | "verifying" | "done" | "error" | "cancelled";
  error?: AppError;
};
export type SavedPayload = { sessionId: string; folder: string; transcriptPath: string };
export type NoticeLevel = "info" | "warn" | "error";
export type Notice = {
  level: NoticeLevel;
  code: string;
  message: string;
  params?: Record<string, string | number | boolean>;
  action?: { label: string; command: string };
  toast?: boolean;
};
export type UiCommandPayload = { command: string };
