import { convertFileSrc } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, Camera, Check, ImagePlus, Link2, Pause, Pencil, Play, Scissors, Trash2, TriangleAlert, X } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../ipc/commands";
import type { Marker, Screenshot, Segment, SourceId, TimelineItem } from "../ipc/types";
import { reportError } from "../lib/actions";
import { clockAt } from "../lib/format";
import { orderTimeline, type PartialItem } from "../lib/timeline";
import { useTranscriptStore } from "../store/transcript";
import { LINE_Y } from "./Deck";
import { Button } from "./ui";

type Row =
  | { key: string; kind: "header" }
  | { key: string; kind: "item"; item: TimelineItem; head: boolean }
  | { key: string; kind: "partial"; partial: PartialItem; head: boolean };

/** A new turn (speaker and time shown) starts at a speaker change, after a picture or marker, and every minute. */
const TURN_MS = 60_000;

/** Further than this many screens from the newest line, 最新へ jumps instead of gliding. */
const FAR_SCREENS = 2;

function useRows(withHeader: boolean): { rows: Row[]; twoSpeakers: boolean; startedAt?: string } {
  const store = useTranscriptStore();
  const items = store((s) => s.items);
  const partials = store((s) => s.partials);
  const sources = store((s) => s.sources);
  const startedAt = store((s) => s.startedAt);
  return useMemo(() => {
    const pending: PartialItem[] = [];
    for (const source of ["app", "system", "mic"] as SourceId[]) {
      const p = partials[source];
      if (p?.text) pending.push({ kind: "partial", id: `~partial-${source}`, source, text: p.text, tStartMs: p.tStartMs });
    }
    const rows: Row[] = withHeader ? [{ key: "header", kind: "header" }] : [];
    let turnSource: SourceId | null = null;
    let turnStart = Number.NEGATIVE_INFINITY;
    for (const e of orderTimeline<TimelineItem | PartialItem>([...items, ...pending])) {
      if (e.kind === "segment" || e.kind === "partial") {
        const t = e.tStartMs ?? turnStart;
        const head = turnSource !== e.source || t - turnStart >= TURN_MS;
        if (head) {
          turnSource = e.source;
          turnStart = t;
        }
        rows.push(e.kind === "segment" ? { key: e.id, kind: "item", item: e, head } : { key: e.id, kind: "partial", partial: e, head });
      } else {
        turnSource = null;
        rows.push({ key: e.id, kind: "item", item: e, head: true });
      }
    }
    // Speaker names only matter when both sides are recorded.
    const twoSpeakers = sources.some((s) => s.id === "mic") && sources.some((s) => s.id !== "mic");
    return { rows, twoSpeakers, startedAt };
  }, [items, partials, sources, startedAt, withHeader]);
}

/** Whether an item arrived live a moment ago (it animates in). */
function useFresh(id: string): boolean {
  const store = useTranscriptStore();
  return store((s) => !!s.fresh[id]);
}

/** A turn's first line: the speaker's name in the colour of their side of the line (相手 red, 自分 ink), then the time. */
function TurnHead({ source, time, twoSpeakers }: { source: SourceId; time: string; twoSpeakers: boolean }) {
  const { t } = useTranslation();
  const me = source === "mic";
  return (
    <div className="mb-0.5 flex items-baseline gap-2 text-[11.5px]">
      {twoSpeakers && <span className={`font-bold ${me ? "text-me" : "text-others"}`}>{me ? t("me") : t("others")}</span>}
      <span className="num text-[11px] text-muted">{time}</span>
    </div>
  );
}

function RowActions({ children }: { children: ReactNode }) {
  return (
    <span className="absolute top-1 right-2 z-10 flex gap-0.5 rounded-lg border border-line bg-surface p-0.5 opacity-0 shadow-card transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
      {children}
    </span>
  );
}

const actionClass = "grid size-6 place-items-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-fg";

