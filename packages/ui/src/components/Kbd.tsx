import { cx } from "../cx";

export type KbdProps = { keys: string; className?: string };

/** A keyboard shortcut chip (UI_SPEC §2.19). */
export function Kbd({ keys, className }: KbdProps) {
  return (
    <kbd
      className={cx(
        "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-xs border border-border bg-[var(--tn-kbd-bg)] px-1 font-mono text-2xs text-fg-muted",
        className,
      )}
    >
      {keys}
    </kbd>
  );
}
