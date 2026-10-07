import { cx } from "../cx";
import { shortcutCaps } from "../platform";

export type KbdProps = {
  /** A literal label ("⌘↩") or keys rendered as separate caps; `Mod`, `Alt`
   * and `Shift` follow the platform (⌘/Ctrl, ⌥/Alt, ⇧/Shift). */
  keys: string | string[];
  className?: string;
};

const CAP =
  "inline-flex h-[18px] min-w-[16px] items-center justify-center rounded-xs border border-border bg-[var(--tn-kbd-bg)] px-1 font-mono text-2xs text-fg-muted";

/** A keyboard shortcut chip (UI_SPEC §2.19). */
export function Kbd({ keys, className }: KbdProps) {
  if (typeof keys === "string") return <kbd className={cx(CAP, className)}>{keys}</kbd>;
  return (
    <span className={cx("inline-flex items-center gap-0.5", className)}>
      {shortcutCaps(keys).map((k, i) => (
        <kbd key={`${k}-${i}`} className={CAP}>
          {k}
        </kbd>
      ))}
    </span>
  );
}