function SegmentRow({ seg, head, startedAt, twoSpeakers, editable }: { seg: Segment; head: boolean; startedAt?: string; twoSpeakers: boolean; editable: boolean }) {
  const { t } = useTranslation();
  const store = useTranscriptStore();
  const sessionId = store((s) => s.sessionId);
  const fresh = useFresh(seg.id);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(seg.text);

  const save = async () => {
    if (!sessionId) return;
    try {
      const updated = await commands.updateSegment(sessionId, seg.id, draft);
      store.getState().updateSegment(updated);
      setEditing(false);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };
  const remove = async () => {
    if (!sessionId) return;
    try {
      await commands.deleteSegment(sessionId, seg.id);
      store.getState().removeItem(seg.id);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };

  return (
    <div className={`group relative px-[18px] pb-0.5 ${head ? "pt-3.5" : "pt-0.5"}`}>
      {head && <TurnHead source={seg.source} time={startedAt ? clockAt(startedAt, seg.tStartMs) : ""} twoSpeakers={twoSpeakers} />}
      {editing ? (
        <div className="space-y-1.5 py-1">
          <textarea
            aria-label={t("editLine")}
            className="selectable min-h-[64px] w-full resize-y rounded-xl border border-accent bg-surface p-2 text-[14px] leading-relaxed focus:outline-none"
            value={draft}
            autoFocus
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void save();
              if (e.key === "Escape") setEditing(false);
            }}
          />
          <div className="flex justify-end gap-1.5">
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              <X size={13} />
              {t("cancel")}
            </Button>
            <Button size="sm" variant="primary" onClick={save}>
              <Check size={13} />
              {t("save")}
            </Button>
          </div>
        </div>
      ) : (
        <p className={`selectable text-[14px] leading-[1.8] break-words ${fresh ? "animate-settle" : ""}`}>
          {seg.text}
          {seg.edited && <span className="ml-1.5 rounded border border-line px-1 py-px align-middle text-[10px] text-muted">{t("edited")}</span>}
        </p>
      )}
      {editable && !editing && (
        <RowActions>
          <button
            type="button"
            aria-label={t("editLine")}
            title={t("editLine")}
            className={actionClass}
            onClick={() => {
              setDraft(seg.text);
              setEditing(true);
            }}
          >
            <Pencil size={13} />
          </button>
          <button type="button" aria-label={t("deleteLine")} title={t("deleteLine")} className={actionClass} onClick={remove}>
            <Trash2 size={13} />
          </button>
        </RowActions>
      )}
    </div>
  );
}

