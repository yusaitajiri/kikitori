import { useCallback, useEffect, useState } from "react";
import { commands } from "../ipc/commands";
import { on } from "../ipc/events";
import type { DownloadPayload, ModelEntry } from "../ipc/types";
import { reportError } from "../lib/actions";

/** The model list plus live download progress. */
export function useModels() {
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadPayload>>({});
  const refresh = useCallback(() => commands.modelsList().then(setModels), []);

  useEffect(() => {
    let alive = true;
    commands.modelsList().then((m) => alive && setModels(m));
    const un = on("model://download", (p) => {
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      if (p.phase !== "downloading" && p.phase !== "verifying") void commands.modelsList().then((m) => alive && setModels(m));
      if (p.phase === "error" && p.error) reportError(p.error);
    });
    return () => {
      alive = false;
      void un.then((f) => f());
    };
  }, []);
  return { models, progress, refresh };
}
