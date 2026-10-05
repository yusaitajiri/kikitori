// One requestAnimationFrame loop for everything that moves. Each animation says whether it still
// needs frames, so the loop stops when nothing moves and an idle window costs no CPU. It draws at
// most about 60 times a second, and an animation can ask for fewer (`fps`): a faster screen asks
// for frames more often (165 a second on a 165 Hz screen), and drawing the recording's line on
// every one cost half a core.

export type Animated = {
  frame: (dt: number) => boolean;
  /** At most this many frames a second; 60 when unset. */
  fps?: number;
};

/** A frame this much sooner than its interval still counts, so a screen's own jitter never drops one. */
const SLACK_MS = 4;
const MIN_FRAME_MS = 1000 / 60 - SLACK_MS;

/** Each running animation, and when it last drew. */
const active = new Map<Animated, number>();
let raf = 0;
let last = 0;

export function animate(a: Animated) {
  if (!active.has(a)) active.set(a, performance.now());
  if (!raf && typeof requestAnimationFrame === "function") {
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }
}

export function stopAnimating(a: Animated) {
  active.delete(a);
}

function tick(now: number) {
  if (now - last >= MIN_FRAME_MS) {
    last = now;
    for (const [a, at] of [...active]) {
      // Skip one stopped by another's frame, and one that drew too recently for its own rate.
      if (!active.has(a) || (a.fps && now - at < 1000 / a.fps - SLACK_MS)) continue;
      active.set(a, now);
      // Long gaps (a hidden window) count as one short step instead of a jump.
      const dt = Math.min(0.05, Math.max(0, (now - at) / 1000));
      let keep = false;
      try {
        keep = a.frame(dt);
      } catch (e) {
        console.error(e);
      }
      if (!keep) active.delete(a);
    }
  }
  raf = active.size ? requestAnimationFrame(tick) : 0;
}

const reducedQuery = typeof window !== "undefined" ? window.matchMedia?.("(prefers-reduced-motion: reduce)") : undefined;

/** Windows' "Animation effects" off: waves stand still and morphs jump to their end. */
export function reducedMotion(): boolean {
  return !!reducedQuery?.matches;
}
