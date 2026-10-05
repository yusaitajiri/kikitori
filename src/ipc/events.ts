// Typed event subscriptions (section 14).

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppLevelsPayload,
  DownloadPayload,
  EngineStatus,
  LagPayload,
  LevelsPayload,
  MarkerPayload,
  Notice,
  PartialPayload,
  ProgressPayload,
  RemovedPayload,
  SavedPayload,
  ScreenshotPayload,
  Segment,
  StatePayload,
  UiCommandPayload,
} from "./types";

export type EventMap = {
  "recording://state": StatePayload;
  "audio://levels": LevelsPayload;
  "audio://app-levels": AppLevelsPayload;
  "transcript://partial": PartialPayload;
  "transcript://segment": Segment;
  "transcript://segment-removed": RemovedPayload;
  "transcript://screenshot": ScreenshotPayload;
  "transcript://marker": MarkerPayload;
  "asr://lag": LagPayload;
  "finishing://progress": ProgressPayload;
  "model://download": DownloadPayload;
  "model://status": EngineStatus;
  "session://saved": SavedPayload;
  "app://notice": Notice;
  "ui://command": UiCommandPayload;
};

export function on<K extends keyof EventMap>(name: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  return listen<EventMap[K]>(name, (e) => handler(e.payload));
}
