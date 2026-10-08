import { Camera, Copy, Ellipsis, FileOutput, Pause, Play, Scissors, Square } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { useElapsed } from "../hooks/useElapsed";
import { commands } from "../ipc/commands";
import type { SourceId } from "../ipc/types";
import { addCut, copy, exportAs, newRecording, openFolder, pauseOrResume, reportError, takeScreenshot, toggleRecording } from "../lib/actions";
import { clockAt, timerText } from "../lib/format";
import { animate, reducedMotion, stopAnimating, type Animated } from "../lib/frameLoop";
import { blobPath, liveLine, mapLine, sessionShape, type Shape } from "../lib/line";
import { popupMenu, type MenuEntry } from "../lib/popupMenu";
import { lineHistory } from "../store/line";
import { useRecording } from "../store/recording";
import { useSettings } from "../store/settings";
import { useSource } from "../store/source";
import { useTranscriptStore } from "../store/transcript";
import { useUi } from "../store/ui";
import { ModelNote } from "./ModelNote";

type DeckState = "start" | "recording" | "paused" | "finishing" | "review";

/**
 * How far below the expanded deck's top its line lies while recording and after. The transcript
 * runs on under the deck down to it, so only the tint under the line covers the text.
 */
export const LINE_Y = 30;

/**
 * In px: the deck's height while recording; the baseline then and, in the compact window, before
 * (the expanded window centres its start dot in the free space instead, on two wavy rings and
 * with no line until it starts); how far each side reaches; one step's width; where the pen starts
 * (in line with the buttons under it) and where it stops; where the compact start dot waits; the
 * start dot's, its rings' and the pen's radius; the whole session's height. The line runs from
 * edge to edge. Last, how many times a second the line may be drawn: the compact window, which
 * stays on screen through whole meetings, half as often.
 */
const SIZES = {
  expanded: { height: 124, base: LINE_Y, startBase: 30, centred: true, up: 20, down: 15, step: 1.6, startX: 14, endPad: 44, dotX: 14, dot: 36, rings: [48, 61], pen: 4.5, mapAmp: 0.62, fps: 60 },
  compact: { height: 66, base: 12, startBase: 30, centred: false, up: 10, down: 8, step: 1.4, startX: 12, endPad: 34, dotX: 28, dot: 11, rings: [0, 0], pen: 3.5, mapAmp: 0.75, fps: 30 },
} as const;
type Size = (typeof SIZES)[keyof typeof SIZES];

/**
 * Where the expanded start dot rests, as a share of the deck's height above the model's line at
 * the bottom (`START_FOOT` px): a little above the middle, at the optical centre, because a shape
 * at the exact middle looks low. The CSS for the dot's target and the hint under it uses the same
 * numbers (`--kk-rest`).
 */
const START_FOOT = 30;
const START_LIFT = 0.45;

const NS = "http://www.w3.org/2000/svg";
const r1 = (v: number) => Math.round(v * 10) / 10;
const easeInOut = (t: number) => (t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2);

type Els = {
  tint: SVGPathElement;
  restLead: SVGLineElement;
  rest: SVGLineElement;
  ring2: SVGPathElement;
  ring1: SVGPathElement;
  live: SVGGElement;
  fill: SVGPathElement;
  lead: SVGLineElement;
  ahead: SVGPathElement;
  gaps: SVGGElement;
  m: SVGPathElement;
  o: SVGPathElement;
  pending: SVGPathElement;
  ticks: SVGGElement;
  map: SVGGElement;
  mapFill: SVGPathElement;
  mapM: SVGPathElement;
  mapO: SVGPathElement;
  mapTicks: SVGGElement;
  pen: SVGCircleElement;
  label: SVGTextElement;
};

