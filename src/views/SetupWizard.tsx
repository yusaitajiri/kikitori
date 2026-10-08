import { Keyboard, Loader2, Mic, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { MicWave } from "../components/MicWave";
import { Button, Kbd, Progress } from "../components/ui";
import { PageTitle } from "../components/PageTitle";
import { commands } from "../ipc/commands";
import { on } from "../ipc/events";
import type { BenchmarkResult, MicTestResult } from "../ipc/types";
import { reportError } from "../lib/actions";
import { bytes } from "../lib/format";
import { useSettings } from "../store/settings";
import { useModels } from "../hooks/useModels";

const STEPS = ["wizardWelcome", "wizardModel", "wizardDownload", "wizardMicTest", "wizardHotkeys", "wizardDone"] as const;
const tierKey = { comfortable: "tierComfortable", ok: "tierOk", heavy: "tierHeavy" } as const;

export function SetupWizard() {
  const { t, i18n } = useTranslation();
  const { info, settings, refreshInfo } = useSettings();
  const { models, progress, refresh } = useModels();
  const [step, setStep] = useState(0);
  const [choice, setChoice] = useState<string | null>(null);
  const [bench, setBench] = useState<BenchmarkResult | null>(null);
  const [benching, setBenching] = useState(false);
  const [mic, setMic] = useState<MicTestResult | null>(null);
  const [micLevel, setMicLevel] = useState<number | undefined>(undefined);
  const [testing, setTesting] = useState(false);

  const visible = models.filter((m) => !m.hidden || m.installed);
  const selectedId = choice ?? info?.recommendedModel ?? "turbo-q5";
  const model = models.find((m) => m.id === selectedId);
  const p = progress[selectedId];

  useEffect(() => {
    const un = on("audio://levels", (l) => l.mic !== undefined && setMicLevel(l.mic));
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // Download step: start, then select and benchmark when done.
  useEffect(() => {
    if (STEPS[step] !== "wizardDownload" || !model) return;
    if (!model.installed && !model.downloading && p?.phase !== "downloading") {
      commands.modelDownload(model.id).catch((e) => reportError(e));
    }
    if (model.installed && !model.selected) {
      void commands.modelSelect(model.id).then(refresh);
    }
    if (model.installed && model.selected && !bench && !benching) {
      setBenching(true);
      commands
        .runBenchmark(model.id)
        .then(setBench)
        .catch(() => {})
        .finally(() => setBenching(false));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, model?.installed, model?.selected, model?.downloading]);

  const runMicTest = async () => {
    setTesting(true);
    setMic(null);
    try {
      setMic(await commands.micTest(10));
    } catch (e) {
      reportError(e);
    } finally {
      setTesting(false);
    }
  };

  const finish = async () => {
    await commands.completeSetup();
    await refreshInfo();
  };

  const name = (m: (typeof models)[number]) => (i18n.language === "ja" ? m.nameJa : m.nameEn);
  const desc = (m: (typeof models)[number]) => (i18n.language === "ja" ? m.descriptionJa : m.descriptionEn);
  const canNext =
    STEPS[step] === "wizardDownload" ? !!model?.installed && !!model.selected && !benching : STEPS[step] === "wizardMicTest" ? !testing : true;

  return (
    <div className="flex h-full flex-col">
      <PageTitle title={t(STEPS[step])} sub={`${step + 1} / ${STEPS.length}`}>
        <div className="mt-3 flex shrink-0 gap-1.5" aria-hidden>
          {STEPS.map((s, i) => (
            <span key={s} className="h-1 flex-1 overflow-hidden rounded-full bg-meter">
              <span className={`block h-full rounded-full bg-accent transition-[width] duration-500 ease-out-soft ${i <= step ? "w-full" : "w-0"}`} />
            </span>
          ))}
        </div>
      </PageTitle>
      <div key={step} className="min-h-0 flex-1 overflow-y-auto border-t border-line px-[18px] py-3.5">
        {STEPS[step] === "wizardWelcome" && (
          <div className="space-y-3">
            <ShieldCheck size={28} className="text-fg" />
            <p>{t("wizardWelcomeBody")}</p>
          </div>
        )}

        {STEPS[step] === "wizardModel" && (
          <div className="space-y-2">
            {visible.map((m) => (
              <label
                key={m.id}
                className={`flex cursor-pointer gap-2.5 rounded-2xl border p-3 transition-[background-color,border-color] duration-200 ${selectedId === m.id ? "border-fg" : "border-line bg-surface hover:border-line-strong"}`}
              >
                <input type="radio" name="model" className="mt-1 accent-[var(--kk-accent)]" checked={selectedId === m.id} onChange={() => setChoice(m.id)} />
                <span className="min-w-0">
                  <span className="flex items-center gap-1.5 font-semibold">
                    {name(m)}
                    {m.recommended && <span className="rounded-[5px] bg-fg px-1.5 text-[10.5px] font-bold text-bg">{t("recommended")}</span>}
                  </span>
                  <span className="block text-xs text-muted">
                    {desc(m)} · {bytes(m.sizeBytes)}
                    {m.installed && ` · ${t("installed")}`}
                  </span>
                </span>
              </label>
            ))}
          </div>
        )}

        {STEPS[step] === "wizardDownload" && model && (
          <div className="space-y-3">
            <div className="font-semibold">
              {name(model)} · {bytes(model.sizeBytes)}
            </div>
            {!model.installed && (
              <>
                <Progress value={p?.received ?? model.partialBytes} max={model.sizeBytes} />
                <div className="flex justify-between text-xs text-muted tabular-nums">
                  <span>
                    {bytes(p?.received ?? model.partialBytes)} / {bytes(model.sizeBytes)}
                  </span>
                  {p && p.bytesPerSec > 0 && <span>{t("downloadSpeed", { speed: bytes(p.bytesPerSec) })}</span>}
                </div>
                {p?.phase === "error" && (
                  <div className="flex items-center gap-2 text-xs text-err">
                    {p.error?.code === "E_MODEL_CHECKSUM" ? t("checksumError") : t("networkError")}
                    <Button size="sm" onClick={() => commands.modelDownload(model.id).then(refresh)}>
                      {t("retry")}
                    </Button>
                  </div>
                )}
              </>
            )}
            {model.installed && (
              <div className="space-y-1 text-xs">
                <div className="text-ok">✓ {t("installed")}</div>
                {benching && (
                  <div className="flex items-center gap-1 text-muted">
                    <Loader2 size={13} className="animate-spin" /> {t("benchmark")}…
                  </div>
                )}
                {bench && (
                  <div className={bench.tier === "heavy" ? "text-warn" : "text-ok"}>
                    {t("benchmarkResult", { seconds: bench.seconds.toFixed(1), tier: t(tierKey[bench.tier]) })}
                    {bench.tier === "heavy" && model.id !== "small-q5" && (
                      <div className="mt-1 flex items-center gap-2">
                        {t("tierHeavyHint")}
                        <Button
                          size="sm"
                          onClick={() => {
                            setChoice("small-q5");
                            setBench(null);
                          }}
                        >
                          {t("download")}
                        </Button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {STEPS[step] === "wizardMicTest" && (
          <div className="space-y-3">
            <Mic size={28} className="text-fg" />
            <p>{t("wizardMicTestBody")}</p>
            <div className="flex items-center gap-3">
              <Button variant="primary" onClick={runMicTest} disabled={testing}>
                {testing ? <Loader2 size={14} className="animate-spin" /> : <Mic size={14} />}
                {testing ? t("wizardMicListening") : t("wizardMicStart")}
              </Button>
              <MicWave dbfs={testing ? micLevel : undefined} active={testing} label={t("me")} />
            </div>
            {mic && (
              <div className="animate-fade-up rounded-2xl border border-line bg-surface p-3 text-[13px]">
                {mic.device && <div className="mb-1 text-xs text-muted">{mic.device}</div>}
                {mic.text ? (
                  <>
                    <div className="text-xs text-muted">{t("wizardMicResult")}</div>
                    <div className="selectable">{mic.text}</div>
                  </>
                ) : (
                  <div className="text-xs text-muted">{t("wizardMicNothing")}</div>
                )}
              </div>
            )}
          </div>
        )}

        {STEPS[step] === "wizardHotkeys" && settings && (
          <div className="space-y-3">
            <Keyboard size={28} className="text-fg" />
            <p>{t("wizardHotkeysBody")}</p>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2">
              <dt className="text-muted">{t("hotkeyToggle")}</dt>
              <dd>{settings.hotkeys.toggle ? <Kbd keys={settings.hotkeys.toggle} /> : <span className="text-muted">{t("hotkeyOff")}</span>}</dd>
              <dt className="text-muted">{t("hotkeyScreenshot")}</dt>
              <dd>{settings.hotkeys.screenshot ? <Kbd keys={settings.hotkeys.screenshot} /> : <span className="text-muted">{t("hotkeyOff")}</span>}</dd>
            </dl>
          </div>
        )}

        {STEPS[step] === "wizardDone" && (
          <div className="space-y-3">
            <p>{t("wizardDoneBody")}</p>
          </div>
        )}
      </div>
      <div className="flex shrink-0 items-center justify-between gap-2 border-t border-line px-4 pt-2.5 pb-3">
        {step > 0 ? <Button onClick={() => setStep(step - 1)}>{t("back")}</Button> : <span />}
        <div className="flex gap-2">
          {STEPS[step] === "wizardMicTest" && (
            <Button variant="ghost" onClick={() => setStep(step + 1)}>
              {t("skip")}
            </Button>
          )}
          {step < STEPS.length - 1 ? (
            <Button variant="primary" disabled={!canNext} onClick={() => setStep(step + 1)}>
              {t("next")}
            </Button>
          ) : (
            <Button variant="primary" onClick={finish}>
              {t("wizardFinish")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
