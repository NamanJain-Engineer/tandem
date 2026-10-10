import type { RecordedKeys } from "@tandem/ui";

/** A key's binding name from `KeyboardEvent.code` (the core's names, D-501). */
export function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  const named: Record<string, string> = {
    Space: "Space",
    Tab: "Tab",
    Enter: "Enter",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Pause: "Pause",
    ScrollLock: "ScrollLock",
    PrintScreen: "PrintScreen",
    ContextMenu: "Menu",
    Escape: "Esc",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Mouse4: "Mouse4",
    Mouse5: "Mouse5",
  };
  return named[code] ?? null;
}

/** What the recorder captured, as a binding string (`Ctrl+Alt+D`, `Ctrl+Alt`). */
export function bindingFrom(keys: RecordedKeys): string | null {
  const parts: string[] = [];
  if (keys.ctrlKey) parts.push("Ctrl");
  if (keys.altKey) parts.push("Alt");
  if (keys.shiftKey) parts.push("Shift");
  if (keys.metaKey) parts.push("Meta");
  if (keys.code) {
    const name = keyName(keys.code);
    if (!name) return null;
    parts.push(name);
  }
  return parts.length ? parts.join("+") : null;
}

/** The setup step's presets (F-DI-11 "Recommended / One key / Choose my own"). */
export function presets(platform: string): { recommended: string; oneKey: string; fallback?: string } {
  if (platform === "macos") return { recommended: "Fn", oneKey: "Alt", fallback: "Ctrl+Alt" };
  if (platform === "windows") return { recommended: "Ctrl+Meta", oneKey: "Ctrl" };
  return { recommended: "Ctrl+Alt", oneKey: "Ctrl" };
}
