import { beforeEach, describe, expect, it, vi } from "vitest";

type Made = { kind: string; opts: Record<string, unknown>; close: ReturnType<typeof vi.fn> };
const made: Made[] = [];

vi.mock("@tauri-apps/api/menu", () => {
  const factory = (kind: string) => ({
    new: vi.fn(async (opts: Record<string, unknown>) => {
      const item: Made & { popup?: () => Promise<void> } = { kind, opts, close: vi.fn(async () => {}) };
      if (kind === "Menu") item.popup = vi.fn(async () => {});
      made.push(item);
      return item;
    }),
  });
  return {
    Menu: factory("Menu"),
    MenuItem: factory("MenuItem"),
    Submenu: factory("Submenu"),
    PredefinedMenuItem: factory("PredefinedMenuItem"),
  };
});

const { popupMenu } = await import("./popupMenu");

describe("popupMenu", () => {
  beforeEach(() => {
    made.length = 0;
  });

  it("creates every item as its own resource, keeping order and actions", async () => {
    const history = vi.fn();
    const pdf = vi.fn();
    await popupMenu([
      { text: "Copy", enabled: false, action: () => {} },
      { text: "Export", items: [{ text: "PDF", action: pdf }] },
      "separator",
      { text: "History", action: history },
    ]);
    const menu = made.find((m) => m.kind === "Menu")!;
    const items = menu.opts.items as Made[];
    expect(items.map((i) => i.kind)).toEqual(["MenuItem", "Submenu", "PredefinedMenuItem", "MenuItem"]);
    expect(items[0].opts.enabled).toBe(false);
    expect(items[3].opts.action).toBe(history);
    const sub = items[1].opts.items as Made[];
    expect(sub[0].kind).toBe("MenuItem");
    expect(sub[0].opts.action).toBe(pdf);
  });

  it("closes the previous menu's resources when the next one opens", async () => {
    await popupMenu([{ text: "A", action: () => {} }]);
    const first = [...made];
    expect(first.every((m) => m.close.mock.calls.length === 0)).toBe(true);
    await popupMenu([{ text: "B", action: () => {} }]);
    expect(first.every((m) => m.close.mock.calls.length === 1)).toBe(true);
  });
});
