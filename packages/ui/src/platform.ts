/** Platform helpers for shortcut labels (UI_SPEC §2.19). */
export function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = nav.userAgentData?.platform ?? navigator.platform ?? "";
  return /mac/i.test(platform) || /Mac OS X/.test(navigator.userAgent);
}

const MAC: Record<string, string> = { Mod: "⌘", Alt: "⌥", Shift: "⇧", Ctrl: "⌃", Enter: "↩", Backspace: "⌫" };
const OTHER: Record<string, string> = { Mod: "Ctrl", Enter: "Enter" };

/** Turns `["Mod", "Shift", "B"]` into the caps shown on this platform. */
export function shortcutCaps(keys: string[], mac = isMac()): string[] {
  const map = mac ? MAC : OTHER;
  return keys.map((k) => map[k] ?? k);
}

/** "⌘⇧B" on mac, "Ctrl+Shift+B" elsewhere (tooltips, aria-keyshortcuts text). */
export function shortcutLabel(keys: string[], mac = isMac()): string {
  const caps = shortcutCaps(keys, mac);
  return mac ? caps.join("") : caps.join("+");
}