function build(svg: SVGSVGElement): Els {
  const el = <K extends keyof SVGElementTagNameMap>(tag: K, cls: string, parent: Element = svg) => {
    const e = document.createElementNS(NS, tag);
    if (cls) e.setAttribute("class", cls);
    if (tag === "path" || tag === "line") e.setAttribute("vector-effect", "non-scaling-stroke");
    parent.appendChild(e);
    return e as SVGElementTagNameMap[K];
  };
  const tint = el("path", "kk-line-tint");
  const restLead = el("line", "kk-line-ahead");
  const rest = el("line", "kk-line-ahead");
  const ring2 = el("path", "kk-ring-2");
  const ring1 = el("path", "kk-ring-1");
  const live = el("g", "");
  const map = el("g", "");
  const els = {
    tint,
    restLead,
    rest,
    ring2,
    ring1,
    live,
    fill: el("path", "kk-line-tint", live),
    lead: el("line", "kk-line-ahead", live),
    ahead: el("path", "kk-line-ahead", live),
    gaps: el("g", "", live),
    m: el("path", "kk-line-m", live),
    o: el("path", "kk-line-o", live),
    pending: el("path", "kk-line-pending", live),
    ticks: el("g", "", live),
    map,
    mapFill: el("path", "kk-line-tint", map),
    mapM: el("path", "kk-line-m", map),
    mapO: el("path", "kk-line-o", map),
    mapTicks: el("g", "", map),
    pen: el("circle", "kk-line-pen"),
    label: el("text", "kk-line-label"),
  };
  els.label.setAttribute("text-anchor", "middle");
  els.label.setAttribute("dominant-baseline", "central");
  return els;
}

function setLine(e: SVGLineElement, x1: number, x2: number, y1: number, y2 = y1) {
  e.setAttribute("x1", String(r1(x1)));
  e.setAttribute("x2", String(r1(x2)));
  e.setAttribute("y1", String(r1(y1)));
  e.setAttribute("y2", String(r1(y2)));
}

/** A short line: x1, x2, y1, y2. */
type Seg = [number, number, number, number];

/**
 * Replaces a group's short lines (pauses, and screenshots: a line of no length with round ends,
 * which stays a round dot while the zoom stretches the group); there are only ever a handful.
 */
function setLines(g: SVGGElement, cls: string, lines: Seg[]) {
  while (g.childElementCount > lines.length) g.lastElementChild?.remove();
  lines.forEach(([x1, x2, y1, y2], i) => {
    let e = g.children[i] as SVGLineElement | undefined;
    if (!e) {
      e = document.createElementNS(NS, "line");
      e.setAttribute("vector-effect", "non-scaling-stroke");
      g.appendChild(e);
    }
    e.setAttribute("class", cls);
    setLine(e, x1, x2, y1, y2);
  });
}

type Tween = { t: number; ms: number; set: (p: number) => void; done?: () => void };

/** Draws the line: the start dot on an empty line, the pen drawing the levels, or a whole session. */
class DeckLine implements Animated {
  mode: "dot" | "live" | "map" = "dot";
  dotR: number;
  penR: number;
  /** 0 the line and dot as they wait before a recording … 1 where they are while recording. */
  move = 0;
  /** The pointer on the start dot, 0..1: it swells a little and its rings stir. */
  lift = 0;
  /** How far the rings have stirred (seconds of stirring). */
  private stir = 0;
  /** 0 the live line … 1 the whole session: the zoom out when a session is saved. */
  k = 0;
  /** The whole session's heights: 1 at rest, 0 flat (on its way back to the dot). */
  amp = 1;
  pending = 0;
  pendingFrom = 0;
  shape: Shape | null = null;
  two = true;
  recording = false;
  readonly fps: number;
  private tweens: Tween[] = [];
  private readonly svg: SVGSVGElement;
  private readonly els: Els;
  private readonly size: Size;

  constructor(svg: SVGSVGElement, els: Els, size: Size) {
    this.svg = svg;
    this.els = els;
    this.size = size;
    this.dotR = size.dot;
    this.penR = size.pen;
    this.fps = size.fps;
  }

  /** Runs `set` from 0 to 1 (eased) over `ms`; with Windows' animation effects off it jumps to the end. */
  tween(ms: number, set: (p: number) => void, done?: () => void) {
    if (reducedMotion()) {
      set(1);
      done?.();
      this.draw();
      return;
    }
    this.tweens.push({ t: 0, ms, set, done });
    animate(this);
  }

  stop() {
    this.tweens = [];
    stopAnimating(this);
  }

