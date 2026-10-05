import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { advance, hearAppLevels, isQuiet, newTrack, STEP_MS, WAVE_STEPS, watchApp } from "./appLevels";

describe("app waves", () => {
  it("rise toward the reported height, a step every 100 ms", () => {
    const t = newTrack(0);
    t.level = 1;
    t.heardAt = 0;
    advance(t, 3 * STEP_MS + 40);
    expect(t.heights).toHaveLength(WAVE_STEPS + 1);
    expect(t.heights.slice(-3).map((h) => Math.round(h * 100))).toEqual([60, 84, 94]);
    expect(t.at).toBe(3 * STEP_MS);
  });

  it("fall flat once reports stop, then have nothing to show", () => {
    const t = newTrack(0);
    t.level = 1;
    t.heardAt = 0;
    advance(t, 1000);
    expect(isQuiet(t, 1000)).toBe(false);
    advance(t, 4000);
    expect(t.heights.every((h) => h === 0)).toBe(true);
    expect(isQuiet(t, 4000)).toBe(true);
  });

  it("are not quiet while a sound is reported, before its first step", () => {
    const t = newTrack(0);
    t.level = 0.5;
    t.heardAt = 50;
    expect(isQuiet(t, 60)).toBe(false);
  });

  it("tell the backend which apps to read, once per change", async () => {
    const call = vi.mocked(invoke);
    call.mockClear();
    const a = watchApp(11, () => {});
    const b = watchApp(22, () => {});
    const c = watchApp(11, () => {});
    await Promise.resolve();
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenLastCalledWith("watch_app_levels", { rootPids: [11, 22] });
    // Another wave still shows 11.
    a.stop();
    await Promise.resolve();
    expect(call).toHaveBeenCalledTimes(1);
    c.stop();
    b.stop();
    await Promise.resolve();
    expect(call).toHaveBeenCalledTimes(2);
    expect(call).toHaveBeenLastCalledWith("watch_app_levels", { rootPids: [] });
  });

  it("wake a wave when its app makes a sound", () => {
    const onSound = vi.fn();
    const w = watchApp(33, onSound);
    hearAppLevels({ levels: [{ rootPid: 33, dbfs: -60 }] }, 0);
    expect(onSound).not.toHaveBeenCalled();
    hearAppLevels({ levels: [{ rootPid: 33, dbfs: -12 }] }, 100);
    expect(onSound).toHaveBeenCalledTimes(1);
    expect(w.track.level).toBeGreaterThan(0.5);
    w.stop();
  });
});
