// Native popup menus from plain item descriptions.
//
// Tauri 2.12 drops the Rust side of items written inline in `Menu.new({ items })` as soon as
// the menu is built, and that removes their click handlers: the menu shows, but picking an item
// does nothing. Items made with `MenuItem.new()` stay in Tauri's resource table until closed, so
// every item is made explicitly and closed when the next menu opens (any click is delivered by
// then).

import { Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";

export type MenuEntry =
  | { text: string; enabled?: boolean; action: () => void }
  | { text: string; enabled?: boolean; items: MenuEntry[] }
  | "separator";

type Closable = { close(): Promise<void> };

let live: Closable[] = [];

async function make(entry: MenuEntry, made: Closable[]): Promise<MenuItem | Submenu | PredefinedMenuItem> {
  let item: MenuItem | Submenu | PredefinedMenuItem;
  if (entry === "separator") {
    item = await PredefinedMenuItem.new({ item: "Separator" });
  } else if ("items" in entry) {
    const children = await Promise.all(entry.items.map((e) => make(e, made)));
    item = await Submenu.new({ text: entry.text, enabled: entry.enabled ?? true, items: children });
  } else {
    item = await MenuItem.new({ text: entry.text, enabled: entry.enabled ?? true, action: entry.action });
  }
  made.push(item);
  return item;
}

/** Shows a native menu at the cursor; resolves when it closes. */
export async function popupMenu(entries: MenuEntry[]) {
  const previous = live;
  live = [];
  await Promise.all(previous.map((r) => r.close().catch(() => {})));
  const items = await Promise.all(entries.map((e) => make(e, live)));
  const menu = await Menu.new({ items });
  live.push(menu);
  await menu.popup();
}
