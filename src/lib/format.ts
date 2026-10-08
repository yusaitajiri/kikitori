// Time, size and source formatting.

import type { RecordedSources } from "../ipc/types";

/** `HH:MM:SS` from a duration in ms. */
export function hms(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}`;
}

/** Wall-clock `HH:MM:SS` of a session offset, in the session's own UTC offset. */
export function clockAt(startedAt: string, tMs: number): string {
  const m = /([+-])(\d{2}):(\d{2})$|Z$/.exec(startedAt);
  const base = Date.parse(startedAt);
  if (Number.isNaN(base)) return hms(tMs);
  let offsetMin = 0;
  if (m && m[1]) offsetMin = (m[1] === "-" ? -1 : 1) * (Number(m[2]) * 60 + Number(m[3]));
  const local = new Date(base + tMs + offsetMin * 60_000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(local.getUTCHours())}:${p(local.getUTCMinutes())}:${p(local.getUTCSeconds())}`;
}

/** A later recording onto a session (FR-08): from `atMs` on, clock times count from `startedAt`. */
export type Continuation = { atMs: number; startedAt: string };

/** Clock times of a session: from its start, or from the start of the continuation an offset falls in. */
export function sessionClock(startedAt: string, continued: readonly Continuation[] = []): (tMs: number) => string {
  return (tMs) => {
    const part = continued.findLast((c) => c.atMs <= tMs);
    return part ? clockAt(part.startedAt, tMs - part.atMs) : clockAt(startedAt, tMs);
  };
}

/** `2026-10-02 15:13` in the session's offset. */
export function dateTimeOf(startedAt: string): string {
  const m = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/.exec(startedAt);
  return m ? `${m[1]} ${m[2]}` : startedAt;
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e8 ? 0 : 1)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} kB`;
  return `${n} B`;
}

/** Maps −60..0 dBFS onto 0..1 for the level bars. */
export function levelFraction(dbfs: number | undefined): number {
  if (dbfs === undefined || Number.isNaN(dbfs)) return 0;
  return Math.min(1, Math.max(0, (dbfs + 60) / 60));
}

/** `15:13` in the session's offset. */
export function timeOf(startedAt: string): string {
  const m = /T(\d{2}:\d{2})/.exec(startedAt);
  return m ? m[1] : "";
}

/** A short duration: `1時間12分`, `32分`, `45秒` (keys `durHm`, `durM`, `durS`). */
export function durationText(ms: number, t: (k: string, p?: Record<string, unknown>) => string): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return t("durS", { s });
  const m = Math.round(s / 60);
  return m < 60 ? t("durM", { m }) : t("durHm", { h: Math.floor(m / 60), m: m % 60 });
}

/** The recording time in the deck: `12:16` under an hour, `1:02:03` after. */
export function timerText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (n: number) => String(n).padStart(2, "0");
  return s >= 3600 ? `${Math.floor(s / 3600)}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}` : `${p(Math.floor(s / 60))}:${p(s % 60)}`;
}

/** A duration as numbers and units, for big numerals: `[[1, "h"], [12, "m"]]`, `[[45, "s"]]`. */
export function durationParts(ms: number): [number, "h" | "m" | "s"][] {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return [[s, "s"]];
  const m = Math.round(s / 60);
  return m < 60 ? [[m, "m"]] : [[Math.floor(m / 60), "h"], ...(m % 60 ? ([[m % 60, "m"]] as [number, "m"][]) : [])];
}

const JA_WEEK = "日月火水木金土";

/** The calendar day of a session start (`2026-10-03`), in the session's own offset. */
export const dayOf = (startedAt: string) => startedAt.slice(0, 10);

/** `10月3日（土）` / `Sat, Oct 3` for a `2026-10-03` day; the year is added when it is not `thisYear`. */
export function dayText(day: string, lang: string, thisYear?: number): string {
  const [y, m, d] = day.split("-").map(Number);
  if (!y || !m || !d) return day;
  const date = new Date(Date.UTC(y, m - 1, d));
  const year = thisYear !== undefined && y !== thisYear;
  if (lang === "ja") return `${year ? `${y}年` : ""}${m}月${d}日（${JA_WEEK[date.getUTCDay()]}）`;
  return new Intl.DateTimeFormat("en", { weekday: "short", month: "short", day: "numeric", ...(year ? { year: "numeric" } : {}), timeZone: "UTC" }).format(date);
}

/** `今日`, `昨日` or nothing for a day, seen from `now` (local time). */
export function dayName(day: string, now: Date): "today" | "yesterday" | null {
  const key = (n: number) => {
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - n);
    const p = (v: number) => String(v).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  };
  return day === key(0) ? "today" : day === key(1) ? "yesterday" : null;
}

/** What is being recorded, in the UI language: `Zoom + マイク`, `システム全体`. */
export function sourcesText(sources: RecordedSources | undefined, t: (k: string) => string): string {
  if (!sources) return "";
  const name = (id: string) => (id === "app" ? (sources.appName ?? t("srcApps")) : id === "system" ? t("srcSystem") : t("srcMic"));
  return sources.ids.map(name).join(" + ");
}

/** Splits a catalog name like `標準 (large-v3-turbo q5_0)` into its everyday and technical parts. */
export function modelNameParts(name: string): { short: string; tech: string } {
  const m = /^(.*?)\s*[(（](.+)[)）]\s*$/.exec(name);
  return m ? { short: m[1], tech: m[2] } : { short: name, tech: "" };
}
