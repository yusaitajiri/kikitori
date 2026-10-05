import { useEffect, useState } from "react";
import { useRecording } from "../store/recording";

/** Recording time without pauses, ticking locally between the backend's 1 Hz updates. */
export function useElapsed(): number {
  const state = useRecording((s) => s.state);
  const elapsedMs = useRecording((s) => s.elapsedMs);
  const elapsedAt = useRecording((s) => s.elapsedAt);
  const [now, setNow] = useState(0);
  useEffect(() => {
    if (state !== "recording") return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [state]);
  return state === "recording" ? elapsedMs + Math.max(0, now - elapsedAt) : elapsedMs;
}