function ScreenshotRow({ shot, startedAt, editable }: { shot: Screenshot; startedAt?: string; editable: boolean }) {
  const { t } = useTranslation();
  const store = useTranscriptStore();
  const thumb = store((s) => s.thumbs[shot.id]);
  const folder = store((s) => s.folder);
  const sessionId = store((s) => s.sessionId);
  const fresh = useFresh(shot.id);
  const [captionOpen, setCaptionOpen] = useState(false);
  const [caption, setCaption] = useState(shot.caption ?? "");
  const time = startedAt ? clockAt(startedAt, shot.tMs) : "";
  const path = folder ? `${folder}\\${shot.file.replaceAll("/", "\\")}` : undefined;
  const src = thumb ?? (path ? convertFileSrc(path) : undefined);

  const saveCaption = async () => {
    if (!sessionId) return;
    try {
      await commands.setCaption(sessionId, shot.id, caption);
      store.getState().setCaption(shot.id, caption.trim());
      setCaptionOpen(false);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };
  const remove = async () => {
    if (!sessionId) return;
    try {
      await commands.deleteScreenshot(sessionId, shot.id);
      store.getState().removeItem(shot.id);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };

  return (
    <div className={`group relative px-[18px] pt-3.5 pb-1 ${fresh ? "animate-fade-up" : ""}`}>
      <div className="mb-1 flex items-center gap-1.5 text-muted">
        <Camera size={12} aria-hidden />
        <span className="num text-[11px]">{time}</span>
      </div>
      <figure className="min-w-0">
        {src ? (
          <button
            type="button"
            title={t("openImage")}
            aria-label={t("openImage")}
            disabled={!path}
            onClick={() => path && commands.openPath(path).catch((e) => reportError(e))}
            className="block max-w-full overflow-hidden rounded-lg border border-line bg-surface-2 transition-colors duration-200 hover:border-line-strong"
          >
            <img src={src} alt={shot.caption || t("screenshotAlt", { time })} className="max-h-[220px] w-auto max-w-full object-contain" loading="lazy" draggable={false} />
          </button>
        ) : (
          <div className="grid h-20 w-48 place-items-center rounded-lg border border-dashed border-line text-muted">
            <Camera size={18} />
          </div>
        )}
        {shot.caption && !captionOpen && <figcaption className="selectable mt-1.5 text-[12px] text-muted">{shot.caption}</figcaption>}
        {captionOpen && (
          <div className="mt-1.5 flex gap-1.5">
            <input
              aria-label={t("caption")}
              placeholder={t("caption")}
              className="h-8 min-w-0 flex-1 rounded-[10px] border border-accent bg-surface px-2.5 text-[12px] focus:outline-none"
              value={caption}
              autoFocus
              onChange={(e) => setCaption(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void saveCaption();
                if (e.key === "Escape") setCaptionOpen(false);
              }}
            />
            <Button size="sm" variant="primary" className="!h-8" onClick={saveCaption} aria-label={t("save")}>
              <Check size={13} />
            </Button>
          </div>
        )}
      </figure>
      {editable && (
        <RowActions>
          <button type="button" aria-label={t("addCaption")} title={t("addCaption")} className={actionClass} onClick={() => setCaptionOpen((v) => !v)}>
            <ImagePlus size={13} />
          </button>
          <button type="button" aria-label={t("deleteScreenshot")} title={t("deleteScreenshot")} className={actionClass} onClick={remove}>
            <Trash2 size={13} />
          </button>
        </RowActions>
      )}
    </div>
  );
}

function MarkerRow({ marker, startedAt }: { marker: Marker; startedAt?: string }) {
  const { t } = useTranslation();
  const fresh = useFresh(marker.id);
  const time = startedAt ? clockAt(startedAt, marker.tMs) : "";
  const [Icon, text] =
    marker.type === "source_reattached"
      ? [Link2, t("markerReattached")]
      : marker.type === "paused"
        ? [Pause, t("markerPaused")]
        : marker.type === "resumed"
          ? [Play, t("markerResumed")]
          : marker.type === "cut"
            ? [Scissors, t("markerCut")]
            : [TriangleAlert, t("markerUnprocessed", { n: marker.detail ?? "?" })];
  // A pause or a gap had no sound, so its line stays flat. A cut starts a part, so its line is
  // darker and it stands further from the part before.
  const cut = marker.type === "cut";
  const rule = `h-px flex-1 ${cut ? "bg-line-strong" : "bg-line"}`;
  return (
    <div className={`flex items-center gap-3 px-[18px] ${cut ? "pt-6 pb-2" : "py-3"} ${fresh ? "animate-fade-up" : ""}`}>
      <span className={rule} />
      <span className={`inline-flex items-center gap-1.5 text-[11px] font-semibold ${cut ? "text-fg" : "text-muted"}`}>
        <Icon size={11} aria-hidden />
        {text}
        {marker.type !== "unprocessed" && <span className="num font-normal">{time}</span>}
      </span>
      <span className={rule} />
    </div>
  );
}

/** Provisional words in pencil grey; the final line darkens into place where they were. */
function PartialRow({ partial, head, startedAt, twoSpeakers }: { partial: PartialItem; head: boolean; startedAt?: string; twoSpeakers: boolean }) {
  return (
    <div className={`relative px-[18px] pb-0.5 ${head ? "pt-3.5" : "pt-0.5"}`}>
      {head && <TurnHead source={partial.source} time={startedAt && partial.tStartMs !== undefined ? clockAt(startedAt, partial.tStartMs) : ""} twoSpeakers={twoSpeakers} />}
      <p className="text-[14px] leading-[1.8] break-words text-partial">{partial.text}</p>
    </div>
  );
}

const RowView = memo(function RowView({ row, startedAt, twoSpeakers, editable }: { row: Row; startedAt?: string; twoSpeakers: boolean; editable: boolean }) {
  if (row.kind === "header") return null;
  if (row.kind === "partial") return <PartialRow partial={row.partial} head={row.head} startedAt={startedAt} twoSpeakers={twoSpeakers} />;
  const item = row.item;
  if (item.kind === "segment") return <SegmentRow seg={item} head={row.head} startedAt={startedAt} twoSpeakers={twoSpeakers} editable={editable} />;
  if (item.kind === "screenshot") return <ScreenshotRow shot={item} startedAt={startedAt} editable={editable} />;
  return <MarkerRow marker={item} startedAt={startedAt} />;
});

/** Before the first words: the line below already shows that it is listening. */
function WaitingForSpeech() {
  const { t } = useTranslation();
  return <div className="flex flex-1 animate-fade-in items-center justify-center px-6 text-center text-[12px] text-muted">{t("waitingForSpeech")}</div>;
}

/** The time of a row, for jumping to a moment of the session. */
function rowTime(r: Row): number {
  if (r.kind === "partial") return r.partial.tStartMs ?? Number.MAX_SAFE_INTEGER;
  if (r.kind !== "item") return Number.NEGATIVE_INFINITY;
  return r.item.kind === "segment" ? r.item.tStartMs : r.item.tMs;
}

/**
 * The transcript as a virtualized list. Follows the newest line until the user scrolls up
 * (FR-40); `header` scrolls with the lines as the first row. Pointing at the session's line
 * (a `seek` in the store) brings that moment into view and marks it for a moment.
 */
export function Transcript({ editable, header, live }: { editable: boolean; header?: ReactNode; live?: boolean }) {
  const { t } = useTranslation();
  const { rows, twoSpeakers, startedAt } = useRows(!!header);
  const store = useTranscriptStore();
  const seek = store((s) => s.seek);
  const parentRef = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);
  const atBottomRef = useRef(atBottom);
  atBottomRef.current = atBottom;
  const lastTop = useRef(0);
  const [marked, setMarked] = useState<string | null>(null);
  const hasLines = rows.some((r) => r.kind !== "header");
  const hasHeader = !!header;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (i) => {
      const r = rows[i];
      if (r?.kind === "header") return 150;
      if (r?.kind === "item" && r.item.kind === "screenshot") return 220;
      return r && "head" in r && r.head ? 54 : 30;
    },
    overscan: 8,
    getItemKey: (i) => rows[i]?.key ?? i,
  });

  const scrollToEnd = (smooth: boolean) => {
    const el = parentRef.current;
    if (!el) return;
    // From far up (a long session left in the background) a glide takes seconds, and rows
    // measured on the way keep moving the end; jump there instead.
    const far = el.scrollHeight - el.scrollTop - el.clientHeight > FAR_SCREENS * el.clientHeight;
    el.scrollTo({ top: el.scrollHeight, behavior: smooth && !far ? "smooth" : "auto" });
  };

  // Follow new lines, and rows that grow once measured (a picture loading); wait a frame so
  // new rows are measured first.
  const totalSize = virtualizer.getTotalSize();
  useEffect(() => {
    if (!live || !atBottom || rows.length === 0) return;
    const id = requestAnimationFrame(() => scrollToEnd(true));
    return () => cancelAnimationFrame(id);
  }, [rows, totalSize, atBottom, live]);

  // Stay pinned to the newest line when the window is resized (compact ⇄ expanded).
  useEffect(() => {
    const el = parentRef.current;
    if (!el || !live) return;
    const ro = new ResizeObserver(() => atBottomRef.current && scrollToEnd(false));
    ro.observe(el);
    return () => ro.disconnect();
  }, [live, hasLines]);

  // A finished session opens at its header.
  useEffect(() => {
    if (hasHeader) parentRef.current?.scrollTo({ top: 0 });
  }, [hasHeader]);

  // Jump to the moment pointed at on the session's line.
  useEffect(() => {
    if (!seek) return;
    let index = rows.findIndex((r) => r.kind !== "header" && rowTime(r) >= seek.ms);
    if (index < 0) index = rows.length - 1;
    const row = rows[index];
    if (!row) return;
    virtualizer.scrollToIndex(index, { align: "center" });
    setMarked(row.key);
    const id = setTimeout(() => setMarked(null), 1600);
    return () => clearTimeout(id);
    // Only a new request jumps; new rows do not.
  }, [seek]); // eslint-disable-line react-hooks/exhaustive-deps

  const onScroll = () => {
    const el = parentRef.current;
    if (!el) return;
    const top = el.scrollTop;
    // Only a move up (by the user) stops following; content growing below never does.
    if (el.scrollHeight - top - el.clientHeight <= 32) setAtBottom(true);
    else if (top < lastTop.current - 2) setAtBottom(false);
    lastTop.current = top;
  };

  if (live && !hasLines) return <WaitingForSpeech />;
  if (!hasLines && !header) {
    return <div className="flex flex-1 animate-fade-in items-center justify-center px-6 text-center text-[12px] text-muted">{t("emptyTranscript")}</div>;
  }

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={parentRef}
        onScroll={onScroll}
        className="h-full overflow-y-auto [overflow-anchor:none]"
        role="log"
        aria-live={live ? "polite" : "off"}
        aria-relevant="additions"
      >
        {/* The list runs on under the deck down to its line; scrolled to the end, the last row stays clear of it. */}
        <div className="relative mx-auto w-full max-w-[760px]" style={{ height: totalSize + 20 + LINE_Y }}>
          {virtualizer.getVirtualItems().map((v) => {
            const row = rows[v.index];
            return (
              <div
                key={v.key}
                data-index={v.index}
                ref={virtualizer.measureElement}
                className={`absolute top-0 left-0 w-full transition-colors duration-500 ${marked === row.key ? "bg-surface-2" : ""}`}
                style={{ transform: `translateY(${v.start}px)` }}
              >
                {row.kind === "header" ? (
                  <>
                    {header}
                    {!hasLines && <p className="py-10 text-center text-[12px] text-muted">{t("emptyTranscript")}</p>}
                  </>
                ) : (
                  <RowView row={row} startedAt={startedAt} twoSpeakers={twoSpeakers} editable={editable} />
                )}
              </div>
            );
          })}
        </div>
      </div>
      {live && !atBottom && (
        <button
          type="button"
          onClick={() => {
            setAtBottom(true);
            scrollToEnd(true);
          }}
          style={{ bottom: LINE_Y + 16 }}
          className="absolute left-1/2 inline-flex -translate-x-1/2 animate-pop items-center gap-1.5 rounded-full border border-line bg-surface px-3.5 py-1.5 text-[12px] font-semibold shadow-float transition-colors hover:bg-surface-2"
        >
          <ArrowDown size={13} />
          {t("jumpLatest")}
        </button>
      )}
    </div>
  );
}

