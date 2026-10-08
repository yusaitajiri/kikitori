import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSource } from "../store/source";
import { SourcePanel, SourcePicker, SourceSummary } from "./SourcePicker";

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
        <SourceSummary className="w-full" open={picking} onOpen={() => setPicking(true)} />
        {/* Over the whole window under the title bar, the start row included. */}
        {picking && <SourcePanel dense onClose={() => setPicking(false)} />}
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
