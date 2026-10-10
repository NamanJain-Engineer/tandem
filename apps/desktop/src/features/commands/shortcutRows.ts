// The registry's shortcuts as people read them (F-SH-09 P2, UI-113,
// UI_SPEC §5): in §5's groups, with the keys in effect after any rebinding.
// Settings › Shortcuts and the cheatsheet both list these.
import type { CommandCategory } from "@tandem/protocol";
import { shortcutLabel } from "@tandem/ui";
import type { MessageKey } from "../../i18n";
import { COMMANDS, bindingFor, commandTitle, type ShortcutOverrides } from "./commands";
import { keysOf, sameShortcut } from "./keys";

export const SHORTCUT_GROUPS: Array<{ id: string; label: MessageKey; categories: CommandCategory[] }> = [
  { id: "app", label: "shortcuts.group.app", categories: ["app", "navigation", "help"] },
  { id: "panes", label: "shortcuts.group.panes", categories: ["workspace", "pane"] },
  { id: "terminal", label: "shortcuts.group.terminal", categories: ["terminal"] },
];

export type ShortcutRow = {
  id: string;
  category: CommandCategory;
  title: string;
  /** The keys in effect, or null for none. */
  keys: string | null;
  /** Windows and Linux: the keys a focused terminal leaves to the app under
   * "Terminal first" (null: the key goes to the shell). */
  terminal: string | null;
  rebound: boolean;
};

export function shortcutRows(mac: boolean, overrides: ShortcutOverrides): ShortcutRow[] {
  return COMMANDS.map((spec) => ({
    id: spec.id,
    category: spec.category,
    title: commandTitle(spec.id),
    keys: bindingFor(spec, { mac, terminalFirst: false }, overrides),
    terminal: mac ? null : bindingFor(spec, { mac, terminalFirst: true }, overrides),
    rebound: typeof overrides[spec.id] === "string",
  }));
}

/** How keys read on this platform: "Ctrl+Alt+R", "⌥⌘R". */
export const keysLabel = (keys: string, mac: boolean) => shortcutLabel(keysOf(keys), mac);

/** The rows in §5's groups, narrowed to those whose name or keys match. */
export function groupRows(rows: ShortcutRow[], query: string, mac: boolean) {
  const q = query.trim().toLowerCase();
  const hit = (r: ShortcutRow) => !q || r.title.toLowerCase().includes(q) || (r.keys !== null && keysLabel(r.keys, mac).toLowerCase().includes(q));
  return SHORTCUT_GROUPS.map((g) => ({ ...g, rows: rows.filter((r) => g.categories.includes(r.category) && hit(r)) })).filter(
    (g) => g.rows.length > 0,
  );
}

/** The other command already on these keys, outside terminals or in them. */
export function conflictFor(id: string, keys: string, mac: boolean, overrides: ShortcutOverrides): string | null {
  for (const spec of COMMANDS) {
    if (spec.id === id) continue;
    const taken = [bindingFor(spec, { mac, terminalFirst: false }, overrides), mac ? null : bindingFor(spec, { mac, terminalFirst: true }, overrides)];
    if (taken.some((k) => k !== null && sameShortcut(k, keys, mac))) return spec.id;
  }
  return null;
}

/** The overrides with `id` on these keys ("" for none). Its default keys
 * drop the entry, so the terminal alias comes back with them. */
export function withKeys(overrides: ShortcutOverrides, id: string, keys: string, mac: boolean): ShortcutOverrides {
  const spec = COMMANDS.find((c) => c.id === id);
  const fallback = spec ? (mac ? spec.mac : spec.pc) : null;
  const rest = { ...overrides };
  delete rest[id];
  const isDefault = keys === "" ? fallback === null : fallback !== null && sameShortcut(fallback, keys, mac);
  return isDefault ? rest : { ...rest, [id]: keys };
}
