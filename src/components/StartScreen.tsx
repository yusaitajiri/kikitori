import { Check, ChevronDown, Mic, MonitorSpeaker } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useSource } from "../store/source";
import { AppWave } from "./AppWave";
import { AppIcon, SourcePicker } from "./SourcePicker";
import { IconButton } from "./ui";

/** The compact window's one line about what it will listen to: `Zoom · 会話`. */
function SourceChip({ onOpen, open }: { onOpen: () => void; open: boolean }) {
  const { t } = useTranslation();
  const { mode, app, includeMic } = useSource();
  const two = mode !== "mic" && includeMic;
  const icon = mode === "app" ? <AppIcon src={app?.iconDataUrl} size={18} /> : mode === "system" ? <MonitorSpeaker size={16} /> : <Mic size={16} />;
  const name = mode === "app" ? (app?.name ?? t("chooseApp")) : mode === "system" ? t("srcSystem") : t("srcMicOnly");
  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={t("chooseSource")}
      aria-expanded={open}
      className="flex h-10 w-full min-w-0 items-center gap-2 rounded-[11px] border border-line bg-surface px-3 text-left transition-colors hover:border-line-strong"
    >
      <span className="grid shrink-0 place-items-center">{icon}</span>
      <span className="min-w-0 truncate text-[13px] font-semibold">{name}</span>
      {two && <span className="shrink-0 text-[12px] text-muted">· {t("conversationShort")}</span>}
      <span className="ml-auto flex shrink-0 items-center gap-2">
        {mode === "app" && <AppWave pid={app?.rootPid} />}
        <ChevronDown size={15} className="text-muted" />
      </span>
    </button>
  );
}

/**
 * Above the line before a recording: what to listen to, named as the two voices the transcript
 * will show. The line and its start dot wait in the free space below.
 */
export function StartScreen({ compact }: { compact: boolean }) {
  const { t } = useTranslation();
  const [picking, setPicking] = useState(false);
  const refreshApps = useSource((s) => s.refreshApps);

  // The chip shows the chosen app's icon, which only the app list has.
  useEffect(() => {
    if (compact) void refreshApps().catch(() => {});
  }, [compact, refreshApps]);

  if (compact) {
    return (
      <div className="flex min-h-0 flex-1 items-center px-3">
        <SourceChip open={picking} onOpen={() => setPicking(true)} />
        {/* Over the whole window under the title bar, the start row included (a portal, so the page's own layers don't cover it). */}
        {picking &&
          createPortal(
            <div className="fixed inset-x-0 top-10 bottom-0 z-30 animate-fade-in overflow-y-auto bg-bg px-3 pt-0.5 pb-2">
              <div className="mb-1 flex items-center justify-between pl-0.5">
                <span className="text-[12px] font-semibold text-muted">{t("listenTo")}</span>
                <IconButton size="sm" label={t("done")} onClick={() => setPicking(false)} className="!text-fg">
                  <Check size={16} strokeWidth={2.5} />
                </IconButton>
              </div>
              <SourcePicker dense />
            </div>,
            document.body,
          )}
      </div>
    );
  }
  return (
    <div className="px-4 pt-3 pb-2">
      <div className="mx-auto w-full max-w-[380px] space-y-2">
        <div className="px-0.5 text-[12px] font-semibold text-muted">{t("listenTo")}</div>
        <SourcePicker />
      </div>
    </div>
  );
}
