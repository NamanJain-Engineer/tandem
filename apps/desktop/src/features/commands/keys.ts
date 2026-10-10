// Shortcut strings from the registry ("Mod+Shift+D") as matchers and as Kbd
// caps (F-SH-09, UI_SPEC §5). Keys match on `KeyboardEvent.code`, so
// Shift+[ is still "[" and layouts don't move the bracket keys.

export type Binding = { mod: boolean; ctrl: boolean; shift: boolean; alt: boolean; code: string };

const NAMED: Record<string, string> = {
  "[": "BracketLeft",
  "]": "BracketRight",
  "\\": "Backslash",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  Enter: "Enter",
  Tab: "Tab",
  Space: "Space",
  Escape: "Escape",
  ";": "Semicolon",
  "'": "Quote",
  "`": "Backquote",
  "-": "Minus",
  "=": "Equal",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Left: "ArrowLeft",
  Right: "ArrowRight",
  Up: "ArrowUp",
  Down: "ArrowDown",
  End: "End",
};

const NAME_OF_CODE: Record<string, string> = Object.fromEntries(Object.entries(NAMED).map(([name, code]) => [code, name]));

const ARROWS: Record<string, string> = { Left: "←", Right: "→", Up: "↑", Down: "↓" };

function codeFor(key: string): string {
  if (/^[A-Z]$/.test(key)) return `Key${key}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  if (/^F\d{1,2}$/.test(key)) return key;
  return NAMED[key] ?? key;
}

export function parseBinding(keys: string): Binding {
  const parts = keys.split("+");
  // "Mod++" isn't used; the last part is always the key.
  const key = parts[parts.length - 1] ?? "";
  const has = (m: string) => parts.slice(0, -1).includes(m);
  return { mod: has("Mod"), ctrl: has("Ctrl"), shift: has("Shift"), alt: has("Alt"), code: codeFor(key) };
}

type KeyLike = Pick<KeyboardEvent, "code" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">;

/** Whether an event is exactly this binding (no extra modifiers). */
export function matches(b: Binding, e: KeyLike, mac: boolean): boolean {
  if (e.code !== b.code) return false;
  const wantMeta = mac && b.mod;
  const wantCtrl = mac ? b.ctrl : b.mod || b.ctrl;
  return e.metaKey === wantMeta && e.ctrlKey === wantCtrl && e.shiftKey === b.shift && e.altKey === b.alt;
}

/** Caps for the Kbd component: `["Mod", "Alt", "←"]`. */
export function keysOf(keys: string): string[] {
  return keys.split("+").map((k) => ARROWS[k] ?? k);
}

/** The registry's name for a physical key ("KeyR" → "R", "ArrowLeft" →
 * "Left"), or null for keys a shortcut can't use. */
export function keyNameOf(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1] ?? null;
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1] ?? null;
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return NAME_OF_CODE[code] ?? null;
}

/** The Windows/Linux terminal intercept list (UI_SPEC §5), as the core has
 * it (`tandem_core::commands::terminal_may_intercept`). */
export function terminalMayIntercept(keys: string): boolean {
  const parts = keys.split("+");
  const key = parts[parts.length - 1] ?? "";
  const has = (m: string) => parts.slice(0, -1).includes(m);
  const ctrl = has("Mod") || has("Ctrl");
  return key === "F6" || (ctrl && (has("Shift") || has("Alt"))) || (ctrl && parts.length === 2 && ["K", ",", "1", "2", "3", "Tab"].includes(key));
}

/** Whether two shortcuts are the same keys. Outside macOS, Mod is Ctrl. */
export function sameShortcut(a: string, b: string, mac: boolean): boolean {
  const x = parseBinding(a);
  const y = parseBinding(b);
  const ctrl = (k: Binding) => (mac ? k.ctrl : k.ctrl || k.mod);
  return x.code === y.code && x.shift === y.shift && x.alt === y.alt && ctrl(x) === ctrl(y) && (!mac || x.mod === y.mod);
}

/** Combos the system or text editing owns, by platform (F-SH-09 AC8). */
const EDITING = ["Mod+C", "Mod+V", "Mod+X", "Mod+Z", "Mod+Shift+Z", "Mod+A"];
const RESERVED: Record<string, string[]> = {
  macos: [
    ...EDITING,
    "Mod+Q",
    "Mod+H",
    "Mod+Alt+H",
    "Mod+M",
    "Mod+Tab",
    "Mod+Shift+Tab",
    "Mod+`",
    "Mod+Space",
    "Ctrl+Space",
    "Mod+Ctrl+Space",
    "Mod+Ctrl+Q",
    "Mod+Alt+Escape",
    "Mod+Shift+3",
    "Mod+Shift+4",
    "Mod+Shift+5",
    "Ctrl+Up",
    "Ctrl+Down",
    "Ctrl+Left",
    "Ctrl+Right",
  ],
  windows: [...EDITING, "Mod+Y", "Alt+F4", "Alt+Tab", "Alt+Shift+Tab", "Alt+Escape", "Alt+Space", "Mod+Escape", "Mod+Shift+Escape", "Mod+Alt+Delete"],
  linux: [...EDITING, "Mod+Y", "Alt+F4", "Alt+Tab", "Alt+Shift+Tab", "Alt+Escape", "Alt+Space", "Alt+F2", "Mod+Alt+Delete", "Mod+Alt+T", "Mod+Alt+L"],
};

export type Recorded = { ok: true; keys: string } | { ok: false; reason: "system" | "modifier" | "key" };

/** A pressed combo in the registry's form, or why it can't be a shortcut. */
export function recordShortcut(
  e: { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean },
  platform: string,
): Recorded {
  const mac = platform === "macos";
  // The Windows and Super keys belong to the system.
  if (!mac && e.metaKey) return { ok: false, reason: "system" };
  const key = keyNameOf(e.code);
  if (!key) return { ok: false, reason: "key" };
  const mods = [
    (mac ? e.metaKey : e.ctrlKey) && "Mod",
    mac && e.ctrlKey && "Ctrl",
    e.altKey && "Alt",
    e.shiftKey && "Shift",
  ].filter((m): m is string => !!m);
  const keys = [...mods, key].join("+");
  const fkey = /^F\d+$/.test(key);
  if (!fkey && !mods.some((m) => m !== "Shift")) return { ok: false, reason: "modifier" };
  if ((RESERVED[platform] ?? RESERVED.linux ?? []).some((r) => sameShortcut(r, keys, mac))) return { ok: false, reason: "system" };
  return { ok: true, keys };
}

