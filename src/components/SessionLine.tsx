import { useMemo } from "react";
import { mapLine, type Shape } from "../lib/line";

const W = 360;
const H = 30;
const BASE = 15;

/** A past session's line, still and small: 相手 above in red, 自分 below in ink, dots on the baseline for screenshots. */
export function SessionLine({ shape, two, className = "" }: { shape: Shape; two: boolean; className?: string }) {
  const l = useMemo(() => mapLine(shape, W, 2, BASE, 10, 8, two), [shape, two]);
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className={`kk-session-line block h-[30px] w-full overflow-visible ${className}`} aria-hidden>
      <line x1={l.x0} x2={l.x1} y1={BASE} y2={BASE} className="kk-line-rest" vectorEffect="non-scaling-stroke" />
      {two && <path d={l.m} className="kk-sl-m" vectorEffect="non-scaling-stroke" />}
      <path d={l.o} className="kk-sl-o" vectorEffect="non-scaling-stroke" />
      {l.ticks.map((x, i) => (
        <line key={i} x1={x} x2={x} y1={BASE} y2={BASE} className="kk-line-shot kk-sl-shot" vectorEffect="non-scaling-stroke" />
      ))}
    </svg>
  );
}
