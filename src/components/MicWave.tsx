import { useEffect, useRef } from "react";
import { animate, reducedMotion, stopAnimating } from "../lib/frameLoop";
import { heightOf, liveLine, settle, type Geometry, type Sample } from "../lib/line";

const W = 200;
const H = 36;
const G: Geometry = { base: 26, up: 20, down: 0, step: 1.6, startX: 6, endPad: 8 };

/** The mic test as the recording line: while it listens, the pen draws your voice. */
export function MicWave({ dbfs, active, label }: { dbfs: number | undefined; active: boolean; label: string }) {
  const path = useRef<SVGPathElement>(null);
  const pen = useRef<SVGCircleElement>(null);
  const ahead = useRef<SVGPathElement>(null);
  const level = useRef(dbfs);

  useEffect(() => {
    level.current = dbfs;
  }, [dbfs]);

  useEffect(() => {
    if (!active) return;
    const samples: Sample[] = [];
    let height = 0;
    let at = performance.now();
    const id = setInterval(() => {
      height = settle(height, heightOf(level.current));
      samples.push({ o: height, m: 0 });
      if (samples.length > 400) samples.shift();
      at = performance.now();
    }, 100);
    const anim = {
      frame() {
        const frac = reducedMotion() ? 0 : Math.min(1, (performance.now() - at) / 100);
        const l = liveLine(samples, frac, 0, W, G, false);
        path.current?.setAttribute("d", l.o);
        pen.current?.setAttribute("cx", String(l.pen[0]));
        pen.current?.setAttribute("cy", String(l.pen[1]));
        ahead.current?.setAttribute("d", l.ahead);
        return true;
      },
    };
    animate(anim);
    return () => {
      clearInterval(id);
      stopAnimating(anim);
    };
  }, [active]);

  return (
    <svg viewBox={`0 0 ${W} ${H}`} width={W} height={H} className="max-w-full shrink overflow-visible" role="img" aria-label={label}>
      <path ref={ahead} d={`M${G.startX} ${G.base}H${W}`} className="kk-line-ahead" />
      <path ref={path} d="" className="kk-line-o" />
      <circle ref={pen} cx={G.startX} cy={G.base} r={active ? 3.5 : 5} className="kk-line-pen" />
    </svg>
  );
}
