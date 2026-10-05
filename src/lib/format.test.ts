import { describe, expect, it } from "vitest";
import en from "../i18n/en.json";
import ja from "../i18n/ja.json";
import { bytes, clockAt, dateTimeOf, dayName, dayOf, dayText, durationParts, durationText, hms, levelFraction, modelNameParts, sourcesText, timeOf, timerText } from "./format";

describe("format", () => {
  it("hms", () => {
    expect(hms(3_735_000)).toBe("01:02:15");
    expect(hms(59_999)).toBe("00:00:59");
  });

  it("clock uses the session's own offset", () => {
    expect(clockAt("2026-10-02T15:13:05+09:00", 0)).toBe("15:13:05");
    expect(clockAt("2026-10-02T15:13:05+09:00", 172_340)).toBe("15:15:57");
    expect(clockAt("2026-10-02T23:59:59-05:00", 2000)).toBe("00:00:01");
  });

  it("dateTimeOf", () => {
    expect(dateTimeOf("2026-10-02T15:13:05+09:00")).toBe("2026-10-02 15:13");
  });

  it("bytes", () => {
    expect(bytes(574_041_195)).toBe("574 MB");
    expect(bytes(1_081_140_203)).toBe("1.08 GB");
    expect(bytes(59_707_625)).toBe("59.7 MB");
  });

  it("levelFraction maps -60..0 dBFS", () => {
    expect(levelFraction(-60)).toBe(0);
    expect(levelFraction(-30)).toBe(0.5);
    expect(levelFraction(0)).toBe(1);
    expect(levelFraction(undefined)).toBe(0);
  });

  it("timeOf", () => {
    expect(timeOf("2026-10-02T15:13:05+09:00")).toBe("15:13");
  });

  it("durationText", () => {
    const t = (k: string, p?: Record<string, unknown>) => `${k}:${JSON.stringify(p)}`;
    expect(durationText(45_000, t)).toBe('durS:{"s":45}');
    expect(durationText(32 * 60_000 + 10_000, t)).toBe('durM:{"m":32}');
    expect(durationText(72 * 60_000, t)).toBe('durHm:{"h":1,"m":12}');
  });

  it("timerText", () => {
    expect(timerText(736_000)).toBe("12:16");
    expect(timerText(3_723_000)).toBe("1:02:03");
  });

  it("durationParts", () => {
    expect(durationParts(45_000)).toEqual([[45, "s"]]);
    expect(durationParts(12 * 60_000)).toEqual([[12, "m"]]);
    expect(durationParts(90 * 60_000)).toEqual([[1, "h"], [30, "m"]]);
    expect(durationParts(120 * 60_000)).toEqual([[2, "h"]]);
  });

  it("days", () => {
    const now = new Date(2026, 9, 3, 1, 0); // 2026-10-03 01:00 local
    expect(dayOf("2026-10-03T00:25:00+09:00")).toBe("2026-10-03");
    expect(dayName("2026-10-03", now)).toBe("today");
    expect(dayName("2026-10-02", now)).toBe("yesterday");
    expect(dayName("2026-09-30", now)).toBeNull();
    expect(dayText("2026-10-03", "ja", 2026)).toBe("10月3日（土）");
    expect(dayText("2025-12-31", "ja", 2026)).toBe("2025年12月31日（水）");
    expect(dayText("2026-10-03", "en", 2026)).toBe("Sat, Oct 3");
  });

  it("modelNameParts", () => {
    expect(modelNameParts("標準 (large-v3-turbo q5_0)")).toEqual({ short: "標準", tech: "large-v3-turbo q5_0" });
    expect(modelNameParts("custom")).toEqual({ short: "custom", tech: "" });
  });

  it("sourcesText names what is recorded in the UI language", () => {
    const jaT = (k: string) => ja[k as keyof typeof ja];
    const enT = (k: string) => en[k as keyof typeof en];
    expect(sourcesText({ ids: ["app", "mic"], appName: "Zoom" }, jaT)).toBe("Zoom + マイク");
    expect(sourcesText({ ids: ["app", "mic"], appName: "Zoom" }, enT)).toBe("Zoom + Mic");
    expect(sourcesText({ ids: ["system"] }, jaT)).toBe("システム全体");
    expect(sourcesText({ ids: ["system", "mic"] }, enT)).toBe("All system audio + Mic");
    expect(sourcesText(undefined, enT)).toBe("");
  });
});
