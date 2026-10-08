import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import i18n from "../i18n";
import { useTranscriptStore } from "../store/transcript";
import { useUi } from "../store/ui";

export type Player = {
  /** The session kept its sound, and it can be played here. */
  available: boolean;
  playing: boolean;
  toggle: () => void;
};

/**
 * Plays a finished session's sound (FR-09): from the line the transcript asks for (`play` in its
 * store) or from the deck's button, and tells the store where it is (`playingMs`). A session
 * continued later has one file per recording; playing runs on from one into the next.
 */
export function useSessionAudio(active: boolean): Player {
  const store = useTranscriptStore();
  const audio = store((s) => s.audio);
  const folder = store((s) => s.folder);
  const request = store((s) => s.play);
  const [playing, setPlaying] = useState(false);
  const el = useRef<HTMLAudioElement | null>(null);
  /** Which of the session's files is loaded. */
  const part = useRef(-1);
  const latest = useRef({ audio, folder });
  latest.current = { audio, folder };
  const available = active && !!folder && audio.length > 0;

  const element = () => {
    if (el.current) return el.current;
    const e = new Audio();
    e.preload = "auto";
    e.addEventListener("timeupdate", () => {
      const file = latest.current.audio[part.current];
      if (file && !e.paused) store.getState().setPlaying(file.startMs + e.currentTime * 1000);
    });
    e.addEventListener("play", () => setPlaying(true));
    e.addEventListener("pause", () => setPlaying(false));
    e.addEventListener("ended", () => {
      const next = latest.current.audio[part.current + 1];
      if (next) playFrom(next.startMs);
      else store.getState().setPlaying(null);
    });
    e.addEventListener("error", () => {
      setPlaying(false);
      store.getState().setPlaying(null);
      useUi.getState().toast(i18n.t("audioMissing"), "warn");
    });
    el.current = e;
    return e;
  };

  function playFrom(ms: number) {
    const { audio: files, folder: dir } = latest.current;
    if (!files.length || !dir) return;
    const i = Math.max(0, files.findLastIndex((f) => f.startMs <= ms));
    const file = files[i];
    const e = element();
    const start = () => {
      e.currentTime = Math.max(0, (ms - file.startMs) / 1000);
      void e.play().catch(() => {});
    };
    if (part.current !== i) {
      part.current = i;
      e.src = convertFileSrc(`${dir}\\${file.file.replaceAll("/", "\\")}`);
      e.addEventListener("loadedmetadata", start, { once: true });
    } else {
      start();
    }
    store.getState().setPlaying(ms);
  }

  // A line asked to be played.
  useEffect(() => {
    if (request && available) playFrom(request.ms);
  }, [request]); // eslint-disable-line react-hooks/exhaustive-deps

  // Leaving the session (or the review) stops it and lets go of the file.
  useEffect(() => {
    if (!available) return;
    return () => {
      const e = el.current;
      if (e) {
        e.pause();
        e.removeAttribute("src");
        e.load();
      }
      part.current = -1;
      store.getState().setPlaying(null);
    };
  }, [available, folder, store]);

  const toggle = () => {
    const e = el.current;
    if (e && part.current >= 0 && store.getState().playingMs !== null) {
      if (e.paused) void e.play().catch(() => {});
      else e.pause();
    } else {
      playFrom(0);
    }
  };
  return { available, playing, toggle };
}
