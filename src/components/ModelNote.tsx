import { useTranslation } from "react-i18next";
import { useModelName } from "../hooks/useModelName";
import { isBusy, useRecording } from "../store/recording";
import { useSettings } from "../store/settings";
import { useUi } from "../store/ui";
import { Spinner } from "./ui";

/** Which model transcribes and on what, as a quiet line of text (`標準 · GPU`). Opens the model settings when idle. */
export function ModelNote({ className = "" }: { className?: string }) {
  const { t } = useTranslation();
  const engine = useRecording((s) => s.engine);
  const busy = isBusy(useRecording((s) => s.state));
  const selected = useSettings((s) => s.settings?.modelId);
  const go = useUi((s) => s.go);
  const id = engine?.state === "ready" && engine.modelId ? engine.modelId : selected;
  const name = useModelName(id);
  if (!name) return null;

  const loading = engine?.state === "loading";
  const failed = engine?.state === "failed";
  const device = engine?.state === "ready" ? (engine.gpu ? t("gpu") : t("cpu")) : null;
  const tooltip = [name.full, engine?.gpu && engine.deviceName ? engine.deviceName : device, loading ? t("modelLoading") : null, busy ? null : t("changeModel")]
    .filter(Boolean)
    .join("\n");
  const body = (
    <>
      <span className="font-semibold text-fg">{name.short}</span>
      {device && <span>· {device}</span>}
      {loading && <Spinner size={11} />}
      {failed && <span className="font-bold text-fg">· {t("modelFailed")}</span>}
    </>
  );
  const base = `inline-flex min-w-0 items-center gap-1 text-[11px] whitespace-nowrap text-muted ${className}`;
  if (busy) {
    return (
      <span className={base} title={tooltip}>
        {body}
      </span>
    );
  }
  return (
    <button
      type="button"
      title={tooltip}
      aria-label={`${t("changeModel")}: ${name.full}`}
      onClick={() => go("settings", { tab: "models" })}
      className={`${base} rounded-md transition-colors hover:text-fg`}
    >
      {body}
    </button>
  );
}
