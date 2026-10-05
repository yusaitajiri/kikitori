// Hotkey recording for Settings.

/** Turns a keydown into an accelerator such as `Ctrl+Alt+R`. */
export function acceleratorFrom(e: Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "code">): string | null {
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (/^F\d{1,2}$/.test(e.code)) key = e.code;
  else if (["Space", "Insert", "Home", "End", "PageUp", "PageDown", "Pause"].includes(e.code)) key = e.code;
  if (!key) return null;
  // A bare letter would fire while typing anywhere; require Ctrl or Alt (F-keys may stand alone).
  if (!/^F\d/.test(key) && !e.ctrlKey && !e.altKey) return null;
  return [...mods, key].join("+");
}
