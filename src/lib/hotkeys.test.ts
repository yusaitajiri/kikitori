import { describe, expect, it } from "vitest";
import { acceleratorFrom } from "./hotkeys";

const key = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) => ({
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  code,
  ...mods,
});

describe("acceleratorFrom", () => {
  it("builds Ctrl+Alt+R", () => {
    expect(acceleratorFrom(key("KeyR", { ctrlKey: true, altKey: true }))).toBe("Ctrl+Alt+R");
  });
  it("accepts digits and function keys", () => {
    expect(acceleratorFrom(key("Digit5", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+5");
    expect(acceleratorFrom(key("F9"))).toBe("F9");
  });
  it("rejects bare letters, Shift-only and modifier-only presses", () => {
    expect(acceleratorFrom(key("KeyR"))).toBeNull();
    expect(acceleratorFrom(key("KeyR", { shiftKey: true }))).toBeNull();
    expect(acceleratorFrom(key("ControlLeft", { ctrlKey: true }))).toBeNull();
  });
});
