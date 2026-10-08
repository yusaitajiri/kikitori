import { useEffect } from "react";
import { commands } from "../ipc/commands";
import type { SourceConfig } from "../ipc/types";
import i18n from "../i18n";
import { reportError } from "../lib/actions";
import { useSettings } from "../store/settings";
import { useSource } from "../store/source";
import { useUi } from "../store/ui";

/** A source choice as the recorder compares it: which app, not which of its processes. */
const sourceKey = (c: SourceConfig) => JSON.stringify([c.mode, c.app?.exe.toLowerCase() ?? null, c.includeMic, c.micDeviceId ?? null]);

/**
 * While a recording runs, a change of the source choice (the header's dropdown, the compact
 * window's menu) switches what is recorded at once (FR-17); if the new source cannot start, the
 * recording keeps the old one and the choice goes back to it.
 *
 * It first takes the choice from the settings, without switching: the recording may have been
 * started by the hotkey with the last-used source, which the picker never showed.
 */
export function useLiveSource(recording: boolean, sessionId?: string) {
  useEffect(() => {
    if (!recording) return;
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const settings = await commands.getSettings();
        if (cancelled) return;
        useSettings.setState({ settings });
        useSource.getState().initFrom(settings);
        await useSource.getState().refreshApps();
      } catch {
        // The choice stays as it was.
      }
      if (cancelled) return;
      let applied = useSource.getState();
      let key = sourceKey(applied.config());
      unsubscribe = useSource.subscribe((s) => {
        const config = s.config();
        const next = sourceKey(config);
        if (next === key || (config.mode === "app" && !config.app)) return;
        const before = applied;
        applied = s;
        key = next;
        commands
          .switchSource(config)
          .then(() => useUi.getState().toast(i18n.t("sourceSwitched")))
          .catch((e) => {
            reportError(e);
            applied = before;
            key = sourceKey(before.config());
            useSource.setState({ mode: before.mode, app: before.app, includeMic: before.includeMic, micDeviceId: before.micDeviceId });
          });
      });
    })();
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, [recording, sessionId]);
}
