import { useCallback } from "react";
import { useSettings } from "../../stores/settings";
import { useBoot } from "../shell/useBoot";
import { COMMANDS, bindingFor, type ShortcutOverrides } from "./commands";
import { keysOf } from "./keys";

const NONE: ShortcutOverrides = {};

/** Kbd caps for registry commands as they are now, rebinding included
 * (F-SH-09 P2): `keys("pane.split_right")` → `["Mod", "D"]`, or undefined
 * when the command has no shortcut. Tooltips and menus show these. */
export function useShortcutKeys(): (id: string) => string[] | undefined {
  const mac = useBoot((s) => s.boot?.platform === "macos");
  const overrides = useSettings((s) => (s.values.shortcut_overrides as ShortcutOverrides | undefined) ?? NONE);
  return useCallback(
    (id: string) => {
      const spec = COMMANDS.find((c) => c.id === id);
      const keys = spec ? bindingFor(spec, { mac, terminalFirst: false }, overrides) : null;
      return keys ? keysOf(keys) : undefined;
    },
    [mac, overrides],
  );
}