  setLabel(text: string) {
    this.els.label.textContent = text;
    this.draw();
  }

  frame(dt: number): boolean {
    if (this.mode === "dot" && this.lift > 0 && !reducedMotion()) this.stir += dt * this.lift;
    for (const tw of [...this.tweens]) {
      tw.t += dt * 1000;
      const p = Math.min(1, tw.t / tw.ms);
      tw.set(easeInOut(p));
      if (p >= 1) {
        this.tweens.splice(this.tweens.indexOf(tw), 1);
        tw.done?.();
      }
    }
    this.draw();
    return this.tweens.length > 0 || (this.mode === "live" && this.recording) || (this.mode === "dot" && this.lift > 0.001);
  }

  draw() {
    const w = this.svg.clientWidth;
    const h = this.svg.clientHeight;
    if (!w) return;
    const s = this.size;
    const e = this.els;
    // Before a recording the expanded window holds its dot in the middle of the free space, with
    // no line yet; recording, the line lies near the top of the deck.
    const restBase = s.centred ? (h - START_FOOT) * START_LIFT : s.startBase;
    const base = this.mode === "dot" ? restBase + (s.base - restBase) * this.move : s.base;
    e.tint.setAttribute("d", `M-2 ${r1(base)}H${w + 2}V${h + 2}H-2Z`);
    if (this.mode === "dot") {
      const restX = s.centred ? w / 2 : s.dotX;
      const x = restX + (s.startX - restX) * this.move;
      const r = Math.max(0, this.dotR + 3 * this.lift);
      // The line appears as the dot goes down to become its pen; the compact one waits at the
      // start of its line, which reaches back to the edge as the dot moves to where the pen starts.
      setLine(e.rest, x, w, base);
      e.rest.setAttribute("opacity", s.centred ? this.move.toFixed(3) : "1");
      setLine(e.restLead, 0, x, base);
      e.restLead.setAttribute("opacity", this.move.toFixed(3));
      // Two wavy rings, each bigger and paler, shrink with the dot and fade as it leaves.
      const scale = this.dotR / s.dot;
      const amp = 1 + 0.8 * this.lift;
      e.ring1.setAttribute("d", blobPath(x, base, s.rings[0] * scale, 1.8 * amp, 7, 0.6 + this.stir * 1.6));
      e.ring2.setAttribute("d", blobPath(x, base, s.rings[1] * scale, 2.4 * amp, 9, 2.1 - this.stir * 2.1));
      const ringsShown = Math.max(0, Math.min(1, 1 - this.move * 2.2)).toFixed(3);
      e.ring1.setAttribute("opacity", ringsShown);
      e.ring2.setAttribute("opacity", ringsShown);
      e.pen.setAttribute("cx", String(r1(x)));
      e.pen.setAttribute("cy", String(r1(base)));
      e.pen.setAttribute("r", String(r1(r)));
      // 開始 is written on the big dot; it goes as soon as the dot starts to shrink.
      e.label.setAttribute("x", String(r1(x)));
      e.label.setAttribute("y", String(r1(base)));
      e.label.setAttribute("opacity", s.centred ? Math.max(0, Math.min(1, (this.dotR - 0.7 * s.dot) / (0.3 * s.dot))).toFixed(3) : "0");
      e.live.setAttribute("opacity", "0");
      e.map.setAttribute("opacity", "0");
      return;
    }
    e.label.setAttribute("opacity", "0");
    e.ring1.setAttribute("d", "");
    e.ring2.setAttribute("d", "");
    e.rest.setAttribute("opacity", "0");
    e.restLead.setAttribute("opacity", "0");
    let ax: number = s.startX;
    if (this.mode === "live") {
      const frac = this.recording && !reducedMotion() ? Math.min(1, Math.max(0, (performance.now() - lineHistory.at) / 100)) : 0;
      const g = { base, up: s.up, down: s.down, step: s.step, startX: s.startX, endPad: s.endPad };
      const l = liveLine(lineHistory.samples, frac, this.pending, w, g, this.two);
      e.fill.setAttribute("d", l.fill);
      e.o.setAttribute("d", l.o);
      e.m.setAttribute("d", l.m);
      e.pending.setAttribute("d", l.pending);
      setLines(e.gaps, "kk-line-gap", l.gaps.map(([a, b]): Seg => [a, b, base, base]));
      setLines(e.ticks, "kk-line-shot", l.ticks.map((x): Seg => [x, x, base, base]));
      // Grey where nothing is written: before the pen started, and ahead of the line.
      setLine(e.lead, 0, l.x0, base);
      e.ahead.setAttribute("d", l.ahead);
      e.pen.setAttribute("cx", String(r1(l.pen[0])));
      e.pen.setAttribute("cy", String(r1(l.pen[1])));
      e.pen.setAttribute("r", String(r1(Math.max(0, this.penR))));
      ax = l.pen[0];
    } else {
      e.pen.setAttribute("r", "0");
    }
    if (this.shape && (this.k > 0 || this.mode === "map")) {
      const a = s.mapAmp * this.amp;
      const l = mapLine(this.shape, w, 0, base, s.up * a, s.down * a, this.two);
      e.mapFill.setAttribute("d", l.fill);
      e.mapO.setAttribute("d", l.o);
      e.mapM.setAttribute("d", l.m);
      setLines(e.mapTicks, "kk-line-shot", l.ticks.map((x): Seg => [x, x, base, base]));
    }
    // The zoom out: the live line shrinks towards the pen while the whole session shrinks into place.
    const k = this.mode === "map" ? 1 : this.k;
    const sl = 1 - 0.92 * k;
    const sm = 5 - 4 * k;
    e.live.setAttribute("transform", `translate(${r1(ax * (1 - sl))} 0) scale(${sl.toFixed(4)} 1)`);
    e.live.setAttribute("opacity", (1 - k).toFixed(3));
    e.map.setAttribute("transform", `translate(${r1(ax * (1 - sm))} 0) scale(${sm.toFixed(4)} 1)`);
    e.map.setAttribute("opacity", k.toFixed(3));
  }
}

