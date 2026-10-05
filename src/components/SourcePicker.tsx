import { AppWindow, Check, ChevronDown, Mic, MonitorSpeaker, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { AudioApp, SourceMode } from "../ipc/types";
import { reportError } from "../lib/actions";
import { isBusy, useRecording } from "../store/recording";
import { useSettings } from "../store/settings";
import { useSource } from "../store/source";
import { AppWave } from "./AppWave";
import { IconButton, SwitchMark } from "./ui";

export function AppIcon({ src, size = 20 }: { src?: string; size?: number }) {
  return src ? (
    <img src={src} alt="" width={size} height={size} className="shrink-0 rounded-[5px]" draggable={false} />
  ) : (
    <AppWindow size={size - 2} className="shrink-0 text-muted" aria-hidden />
  );
}

/**
 * A list floating over everything (a portal), under its anchor, or above it when there is no room
 * below. It is open while `rect` (the anchor's box when it was opened) is set.
 */
function Popover({ anchor, rect, onClose, label, children }: { anchor: RefObject<HTMLElement | null>; rect: DOMRect | null; onClose: () => void; label: string; children: ReactNode }) {
  const list = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  const open = !!rect;

  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    if (!open) return;
    const outside = (e: Event) => {
      const target = e.target as Node;
      if (!anchor.current?.contains(target) && !list.current?.contains(target)) close.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      close.current();
      anchor.current?.querySelector<HTMLElement>("[aria-haspopup]")?.focus();
    };
    const onResize = () => close.current();
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onResize);
    list.current?.querySelector<HTMLElement>("[role=option][aria-selected=true], [role=option]:not(:disabled)")?.focus();
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
    };
  }, [open, anchor]);

  if (!rect) return null;
  const below = window.innerHeight - rect.bottom - 8;
  const above = rect.top - 8;
  const up = below < 160 && above > below;
  const maxHeight = Math.min(300, up ? above : below);
  const onListKey = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const options = [...(list.current?.querySelectorAll<HTMLElement>("[role=option]:not(:disabled)") ?? [])];
    const i = options.indexOf(document.activeElement as HTMLElement);
    options[Math.max(0, Math.min(options.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))]?.focus();
  };
  return createPortal(
    <div
      ref={list}
      role="listbox"
      aria-label={label}
      onKeyDown={onListKey}
      className="fixed z-[60] animate-pop overflow-y-auto rounded-2xl border border-line bg-surface p-1 shadow-float"
      style={{ left: rect.left, width: rect.width, maxHeight, ...(up ? { bottom: window.innerHeight - rect.top + 6 } : { top: rect.bottom + 6 }) }}
    >
      {children}
    </div>,
    document.body,
  );
}

function Option({ selected, disabled, title, onPick, icon, label, extra }: { selected: boolean; disabled?: boolean; title?: string; onPick: () => void; icon: ReactNode; label: string; extra?: ReactNode }) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      disabled={disabled}
      title={title}
      onClick={onPick}
      className={`flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left text-[13px] transition-colors hover:bg-surface-2 focus-visible:bg-surface-2 focus-visible:outline-none disabled:opacity-45 ${selected ? "font-semibold" : ""}`}
    >
      <span className="grid size-5 shrink-0 place-items-center text-fg">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {extra}
      {/* Kept in every row, so the apps' waves line up. */}
      <Check size={14} className={`shrink-0 ${selected ? "" : "invisible"}`} />
    </button>
  );
}

