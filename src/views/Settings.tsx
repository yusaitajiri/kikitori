import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Button, Field, inputClass, Section, Select, Toggle } from "../components/ui";
import { PageTitle } from "../components/PageTitle";
import { useModelName } from "../hooks/useModelName";
import { commands } from "../ipc/commands";
import type { DeepPartial, Settings as SettingsT } from "../ipc/types";
import { reportError } from "../lib/actions";
import { acceleratorFrom } from "../lib/hotkeys";
import { useSettings } from "../store/settings";
import { useSource } from "../store/source";
import { type SettingsTab, useUi } from "../store/ui";
import { ModelManager } from "./ModelManager";

const TABS: { id: Exclude<SettingsTab, "index">; key: string }[] = [
  { id: "general", key: "tabGeneral" },
  { id: "audio", key: "tabAudio" },
  { id: "transcription", key: "tabTranscription" },
  { id: "models", key: "tabModels" },
  { id: "screenshots", key: "tabScreenshots" },
  { id: "hotkeys", key: "tabHotkeys" },
  { id: "output", key: "tabOutput" },
];

function useSave() {
  const update = useSettings((s) => s.update);
  return async (partial: DeepPartial<SettingsT>) => {
    try {
      await update(partial);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };
}

function Advanced({ children }: { children: React.ReactNode }) {
  const { t } = useTranslation();
  return (
    <details className="group rounded-2xl border border-line bg-surface px-3.5">
      <summary className="flex cursor-pointer list-none items-center justify-between py-2.5 text-[12px] font-semibold text-muted transition-colors hover:text-fg [&::-webkit-details-marker]:hidden">
        {t("advanced")}
        <ChevronDown size={15} className="transition-transform duration-200 group-open:rotate-180" />
      </summary>
      <div className="animate-slide-down divide-y divide-line border-t border-line">{children}</div>
    </details>
  );
}

function General({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const save = useSave();
  return (
    <Section>
      <Field label={t("language")}>
        <Select ariaLabel={t("language")} value={s.locale} onChange={(v) => save({ locale: v })} options={[{ value: "ja", label: "日本語" }, { value: "en", label: "English" }]} />
      </Field>
      <Field label={t("layout")}>
        <Select
          ariaLabel={t("layout")}
          value={s.window.layout}
          onChange={(v) => save({ window: { layout: v } })}
          options={[
            { value: "compact", label: t("layoutCompact") },
            { value: "expanded", label: t("layoutExpanded") },
          ]}
        />
      </Field>
      <Toggle label={t("checkUpdates")} checked={s.updates.check} onChange={(v) => save({ updates: { check: v } })} />
    </Section>
  );
}

function Audio({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const save = useSave();
  const { devices, refreshDevices, setMicDevice } = useSource();
  useEffect(() => {
    void refreshDevices().catch(() => {});
  }, [refreshDevices]);
  return (
    <>
      <Section>
        <Field label={t("micDevice")}>
          <Select
            ariaLabel={t("micDevice")}
            value={s.source.micDeviceId ?? ""}
            onChange={(v) => {
              setMicDevice(v || null);
              void save({ source: { micDeviceId: v || null } });
            }}
            options={[{ value: "", label: t("defaultDevice") }, ...devices.map((d) => ({ value: d.id, label: d.name }))]}
          />
        </Field>
        <Toggle label={t("echoGuard")} checked={s.echoGuard} onChange={(v) => save({ echoGuard: v })} />
      </Section>
      <Advanced>
        <Field label={`${t("vadThreshold")}: ${s.vad.startThreshold.toFixed(2)}`}>
          <input
            type="range"
            aria-label={t("vadThreshold")}
            className="w-full accent-[var(--kk-accent)]"
            min={0.3}
            max={0.8}
            step={0.05}
            value={s.vad.startThreshold}
            onChange={(e) => save({ vad: { startThreshold: Number(e.target.value) } })}
          />
        </Field>
        <Field label={`${t("hangover")}: ${s.vad.hangoverMs}`}>
          <input
            type="range"
            aria-label={t("hangover")}
            className="w-full accent-[var(--kk-accent)]"
            min={400}
            max={1200}
            step={40}
            value={s.vad.hangoverMs}
            onChange={(e) => save({ vad: { hangoverMs: Number(e.target.value) } })}
          />
        </Field>
      </Advanced>
    </>
  );
}

function Transcription({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const save = useSave();
  const info = useSettings((st) => st.info);
  const [vocab, setVocab] = useState(s.vocabulary.join("\n"));
  return (
    <>
      <Section>
        <Field label={t("transcriptionLanguage")}>
          <Select
            ariaLabel={t("transcriptionLanguage")}
            value={s.language}
            onChange={(v) => save({ language: v })}
            options={[
              { value: "ja", label: t("langJa") },
              { value: "en", label: t("langEn") },
              { value: "auto", label: t("langAuto") },
            ]}
          />
        </Field>
        <Toggle label={t("useGpu")} checked={s.useGpu} onChange={(v) => save({ useGpu: v })} disabled={!info?.gpuCompiled} />
        <Toggle label={t("accuracyFirst")} checked={s.accuracyFirst} onChange={(v) => save({ accuracyFirst: v })} />
        <Toggle label={t("partials")} checked={s.partials} onChange={(v) => save({ partials: v })} />
        <Field label={t("vocabulary")}>
          <textarea
            aria-label={t("vocabulary")}
            className={`${inputClass} selectable !h-24 resize-y py-2`}
            value={vocab}
            onChange={(e) => setVocab(e.target.value)}
            onBlur={() => save({ vocabulary: vocab.split("\n").map((v) => v.trim()).filter(Boolean) })}
          />
        </Field>
      </Section>
      <Advanced>
        <Toggle label={t("hallucinationFilter")} checked={s.hallucinationFilter} onChange={(v) => save({ hallucinationFilter: v })} />
        <Toggle label={t("audioCtx")} checked={s.audioCtxExperimental} onChange={(v) => save({ audioCtxExperimental: v })} />
      </Advanced>
    </>
  );
}

function Screenshots({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const save = useSave();
  return (
    <Section>
      <Field label={t("screenshotTarget")}>
        <Select
          ariaLabel={t("screenshotTarget")}
          value={s.screenshot.target}
          onChange={(v) => save({ screenshot: { target: v } })}
          options={[
            { value: "appWindow", label: t("targetApp") },
            { value: "cursorMonitor", label: t("targetCursor") },
            { value: "allMonitors", label: t("targetAll") },
          ]}
        />
      </Field>
      <Toggle label={t("excludeSelf")} checked={s.screenshot.excludeSelf} onChange={(v) => save({ screenshot: { excludeSelf: v } })} />
      <Toggle label={t("shutterSound")} checked={s.screenshot.sound} onChange={(v) => save({ screenshot: { sound: v } })} />
    </Section>
  );
}

/** A shortcut: off (empty) until you press 設定 and then the keys; オフにする turns it off again. */
function HotkeyInput({ value, onChange, label, error }: { value: string; onChange: (v: string) => void; label: string; error?: string }) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  return (
    <Field label={label}>
      <div className="flex gap-1.5">
        <button
          type="button"
          className={`${inputClass} min-w-0 flex-1 text-left ${value && !recording ? "font-mono" : "text-muted"} ${recording ? "!border-accent !bg-info-bg !text-fg" : ""}`}
          onClick={() => setRecording(true)}
          onBlur={() => setRecording(false)}
          onKeyDown={(e) => {
            if (!recording) return;
            e.preventDefault();
            if (e.key === "Escape") {
              setRecording(false);
              return;
            }
            const acc = acceleratorFrom(e.nativeEvent);
            if (acc) {
              setRecording(false);
              onChange(acc);
            }
          }}
        >
          {recording ? t("hotkeyRecord") : value || t("hotkeyOff")}
        </button>
        {value && !recording && <Button onClick={() => onChange("")}>{t("hotkeyClear")}</Button>}
      </div>
      {error && <div className="mt-1 text-xs font-semibold text-err">{t("hotkeyFailed", { error })}</div>}
    </Field>
  );
}

function Hotkeys({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const save = useSave();
  const { info, refreshInfo } = useSettings();
  const set = async (k: keyof SettingsT["hotkeys"], v: string) => {
    await save({ hotkeys: { [k]: v } });
    await refreshInfo();
  };
  return (
    <Section>
      <HotkeyInput label={t("hotkeyToggle")} value={s.hotkeys.toggle} onChange={(v) => set("toggle", v)} error={info?.hotkeyErrors.toggle} />
      <HotkeyInput label={t("hotkeyScreenshot")} value={s.hotkeys.screenshot} onChange={(v) => set("screenshot", v)} error={info?.hotkeyErrors.screenshot} />
      <HotkeyInput label={t("hotkeyMark")} value={s.hotkeys.mark} onChange={(v) => set("mark", v)} error={info?.hotkeyErrors.mark} />
      <HotkeyInput label={t("hotkeyCut")} value={s.hotkeys.cut} onChange={(v) => set("cut", v)} error={info?.hotkeyErrors.cut} />
    </Section>
  );
}

function Output({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const save = useSave();
  const [template, setTemplate] = useState(s.output.titleTemplate);
  return (
    <>
      <Section>
        <Field label={t("outputRoot")}>
          <div className="flex gap-1.5">
            <input aria-label={t("outputRoot")} readOnly className={`${inputClass} min-w-0 flex-1 !bg-surface-2 text-[12px]`} value={s.output.root} />
            <Button
              onClick={async () => {
                const folder = await commands.pickFolder();
                if (folder) await save({ output: { root: folder } });
              }}
            >
              {t("change")}
            </Button>
          </div>
        </Field>
        <Field label={t("titleTemplate")} help={t("titleTemplateHelp", { app: "{app}", date: "{date}" })}>
          <input
            aria-label={t("titleTemplate")}
            className={inputClass}
            value={template}
            onChange={(e) => setTemplate(e.target.value)}
            onBlur={() => save({ output: { titleTemplate: template } })}
          />
        </Field>
      </Section>
      <Section>
        <Toggle label={t("timestamps")} checked={s.export.timestamps} onChange={(v) => save({ export: { timestamps: v } })} />
        <Field label={t("labels")}>
          <Select
            ariaLabel={t("labels")}
            value={s.export.labels}
            onChange={(v) => save({ export: { labels: v } })}
            options={[
              { value: "auto", label: t("labelsAuto") },
              { value: "on", label: t("labelsOn") },
              { value: "off", label: t("labelsOff") },
            ]}
          />
        </Field>
        <Toggle label={t("mergeParagraphs")} checked={s.export.mergeParagraphs} onChange={(v) => save({ export: { mergeParagraphs: v } })} />
        <Toggle label={t("screenshotMarkers")} checked={s.copy.screenshotMarkers} onChange={(v) => save({ copy: { screenshotMarkers: v } })} />
        <Toggle label={t("autoCopyOnStop")} checked={s.autoCopyOnStop} onChange={(v) => save({ autoCopyOnStop: v })} />
      </Section>
    </>
  );
}

const targetKey = { cursorMonitor: "targetCursor", allMonitors: "targetAll", appWindow: "targetApp" } as const;
const languageKey = { ja: "langJa", en: "langEn", auto: "langAuto" } as const;

/** The last folders of a path: `Documents › Kikitori`. */
const shortPath = (path: string) => path.split(/[\\/]/).filter(Boolean).slice(-2).join(" › ");

/** 設定 as a list: each section with what it is set to now. */
function SettingsIndex({ s }: { s: SettingsT }) {
  const { t } = useTranslation();
  const go = useUi((st) => st.go);
  const info = useSettings((st) => st.info);
  const devices = useSource((st) => st.devices);
  const model = useModelName(s.modelId);
  const hotkeysTaken = Object.keys(info?.hotkeyErrors ?? {}).length > 0;
  const summary: Record<(typeof TABS)[number]["id"], string> = {
    general: s.locale === "ja" ? "日本語" : "English",
    audio: [devices.find((d) => d.id === s.source.micDeviceId)?.name ?? t("defaultDevice"), s.echoGuard ? t("echoGuard") : null].filter(Boolean).join(" · "),
    transcription: `${t(languageKey[s.language])} · ${s.useGpu && info?.gpuCompiled ? t("gpu") : t("cpu")}`,
    models: model?.full ?? s.modelId,
    screenshots: t(targetKey[s.screenshot.target]),
    hotkeys: hotkeysTaken ? t("hotkeyTaken") : [s.hotkeys.toggle, s.hotkeys.screenshot, s.hotkeys.mark, s.hotkeys.cut].filter(Boolean).join(" · ") || t("hotkeyOff"),
    output: shortPath(s.output.root),
  };
  return (
    <div className="flex h-full flex-col">
      <PageTitle title={t("settingsTitle")} />
      <div className="min-h-0 flex-1 overflow-y-auto px-3.5 pb-4">
        <ul className="border-t border-line">
          {TABS.map((tab) => (
            <li key={tab.id}>
              <button
                type="button"
                onClick={() => go("settings", { tab: tab.id })}
                className="grid w-full grid-cols-[minmax(0,1fr)_auto_14px] items-center gap-2.5 border-b border-line px-1.5 py-3 text-left transition-colors hover:bg-surface-2"
              >
                <span className="text-[13.5px] font-semibold">{t(tab.key)}</span>
                <span className={`max-w-[210px] truncate text-right text-[12px] ${tab.id === "hotkeys" && hotkeysTaken ? "font-bold text-fg" : "text-muted"}`}>{summary[tab.id]}</span>
                <ChevronRight size={14} className="text-muted" />
              </button>
            </li>
          ))}
        </ul>
        <div className="mt-3 flex items-center justify-between px-1.5 text-[11.5px] text-muted">
          <span>{info && t("version", { v: info.version })}</span>
          <button type="button" className="kk-link" onClick={() => commands.openLogs()}>
            {t("openLogs")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** 設定: the list of sections, or one section with a way back to the list. */
export function Settings() {
  const { t } = useTranslation();
  const settingsTab = useUi((st) => st.settingsTab);
  const s = useSettings((st) => st.settings);
  if (!s) return null;
  const tab = TABS.find((x) => x.id === settingsTab);
  if (!tab) return <SettingsIndex s={s} />;
  return (
    <div className="flex h-full flex-col">
      <PageTitle title={t(tab.key)} sub={t("settingsTitle")} />
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-line">
        {settingsTab === "models" ? (
          <ModelManager />
        ) : (
          <div key={settingsTab} className="animate-fade-up space-y-3 px-3.5 pt-3 pb-4">
            {settingsTab === "general" && <General s={s} />}
            {settingsTab === "audio" && <Audio s={s} />}
            {settingsTab === "transcription" && <Transcription s={s} />}
            {settingsTab === "screenshots" && <Screenshots s={s} />}
            {settingsTab === "hotkeys" && <Hotkeys s={s} />}
            {settingsTab === "output" && <Output s={s} />}
          </div>
        )}
      </div>
    </div>
  );
}
