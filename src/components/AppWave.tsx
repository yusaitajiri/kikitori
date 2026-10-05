import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { liveLine, type Geometry } from "../lib/line";
import { advance, isQuiet, STEP_MS, WAVE_STEPS, watchApp } from "../store/appLevels";

/**
 * An app's sound beside its name, while it plays: its last two seconds as a small red line on its
 * baseline, the newest at the right end, from the app's own session meter. It steps ten times a
 * second instead of gliding every frame like the recording's line: redrawn at the screen's rate
 * it cost about 40% of a core on a 165 Hz screen, stepping about 4% (measured 2026-10-05). In
 * silence it fades out and nothing runs. It keeps its place, so the name never shifts.
 */
export function AppWave({ pid, width = 30, height = 14 }: { pid: number | undefined; width?: number; height?: number }) {
  const { t } = useTranslation();
  const svg = useRef<SVGSVGElement>(null);
  const path = useRef<SVGPathElement>(null);

  useEffect(() => {
    if (!pid) return;
    const g: Geometry = { base: height - 1.5, up: height - 3, down: 0, step: width / (WAVE_STEPS - 1), startX: 0, endPad: 0 };
    let shown = false;
    let timer: number | undefined;
    const draw = () => {
      const now = performance.now();
      const track = advance(watch.track, now);
      const quiet = isQuiet(track, now);
      if (shown === quiet) {
        shown = !quiet;
        svg.current?.toggleAttribute("data-on", shown);
        svg.current?.setAttribute("aria-hidden", shown ? "false" : "true");
      }
      if (quiet) {
        window.clearInterval(timer);
        timer = undefined;
        return;
      }
      path.current?.setAttribute("d", liveLine(track.heights.map((o) => ({ o, m: 0 })), 0, 0, width, g, false).o);
    };
    const start = () => {
      if (timer !== undefined) return;
      timer = window.setInterval(draw, STEP_MS);
      draw();
    };
    const watch = watchApp(pid, start);
    // The same app may already be playing in another wave (the list opening under the field).
    start();
    return () => {
      watch.stop();
      window.clearInterval(timer);
    };
  }, [pid, width, height]);

  return (
    <svg ref={svg} viewBox={`0 0 ${width} ${height}`} width={width} height={height} className="kk-app-wave shrink-0" role="img" aria-label={t("playingNow")} aria-hidden>
      <line x1={0} x2={width} y1={height - 1.5} y2={height - 1.5} className="kk-line-rest" />
      <path ref={path} />
    </svg>
  );
}
