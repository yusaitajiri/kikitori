import { describe, expect, it } from "vitest";
import { freshColor, PROJECT_COLORS, projectEntries } from "./projects";

const p = (id: string, color: string) => ({ id, name: id, color });

describe("projects", () => {
  it("gives a new project a colour no other project uses, grey last", () => {
    expect(freshColor([])).toBe("blue");
    expect(freshColor([p("a", "blue"), p("b", "teal")])).toBe("green");
    expect(freshColor(PROJECT_COLORS.filter((c) => c !== "slate").map((c) => p(c, c)))).toBe("slate");
  });

  it("checks the session's project in the menu, or 「プロジェクトなし」 when it names none", () => {
    const t = (k: string) => k;
    const checked = (current?: string) =>
      projectEntries(t, [p("a", "blue"), p("b", "teal")], current, () => {}, () => {})
        .filter((e) => e !== "separator" && "checked" in e && e.checked)
        .map((e) => (e === "separator" ? "" : e.text));
    expect(checked("b")).toEqual(["b"]);
    expect(checked(undefined)).toEqual(["noProject"]);
    // A deleted project's ID.
    expect(checked("gone")).toEqual(["noProject"]);
  });
});