/**
 * The bottom of the main window and of a past session: one line across the window. Before a
 * recording a red dot on two wavy rings waits in the middle of the free space (in the compact
 * window, at the start of an empty line). Pressing it sends the dot down to the bottom, where it
 * shrinks into a pen tip that draws the line as it moves: 相手 above the line in red, 自分 below
 * it in ink, over a red tint. When the session is saved the pen lifts and the line zooms out to
 * the whole session, which you can point at for the time and click to jump there; the finished
 * session's buttons sit under it.
 */
export function Deck({
  compact = false,
  phase,
  sessionId,
  folder,
  onNew = false,
}: {
  compact?: boolean;
  phase: "start" | "live" | "review";
  sessionId?: string;
  folder?: string;
  /** Shows 「新しい録音」 (the main window only). */
  onNew?: boolean;
}) {
  const { t } = useTranslation();
  const recState = useRecording((s) => s.state);
  const finishing = useRecording((s) => s.finishing);
  const state: DeckState =
    phase === "start" ? "start" : phase === "review" ? "review" : recState === "paused" ? "paused" : recState === "finishing" ? "finishing" : "recording";
  const store = useTranscriptStore();
  const items = store((s) => s.items);
  const durationMs = store((s) => s.durationMs);
  const startedAt = store((s) => s.startedAt);
  const sessionSources = store((s) => s.sources);
  const sound = store((s) => s.sound);
  const mode = useSource((s) => s.mode);
  const includeMic = useSource((s) => s.includeMic);
  // Until the session is loaded, the sources come from the picker.
  const ids: SourceId[] = sessionSources.length
    ? sessionSources.map((s) => s.id)
    : mode === "mic"
      ? ["mic"]
      : [mode === "app" ? "app" : "system", ...(includeMic ? (["mic"] as SourceId[]) : [])];
  const two = ids.includes("mic") && ids.some((id) => id !== "mic");
  const size = compact ? SIZES.compact : SIZES.expanded;
  // The expanded deck fills the free space before a recording, so its dot sits in the middle; once
  // there is a transcript, it reaches up under the deck to the line.
  const grows = state === "start" && size.centred;
  const under = !compact && state !== "start";
  let total = durationMs ?? 0;
  for (const i of items) if (i.kind === "segment") total = Math.max(total, i.tEndMs);

  const rootRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const lineRef = useRef<DeckLine | null>(null);
  const prev = useRef<DeckState | null>(null);
  const lastHeight = useRef(0);
  const latest = useRef({ items, durationMs, two, sound });
  latest.current = { items, durationMs, two, sound };
  const [scrub, setScrub] = useState<{ x: number; ms: number } | null>(null);

  useLayoutEffect(() => {
    const root = rootRef.current;
    const svg = svgRef.current;
    if (!root || !svg) return;
    const els = build(svg);
    const line = new DeckLine(svg, els, size);
    const st = prev.current ?? state;
    line.two = latest.current.two;
    line.recording = st === "recording";
    if (st !== "start") {
      line.move = 1;
      line.mode = st === "review" ? "map" : "live";
      if (st === "review") {
        line.k = 1;
        line.shape = sessionShape(latest.current.items, latest.current.durationMs, latest.current.sound);
      }
    }
    lineRef.current = line;
    lastHeight.current = root.getBoundingClientRect().height;
    line.draw();
    if (line.recording) animate(line);
    const ro = new ResizeObserver(() => {
      lastHeight.current = root.getBoundingClientRect().height;
      line.draw();
    });
    ro.observe(root);
    return () => {
      ro.disconnect();
      line.stop();
      lineRef.current = null;
      svg.replaceChildren();
    };
    // One line per size; it reads the state through `prev` and `latest`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [compact]);

  useEffect(() => {
    lineRef.current?.setLabel(t("start"));
  }, [t, compact]);

  // Moving between states: the dot becomes the pen, the line comes to rest, and back. The deck
  // has already been laid out at its new height (taller before a recording); it is held at the old
  // one and eased to the new one together with the line, before anything is painted.
  useLayoutEffect(() => {
    const line = lineRef.current;
    const root = rootRef.current;
    const from = prev.current;
    prev.current = state;
    if (!line || !root || from === state) return;
    line.recording = state === "recording";
    if (from === null) {
      line.draw();
      return;
    }
    const fromH = lastHeight.current;
    const toH = root.getBoundingClientRect().height;
    const resizes = Math.abs(toH - fromH) > 0.5;
    // While it moves, what waits under the dot stays hidden (`data-moving`; React never sets it).
    const hold = () => {
      if (!resizes) return;
      root.dataset.moving = "";
      root.style.flex = "none";
      root.style.height = `${fromH}px`;
    };
    const ease = (p: number) => {
      if (resizes) root.style.height = `${fromH + (toH - fromH) * p}px`;
    };
    const release = () => {
      delete root.dataset.moving;
      root.style.flex = "";
      root.style.height = "";
    };
    if (state === "start") {
      setScrub(null);
      if (from === "review") {
        // 「新しい録音」: the session's line flattens, rises to the middle and the dot grows back.
        hold();
        line.tween(300, (p) => (line.amp = 1 - p), () => {
          line.mode = "dot";
          line.k = 0;
          line.amp = 1;
          line.shape = null;
          line.move = 1;
          line.dotR = 0;
          line.lift = 0;
          line.tween(
            480,
            (p) => {
              ease(p);
              line.move = 1 - p;
              line.dotR = size.dot * p;
            },
            release,
          );
        });
      } else {
        line.stop();
        release();
        line.mode = "dot";
        line.k = 0;
        line.move = 0;
        line.dotR = size.dot;
        line.draw();
      }
      return;
    }
    if (from === "start") {
      // 開始: the line sinks to the bottom while the dot shrinks into the pen at its start.
      line.stop();
      line.lift = 0;
      hold();
      line.mode = "dot";
      line.tween(
        480,
        (p) => {
          ease(p);
          line.move = p;
          line.dotR = size.dot + (size.pen - size.dot) * p;
        },
        () => {
          release();
          line.mode = "live";
          line.move = 1;
          line.penR = size.pen;
          line.k = 0;
          animate(line);
        },
      );
      return;
    }
    if (state === "finishing") {
      line.pendingFrom = Math.min(40, lineHistory.samples.length);
      line.pending = line.pendingFrom;
      line.draw();
      return;
    }
    if (state === "review") {
      line.pending = 0;
      line.shape = sessionShape(latest.current.items, latest.current.durationMs, latest.current.sound);
      if (line.mode !== "live") {
        line.mode = "map";
        line.k = 1;
        line.draw();
        return;
      }
      line.tween(260, (p) => (line.penR = size.pen * (1 - p)), () =>
        line.tween(800, (p) => (line.k = p), () => {
          line.mode = "map";
          line.draw();
        }),
      );
      return;
    }
    // Paused or resumed.
    line.draw();
    if (state === "recording") animate(line);
  }, [state, size]);

  // While finishing, the unwritten tail fills in as the queue empties.
  useEffect(() => {
    const line = lineRef.current;
    if (!line || state !== "finishing") return;
    const left = !finishing ? 1 : finishing.total > 0 ? Math.max(0, (finishing.total - finishing.done) / finishing.total) : 0;
    line.pending = Math.round(line.pendingFrom * left);
    line.draw();
  }, [finishing, state]);

  // A finished session's line follows the session: its sound, or its transcript, and its screenshots.
  useEffect(() => {
    const line = lineRef.current;
    if (!line) return;
    line.two = two;
    if (state === "review") line.shape = sessionShape(items, durationMs, sound);
    if (line.mode !== "live" || !line.recording) line.draw();
  }, [items, durationMs, sound, two, state]);

  /** The start dot swells a little under the pointer. */
  const hover = (on: boolean) => {
    const line = lineRef.current;
    if (!line) return;
    if (line.mode !== "dot") {
      line.lift = 0;
      return;
    }
    const from = line.lift;
    line.tween(160, (p) => (line.lift = from + ((on ? 1 : 0) - from) * p));
  };

  const scrubAt = (clientX: number, clientY: number) => {
    const svg = svgRef.current;
    if (!svg || state !== "review" || total <= 0) return null;
    const b = svg.getBoundingClientRect();
    if (clientY - b.top > size.base + 18) return null;
    const u = Math.min(1, Math.max(0, (clientX - b.left) / b.width));
    return { x: u * b.width, ms: u * total };
  };

  const style = {
    "--kk-h": `${size.height}px`,
    "--kk-dx": `${size.dotX}px`,
    "--kk-sb": `${size.startBase}px`,
    "--kk-dot": `${size.dot}px`,
    "--kk-ring": `${size.rings[1]}px`,
    "--kk-b": `${size.base}px`,
    "--kk-rest": `calc((100% - ${START_FOOT}px) * ${START_LIFT})`,
  } as CSSProperties;
  return (
    <div
      ref={rootRef}
      data-deck={state}
      className={`kk-deck relative ${compact ? "kk-deck-compact" : ""} ${grows ? "kk-deck-grow" : ""} ${under ? "kk-deck-under" : ""}`}
      style={style}
      onPointerMove={(e) => setScrub(scrubAt(e.clientX, e.clientY))}
      onPointerLeave={() => setScrub(null)}
      onClick={(e) => {
        const at = scrubAt(e.clientX, e.clientY);
        if (at && !(e.target as HTMLElement).closest("button")) store.getState().seekTo(at.ms);
      }}
    >
      <svg ref={svgRef} className="kk-line pointer-events-none absolute inset-0 h-full w-full overflow-visible" aria-hidden />
      {/* Where the finished session's line takes the pointer; above it, the pointer reaches the text. */}
      {under && state === "review" && <div className="kk-scrub-zone" aria-hidden />}
      {state === "start" && <StartRow compact={compact} onHover={hover} />}
      {(state === "recording" || state === "paused") && <LiveRow compact={compact} paused={state === "paused"} />}
      {state === "finishing" && <FinishingRow />}
      {state === "review" && sessionId && <ReviewRow compact={compact} sessionId={sessionId} folder={folder} onNew={onNew} />}
      {state === "review" && !compact && startedAt && total > 0 && (
        <div className="kk-line-ends num" aria-hidden>
          <span>{clockAt(startedAt, 0).slice(0, 5)}</span>
          <span>{clockAt(startedAt, total).slice(0, 5)}</span>
        </div>
      )}
      {scrub && (
        <div className="kk-scrub" style={{ left: scrub.x }} aria-hidden>
          <span className="num">{startedAt ? clockAt(startedAt, scrub.ms) : timerText(scrub.ms)}</span>
        </div>
      )}
    </div>
  );
}

function HotkeyHint({ className }: { className: string }) {
  const { t } = useTranslation();
  const key = useSettings((s) => s.settings?.hotkeys.toggle);
  const taken = useSettings((s) => !!s.info?.hotkeyErrors.toggle);
  if (!key || taken) return null;
  return <span className={className}>{t("startHint", { key: key.replaceAll("+", " + ") })}</span>;
}

/**
 * The start dot's target (the dot itself is drawn by the line). In the expanded window 開始 is
 * written on the dot, a hotkey (when one is set) is under its rings and the model sits at the
 * bottom, so the dot alone is centred; in the compact one 開始 stands on the line beside the dot.
 */
function StartRow({ compact, onHover }: { compact: boolean; onHover: (on: boolean) => void }) {
  const { t } = useTranslation();
  const state = useRecording((s) => s.state);
  const info = useSettings((s) => s.info);
  const [starting, setStarting] = useState(false);
  const disabled = state !== "ready" || !info || starting;
  const start = async () => {
    setStarting(true);
    try {
      await toggleRecording();
    } finally {
      setStarting(false);
    }
  };
  return (
    <div className="absolute inset-0 animate-fade-in">
      <button
        type="button"
        className="kk-go"
        onClick={start}
        onPointerEnter={() => !disabled && onHover(true)}
        onPointerLeave={() => onHover(false)}
        disabled={disabled}
        aria-label={t("start")}
        data-starting={starting || undefined}
      />
      {compact ? (
        <>
          <span className="kk-go-label" aria-hidden>
            {t("start")}
          </span>
          <HotkeyHint className="kk-go-hint" />
        </>
      ) : (
        <>
          <div className="kk-go-under">
            <HotkeyHint className="" />
          </div>
          <div className="kk-go-foot">
            <ModelNote />
          </div>
        </>
      )}
    </div>
  );
}

/** The time in big numerals, then pause or resume, stop, screenshot and cut. */
function LiveRow({ compact, paused }: { compact: boolean; paused: boolean }) {
  const { t } = useTranslation();
  const elapsed = useElapsed();
  const lag = useRecording((s) => (s.lag ? Math.round(s.lag.lagMs / 1000) : 0));
  const note = paused ? t("paused") : lag >= 2 ? t("lag", { n: lag }) : "";
  const ctl = "kk-ctl grid shrink-0 place-items-center rounded-[11px] border border-line bg-surface text-fg transition-colors duration-150 hover:bg-surface-2 disabled:pointer-events-none disabled:opacity-40";
  return (
    <div className="kk-row flex items-center justify-between gap-3 animate-fade-in [animation-delay:300ms]">
      <span className="flex min-w-0 items-baseline gap-2.5">
        <span className={`num kk-timer ${paused ? "text-muted" : ""}`} aria-label={t("elapsed")}>
          {timerText(elapsed)}
        </span>
        {note && (
          <span className={`truncate text-[11.5px] ${lag > 30 && !paused ? "font-bold text-fg" : "font-semibold text-muted"}`} aria-live="polite">
            {note}
          </span>
        )}
      </span>
      <span className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          onClick={pauseOrResume}
          aria-pressed={paused}
          aria-label={paused ? t("resume") : t("pause")}
          title={paused ? t("resume") : t("pause")}
          className={`${ctl} ${paused ? "!border-fg !bg-fg !text-bg" : ""}`}
        >
          {paused ? <Play size={16} fill="currentColor" /> : <Pause size={16} fill="currentColor" />}
        </button>
        <button
          type="button"
          onClick={toggleRecording}
          aria-label={t("stop")}
          className={`inline-flex shrink-0 items-center gap-2 rounded-full bg-rec-strong font-bold text-rec-fg transition-colors duration-150 hover:bg-rec-hover ${compact ? "h-9 px-4 text-[12.5px]" : "h-[42px] px-[18px] text-[13px]"}`}
        >
          <Square size={10} fill="currentColor" />
          {t("stop")}
        </button>
        <button type="button" onClick={takeScreenshot} disabled={paused} aria-label={t("screenshot")} title={t("screenshot")} className={ctl}>
          <Camera size={17} />
        </button>
        <button type="button" onClick={addCut} aria-label={t("cut")} title={t("cut")} className={ctl}>
          <Scissors size={16} />
        </button>
      </span>
    </div>
  );
}

function FinishingRow() {
  const { t } = useTranslation();
  const finishing = useRecording((s) => s.finishing);
  const left = finishing && finishing.total > 0 ? finishing.total - finishing.done : null;
  return (
    <div className="kk-row flex items-center justify-between gap-3 text-[12.5px] animate-fade-in" title={t("finishingDetail")}>
      <span className="min-w-0 truncate">
        <b className="font-semibold">{t("finishing")}</b>
        {left !== null && <span className="text-muted"> · {t("finishingLeft", { n: left })}</span>}
      </span>
      <button type="button" onClick={() => commands.cancelFinishing()} className="kk-link shrink-0">
        {t("stopFinishing")}
      </button>
    </div>
  );
}

/** Copy, 書き出し, more, and 「新しい録音」 with the red dot it started from. */
function ReviewRow({ compact, sessionId, folder, onNew }: { compact: boolean; sessionId: string; folder?: string; onNew: boolean }) {
  const { t } = useTranslation();
  const setExportFor = useUi((s) => s.setExportFor);
  const more = async () => {
    const entries: MenuEntry[] = [
      { text: t("copyForAgent"), action: () => void copy("agent", sessionId) },
      { text: t("copyMarkdown"), action: () => void copy("markdown", sessionId) },
      "separator",
      { text: t("openFolder"), enabled: !!folder, action: () => void openFolder(folder) },
    ];
    await popupMenu(entries).catch(reportError);
  };
  // 書き出し picks a format in a dialog; the compact window is too small for one, so a menu there.
  const exportMenu = async () => {
    const entries: MenuEntry[] = [
      { text: t("exportMarkdown"), action: () => void exportAs("markdown", sessionId) },
      { text: t("exportMarkdownZip"), action: () => void exportAs("markdown", sessionId, true) },
      { text: t("exportPdf"), action: () => void exportAs("pdf", sessionId) },
      { text: t("exportTypst"), action: () => void exportAs("typst", sessionId) },
    ];
    await popupMenu(entries).catch(reportError);
  };
  const act = `kk-act inline-flex shrink-0 items-center gap-2 rounded-[11px] border border-line bg-surface font-semibold whitespace-nowrap transition-colors duration-150 hover:bg-surface-2 ${compact ? "h-9 px-3 text-[12.5px]" : "h-10 px-3 text-[13px]"}`;
  return (
    <div className="kk-row flex items-center gap-2 animate-fade-in [animation-delay:500ms]">
      <button type="button" className={act} onClick={() => copy("plain", sessionId)}>
        <Copy size={15} />
        {t("copy")}
      </button>
      {compact ? (
        <button type="button" className={`${act} !w-9 justify-center !px-0`} onClick={exportMenu} aria-haspopup="menu" aria-label={t("export")} title={t("export")}>
          <FileOutput size={16} />
        </button>
      ) : (
        <button type="button" className={act} onClick={() => setExportFor(sessionId)} aria-haspopup="dialog">
          <FileOutput size={15} />
          {t("export")}
        </button>
      )}
      <button type="button" className={`${act} !w-10 justify-center !px-0 ${compact ? "!w-9" : ""}`} onClick={more} aria-haspopup="menu" aria-label={t("moreActions")} title={t("moreActions")}>
        <Ellipsis size={17} />
      </button>
      <span className="min-w-1 flex-1" />
      {onNew && (
        <button type="button" className={act} onClick={newRecording}>
          <span className="size-[11px] shrink-0 rounded-full bg-rec" aria-hidden />
          {t("newRecording")}
        </button>
      )}
    </div>
  );
}