function Group({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return (
    <div role="presentation" className="flex items-center justify-between px-2.5 pt-2 pb-0.5 text-[11px] font-bold text-muted">
      {children}
      {action}
    </div>
  );
}

const fieldClass = (dense: boolean, open = false) =>
  `flex min-w-0 items-center gap-2.5 rounded-xl border bg-surface px-3 text-left transition-colors duration-150 ${dense ? "h-10" : "h-12"} ${
    open ? "border-fg" : "border-line hover:border-line-strong"
  }`;

/** What 相手 is: an app, everything the PC plays, or nothing but the mic (FR-10, FR-11). */
function SourceField({ dense }: { dense: boolean }) {
  const { t } = useTranslation();
  const src = useSource();
  const appSupported = useSettings((s) => s.info?.appLoopbackSupported ?? true);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const open = !!rect;
  const anchor = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const withSession = src.apps.filter((a) => a.hasSession);
  const others = src.apps.filter((a) => !a.hasSession);
  const isApp = (a: AudioApp) => src.mode === "app" && (src.app?.rootPid ? a.rootPid === src.app.rootPid : a.exe.toLowerCase() === src.app?.exe.toLowerCase());
  const pick = (mode: SourceMode, app?: AudioApp) => {
    if (app) src.setApp(app);
    src.setMode(mode);
    setRect(null);
    trigger.current?.focus();
  };

  const iconSize = dense ? 17 : 20;
  let icon: ReactNode;
  let name: string;
  if (src.mode === "app") {
    icon = <AppIcon src={src.app?.iconDataUrl} size={dense ? 18 : 22} />;
    name = src.app?.name ?? t("chooseApp");
  } else if (src.mode === "system") {
    icon = <MonitorSpeaker size={iconSize} />;
    name = t("srcSystem");
  } else {
    icon = <Mic size={iconSize} />;
    name = t("srcMicOnly");
  }

  const appOption = (a: AudioApp) => (
    <Option
      key={a.rootPid}
      selected={isApp(a)}
      disabled={!appSupported}
      title={appSupported ? undefined : t("win11Required")}
      onPick={() => pick("app", a)}
      icon={<AppIcon src={a.iconDataUrl} size={20} />}
      label={a.name}
      extra={a.hasSession && <AppWave pid={a.rootPid} />}
    />
  );

  return (
    <div ref={anchor} className="min-w-0">
      <button
        ref={trigger}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${t("chooseSource")}: ${name}`}
        onClick={() => {
          if (!open) void src.refreshApps();
          setRect(open ? null : (anchor.current?.getBoundingClientRect() ?? null));
        }}
        className={`${fieldClass(dense, open)} w-full`}
      >
        <span className="grid size-[22px] shrink-0 place-items-center">{icon}</span>
        <span className={`min-w-0 flex-1 truncate text-[13.5px] font-semibold ${src.mode === "app" && !src.app ? "text-muted" : ""}`}>{name}</span>
        {src.mode === "app" && <AppWave pid={src.app?.rootPid} width={dense ? 30 : 36} height={dense ? 14 : 16} />}
        <ChevronDown size={15} className={`shrink-0 text-muted transition-transform duration-200 ${open ? "rotate-180" : ""}`} />
      </button>
      <Popover anchor={anchor} rect={rect} onClose={() => setRect(null)} label={t("chooseSource")}>
        <Option selected={src.mode === "system"} onPick={() => pick("system")} icon={<MonitorSpeaker size={18} />} label={t("srcSystem")} />
        <Option selected={src.mode === "mic"} onPick={() => pick("mic")} icon={<Mic size={18} />} label={t("srcMicOnly")} />
        <Group
          action={
            <IconButton size="sm" label={t("refresh")} onClick={() => src.refreshApps()} disabled={src.loadingApps} className="!size-6">
              <RefreshCw size={13} className={src.loadingApps ? "animate-spin" : ""} />
            </IconButton>
          }
        >
          {t("srcApps")}
        </Group>
        {withSession.map(appOption)}
        {others.length > 0 && <Group>{t("otherApps")}</Group>}
        {others.map(appOption)}
        {src.apps.length === 0 && <div className="px-2.5 py-3 text-center text-[12px] text-muted">{src.loadingApps ? t("loading") : t("noApps")}</div>}
      </Popover>
    </div>
  );
}

/**
 * Whether to record a conversation: the mic too, so your own lines are told apart as 自分
 * (「会話として録音」, FR-12); or, with the mic as the only source, just which mic it is (FR-13).
 */
function MicField({ dense, only }: { dense: boolean; only: boolean }) {
  const { t } = useTranslation();
  const { includeMic, setIncludeMic, devices, micDeviceId, setMicDevice, refreshDevices } = useSource();
  const update = useSettings((s) => s.update);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const open = !!rect;
  const anchor = useRef<HTMLDivElement>(null);
  const device = devices.find((d) => d.id === micDeviceId)?.name ?? t("defaultDevice");
  const on = only || includeMic;
  const choose = (id: string | null) => {
    setMicDevice(id);
    update({ source: { micDeviceId: id } }).catch((e) => reportError(e, "saveFailed"));
    setRect(null);
  };
  const labels = (
    <>
      <Mic size={dense ? 17 : 19} className={`shrink-0 transition-colors ${on ? "text-fg" : "text-muted"}`} aria-hidden />
      <span className="grid min-w-0 flex-1 leading-tight">
        <span className={`truncate text-[13.5px] font-semibold transition-colors ${on ? "" : "text-muted"}`}>{only ? t("srcMic") : t("conversation")}</span>
        {!dense && <span className="truncate text-[11px] text-muted">{device}</span>}
      </span>
    </>
  );
  return (
    <div ref={anchor} className={`${fieldClass(dense)} !gap-1 !pr-1.5`}>
      {only ? (
        <span className="flex min-w-0 flex-1 items-center gap-2.5">{labels}</span>
      ) : (
        <button type="button" role="switch" aria-checked={includeMic} aria-label={t("conversation")} onClick={() => setIncludeMic(!includeMic)} className="flex h-full min-w-0 flex-1 items-center gap-2.5 text-left focus-visible:outline-offset-0">
          {labels}
          <SwitchMark on={includeMic} />
        </button>
      )}
      <IconButton
        size="sm"
        label={t("chooseMic")}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => {
          if (!open) void refreshDevices().catch(() => {});
          setRect(open ? null : (anchor.current?.getBoundingClientRect() ?? null));
        }}
      >
        <ChevronDown size={15} className={`transition-transform duration-200 ${open ? "rotate-180" : ""}`} />
      </IconButton>
      <Popover anchor={anchor} rect={rect} onClose={() => setRect(null)} label={t("chooseMic")}>
        <Option selected={!micDeviceId} onPick={() => choose(null)} icon={<Mic size={16} />} label={t("defaultDevice")} />
        {devices.map((d) => (
          <Option key={d.id} selected={micDeviceId === d.id} onPick={() => choose(d.id)} icon={<Mic size={16} />} label={d.name} />
        ))}
      </Popover>
    </div>
  );
}

/**
 * What to listen to (an app, everything the PC plays, or the mic alone), and whether it is a
 * conversation: then the mic records your voice too and the transcript names the lines 相手 and
 * 自分. Locked while recording.
 */
export function SourcePicker({ dense = false }: { dense?: boolean }) {
  const { t } = useTranslation();
  const busy = isBusy(useRecording((s) => s.state));
  const { mode, refreshApps } = useSource();

  useEffect(() => {
    void refreshApps();
  }, [refreshApps]);

  return (
    <fieldset disabled={busy} className={`grid min-w-0 ${dense ? "gap-1.5" : "gap-2"}`}>
      <legend className="sr-only">{t("source")}</legend>
      <SourceField dense={dense} />
      <MicField dense={dense} only={mode === "mic"} />
    </fieldset>
  );
}
