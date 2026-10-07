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
  Left: "ArrowLeft",
  Right: "ArrowRight",
  Up: "ArrowUp",
  Down: "ArrowDown",
  End: "End",
};

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