/** Compact view: the last two lines, newest words visible. */
export function TranscriptTail() {
  const { t } = useTranslation();
  const { rows, twoSpeakers } = useRows(false);
  const tail = rows.filter((r) => r.kind === "partial" || (r.kind === "item" && r.item.kind !== "marker")).slice(-2);
  // Cut long lines at the start, so the newest words stay visible.
  const tailText = (text: string, className = "") => (
    <span className={`min-w-0 flex-1 truncate text-left [direction:rtl] ${className}`}>
      <span dir="ltr">{text}</span>
    </span>
  );
  if (tail.length === 0) {
    return <div className="truncate px-1 text-[12px] text-muted">{t("waitingForSpeech")}</div>;
  }
  const label = (source: SourceId) =>
    twoSpeakers && <span className={`shrink-0 text-[11px] font-bold ${source === "mic" ? "text-me" : "text-others"}`}>{source === "mic" ? t("me") : t("others")}</span>;
  return (
    <div className="min-w-0 px-1" aria-live="polite">
      {tail.map((r) => {
        if (r.kind === "partial") {
          return (
            <div key={r.key} className="flex items-baseline gap-2 text-[13px] leading-6">
              {label(r.partial.source)}
              {tailText(r.partial.text, "text-partial")}
            </div>
          );
        }
        if (r.kind !== "item") return null;
        const item = r.item;
        if (item.kind === "screenshot") {
          return (
            <div key={r.key} className="flex animate-fade-up items-center gap-1.5 truncate text-[12px] leading-6 text-muted">
              <Camera size={12} /> {t("screenshot")}
            </div>
          );
        }
        if (item.kind !== "segment") return null;
        return (
          <div key={r.key} className="flex animate-fade-up items-baseline gap-2 text-[13px] leading-6">
            {label(item.source)}
            {tailText(item.text, "selectable")}
          </div>
        );
      })}
    </div>
  );
}
