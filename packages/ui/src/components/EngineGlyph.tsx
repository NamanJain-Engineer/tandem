import type { ReactNode } from "react";
import { cx } from "../cx";

const SIZE = {
  14: "size-[14px] text-[7px]",
  16: "size-4 text-[8px]",
  20: "size-5 text-2xs",
  32: "size-8 text-sm",
} as const;

/** UI_SPEC §1.6 / §2.28: Tandem's own monochrome mark for an engine — its
 * letters in a circle, never a vendor logo. */
export function EngineGlyph({
  letters,
  size = 16,
  selected,
  label,
  className,
}: {
  letters: string;
  size?: keyof typeof SIZE;
  selected?: boolean;
  /** Accessible name; decorative (hidden) when the name is shown beside it. */
  label?: string;
  className?: string;
}) {
  return (
    <span
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      className={cx(
        "inline-flex shrink-0 select-none items-center justify-center rounded-full bg-surface-4 font-semibold leading-none",
        SIZE[size],
        selected ? "text-accent" : "text-fg-muted",
        className,
      )}
    >
      {letters.slice(0, 2)}
    </span>
  );
}

/** The glyph, the engine's name and an optional account ("Claude Code · Work"). */
export function EngineBadge({ letters, name, account, size = 16 }: { letters: string; name: ReactNode; account?: string; size?: 14 | 16 | 20 }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-sm text-fg">
      <EngineGlyph letters={letters} size={size} />
      <span className="truncate">
        {name}
        {account && <span className="text-fg-muted"> · {account}</span>}
      </span>
    </span>
  );
}
