import shortcuts from "@tandem/protocol/shortcuts.json";
import { Kbd } from "@tandem/ui/components/Kbd";
import { Plus } from "lucide-react";
import { Icon } from "./Icon";
import { cx } from "../../lib/cx";

/** Where a shortcut is pressed: some differ while a terminal has focus. */
export type KeyContext = "default" | "terminal";

/** The Linux binding of a command, from the command registry
 * (`@tandem/protocol/shortcuts.json`): `pcTerminal` in a terminal, `pc`
 * elsewhere. The only build is for Linux, so `Mod` is Ctrl. */
export function linuxKeys(id: string, context: KeyContext = "default"): string[] {
  const s = shortcuts.find((x) => x.id === id);
  if (!s) throw new Error(`no shortcut ${id} in shortcuts.json`);
  const combo = (context === "terminal" ? s.pcTerminal : null) ?? s.pc;
  if (!combo) throw new Error(`${id} has no default shortcut`);
  return combo.split("+");
}

const SHOWN: Record<string, string> = { Mod: "Ctrl" };
export const keyLabel = (keys: string[]): string => keys.map((k) => SHOWN[k] ?? k).join("+");

/** A shortcut as key caps, generated from the registry so the site can't
 * drift from the app. `size="lg"` draws the big caps of the Code mode row. */
export function Keys({
  id,
  context = "default",
  size = "sm",
  className,
}: {
  id: string;
  context?: KeyContext;
  size?: "sm" | "lg";
  className?: string;
}) {
  const keys = linuxKeys(id, context);
  if (size === "sm") return <Kbd keys={keys} className={className} />;
  return (
    <span className={cx("inline-flex items-center gap-1.5", className)} aria-label={keyLabel(keys)} role="img">
      {keys.map((k, i) => (
        <span key={`${k}-${i}`} className="inline-flex items-center gap-1.5" aria-hidden="true">
          {i > 0 && <Icon icon={Plus} size={14} className="text-fg-subtle" />}
          <kbd className="keycap">
            {SHOWN[k] ?? k}
          </kbd>
        </span>
      ))}
    </span>
  );
}
