import { afterEach, describe, expect, it, vi } from "vitest";

/** Frames an animation (asking for at most `fps`) draws in one second of a screen refreshing `hz` times a second. */
async function framesAt(hz: number, fps?: number): Promise<number> {
  vi.resetModules();
  const { animate } = await import("./frameLoop");
  const queue: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => queue.push(cb));
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frames = 0;
  animate({
    fps,
    frame: () => {
      frames++;
      return true;
    },
  });
  for (let i = 1; i <= hz; i++) {
    now = (i * 1000) / hz;
    queue.shift()?.(now);
  }
  return frames;
}

describe("frame loop", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("draws on every third refresh of a 165 Hz screen", async () => {
    expect(await framesAt(165)).toBe(55);
  });

  it("draws every frame of a 60 Hz screen", async () => {
    expect(await framesAt(60)).toBe(60);
  });

  it("draws half as often for an animation that asks for 30", async () => {
    expect(await framesAt(165, 30)).toBe(27);
    expect(await framesAt(60, 30)).toBe(30);
  });
});
