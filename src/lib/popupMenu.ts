// Native popup menus from plain item descriptions.
//
// Tauri 2.12 drops the Rust side of items written inline in `Menu.new({ items })` as soon as
// the menu is built, and that removes their click handlers: the menu shows, but picking an item
// does nothing. Items made with `MenuItem.new()` stay in Tauri's resource table until closed, so
// every item is made explicitly and closed when the next menu opens (any click is delivered by
// then).

import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";

/** An item with `checked` set shows a check mark when true (a choice among several). */
export type MenuEntry =
  | { text: string; enabled?: boolean; checked?: boolean; action: () => void }
  | { text: string; enabled?: boolean; items: MenuEntry[] }
  | "separator";

type Closable = { close(): Promise<void> };

let live: Closable[] = [];

/** Windows reads `&` in a menu item as the start of a keyboard shortcut; `&&` shows one `&`. */
export const menuText = (text: string) => text.replaceAll("&", "&&");

async function make(entry: MenuEntry, made: Closable[]): Promise<MenuItem | CheckMenuItem | Submenu | PredefinedMenuItem> {
  let item: MenuItem | CheckMenuItem | Submenu | PredefinedMenuItem;
  if (entry === "separator") {
    item = await PredefinedMenuItem.new({ item: "Separator" });
  } else if ("items" in entry) {
    const children = await Promise.all(entry.items.map((e) => make(e, made)));
    item = await Submenu.new({ text: entry.text, enabled: entry.enabled ?? true, items: children });
  } else if (entry.checked !== undefined) {
    item = await CheckMenuItem.new({ text: entry.text, enabled: entry.enabled ?? true, checked: entry.checked, action: entry.action });
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
