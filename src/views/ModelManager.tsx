import { Check, Download, Gauge, Loader2, Trash2, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, IconButton, Progress } from "../components/ui";
import { PageTitle } from "../components/PageTitle";
import { commands } from "../ipc/commands";
import { useModels } from "../hooks/useModels";
import type { BenchmarkResult, DownloadPayload, ModelEntry } from "../ipc/types";
import { reportError } from "../lib/actions";
import { bytes } from "../lib/format";
import { isBusy, useRecording } from "../store/recording";
import { useSettings } from "../store/settings";
import { useUi } from "../store/ui";

const tierKey = { comfortable: "tierComfortable", ok: "tierOk", heavy: "tierHeavy" } as const;

export function ModelRow({ m, p, onChanged, compact }: { m: ModelEntry; p?: DownloadPayload; onChanged: () => void; compact?: boolean }) {
  const { t, i18n } = useTranslation();
  const busy = isBusy(useRecording((s) => s.state));
  const [bench, setBench] = useState<BenchmarkResult | null>(null);
  const [benching, setBenching] = useState(false);
  const downloading = m.downloading || p?.phase === "downloading" || p?.phase === "verifying";
  const name = i18n.language === "ja" ? m.nameJa : m.nameEn;
  const desc = i18n.language === "ja" ? m.descriptionJa : m.descriptionEn;
  const record = bench ?? m.benchmark;

  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
    } catch (e) {
      reportError(e);
    }
    onChanged();
  };

  const runBench = async () => {
    setBenching(true);
    try {
      setBench(await commands.runBenchmark(m.id));
    } catch (e) {
      reportError(e);
    } finally {
      setBenching(false);
      onChanged();
    }
  };

  return (
    <div className={`rounded-2xl border bg-surface p-3 transition-[border-color,box-shadow] duration-200 ${m.selected && m.installed ? "border-fg" : "border-line"}`}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-semibold">{name}</span>
            {m.recommended && <span className="rounded-[5px] border border-line px-1.5 text-[10.5px] font-bold text-muted">{t("recommended")}</span>}
            {m.selected && m.installed && <span className="rounded-[5px] bg-fg px-1.5 text-[10.5px] font-bold text-bg">{t("selected")}</span>}
          </div>
          <div className="mt-0.5 text-xs leading-relaxed text-muted">
            {desc} · {bytes(m.sizeBytes)} · {m.license}
          </div>
          {record && (
            <div className={`mt-0.5 text-xs ${record.tier === "heavy" ? "text-warn" : "text-ok"}`}>
              {t("benchmarkResult", { seconds: record.seconds.toFixed(1), tier: t(tierKey[record.tier]) })}
              {record.tier === "heavy" && ` — ${t("tierHeavyHint")}`}
            </div>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {m.installed ? (
            <>
              {!m.selected && (
                <Button variant="primary" onClick={() => act(() => commands.modelSelect(m.id))} disabled={busy}>
                  <Check size={14} />
                  {t("use")}
                </Button>
              )}
              {!compact && (
                <IconButton label={t("benchmark")} onClick={runBench} disabled={busy || benching}>
                  {benching ? <Loader2 size={14} className="animate-spin" /> : <Gauge size={14} />}
                </IconButton>
              )}
              {!compact && (
                <IconButton label={t("delete")} onClick={() => act(() => commands.modelDelete(m.id))} disabled={busy && m.selected}>
                  <Trash2 size={14} />
                </IconButton>
              )}
            </>
          ) : downloading ? (
            <IconButton label={t("cancel")} onClick={() => act(() => commands.modelCancel(m.id))}>
              <X size={14} />
            </IconButton>
          ) : (
            <Button onClick={() => act(() => commands.modelDownload(m.id))}>
              <Download size={14} />
              {m.partialBytes > 0 ? t("resumeDownload") : t("download")}
            </Button>
          )}
        </div>
      </div>
      {downloading && p && (
        <div className="mt-2 space-y-0.5">
          <Progress value={p.received} max={p.total} />
          <div className="flex justify-between text-[11px] text-muted tabular-nums">
            <span>
              {bytes(p.received)} / {bytes(p.total)}
            </span>
            {p.bytesPerSec > 0 && <span>{t("downloadSpeed", { speed: bytes(p.bytesPerSec) })}</span>}
          </div>
        </div>
      )}
    </div>
  );
}

export function ModelManager({ standalone }: { standalone?: boolean }) {
  const { t } = useTranslation();
  const info = useSettings((s) => s.info);
  const { models, progress, refresh } = useModels();
  const visible = models.filter((m) => !m.hidden || m.installed);

  const importFile = async (id: string) => {
    const path = await commands.pickModelFile();
    if (!path) return;
    try {
      await commands.modelImport(id, path);
      useUi.getState().toast(t("saved"));
    } catch (e) {
      reportError(e);
    }
    void refresh();
  };

  const content = (
    <div className="animate-fade-up space-y-2 px-3 pt-1 pb-4">
      {visible.map((m) => (
        <ModelRow key={m.id} m={m} p={progress[m.id]} onChanged={refresh} />
      ))}
      <details className="px-1 pt-1 text-xs text-muted">
        <summary className="cursor-pointer transition-colors hover:text-fg">{t("importFromFile")}</summary>
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          {visible
            .filter((m) => !m.installed)
            .map((m) => (
              <Button key={m.id} size="sm" onClick={() => importFile(m.id)}>
                {m.file}
              </Button>
            ))}
        </div>
      </details>
      {info?.gpuFailed && (
        <Button size="sm" onClick={() => commands.retryGpu().then(() => useUi.getState().toast(t("loading")))}>
          {t("retryGpu")}
        </Button>
      )}
    </div>
  );

  if (!standalone) return content;
  return (
    <div className="flex h-full flex-col">
      <PageTitle title={t("modelsTitle")} />
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-line pt-2">{content}</div>
    </div>
  );
}
