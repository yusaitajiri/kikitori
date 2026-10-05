import { describe, expect, it } from "vitest";
import { backOf, placeOf } from "./nav";

describe("placeOf", () => {
  it("puts each view under its place", () => {
    expect(placeOf("main")).toBe("main");
    expect(placeOf("history")).toBe("history");
    expect(placeOf("session")).toBe("history");
    expect(placeOf("settings")).toBe("settings");
    expect(placeOf("models")).toBe("settings");
  });
});

describe("backOf", () => {
  it("leads from a past session or a settings section back to its list", () => {
    expect(backOf("session", "index")).toBe("history");
    expect(backOf("settings", "audio")).toBe("settings");
  });

  it("has no back arrow on the places themselves", () => {
    expect(backOf("main", "audio")).toBeUndefined();
    expect(backOf("history", "index")).toBeUndefined();
    expect(backOf("settings", "index")).toBeUndefined();
  });
});
