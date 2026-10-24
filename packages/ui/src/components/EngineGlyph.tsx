import type { CSSProperties, ReactNode } from "react";
import { cx } from "../cx";

/** Engines with their own tint in brand.css (`--tn-engine-<id>`, D-1895).
 * Any other id (Shell, plugins, a new registry row) sits on a neutral tile. */
export const ENGINE_MARKS = [
  "claude",
  "codex",
  "gemini",
  "copilot",
  "cursor-agent",
  "grok",
  "opencode",
  "droid",
  "amp",
  "aider",
  "kimi",
  "antigravity",
  "kiro",
] as const;

/** The tile's fill: the engine's tint, or the neutral surface. */
export function engineTint(id: string | undefined): string {
  return id && (ENGINE_MARKS as readonly string[]).includes(id)
    ? `var(--tn-engine-${id})`
    : "var(--tn-surface-4)";
}

const SIZE = {
  14: "size-[14px] text-[7px]",
  16: "size-4 text-[8px]",
  20: "size-5 text-[10px]",
  24: "size-6 text-[11px]",
  32: "size-8 text-sm",
} as const;

/** UI_SPEC §1.6 / §2.28, DESIGN §4.6 (D-1895, D-2602): our own monogram on
 * a 4px tile in the engine's quiet tint. The letters are always `fg`, never
 * the hue, and the tile is never the only thing naming its engine. At 80%
 * opacity at rest, 100% when `active` (its pane is focused). */
export function EngineGlyph({
  letters,
  engine,
  size = 16,
  active,
  selected,
  label,
  className,
}: {
  letters: string;
  /** The engine's id ("claude", "codex"…): picks its tint. */
  engine?: string;
  size?: keyof typeof SIZE;
  /** Its pane has focus: full opacity. */
  active?: boolean;
  selected?: boolean;
  /** Accessible name; decorative (hidden) when the name is shown beside it. */
  label?: string;
  className?: string;
}) {
  const style: CSSProperties = { backgroundColor: engineTint(engine) };
  return (
    <span
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      data-engine={engine}
      style={style}
      className={cx(
        "inline-flex shrink-0 select-none items-center justify-center rounded-xs font-semibold leading-none tracking-tight text-fg",
        "transition-opacity duration-[var(--dur-fast)]",
        SIZE[size],
        active || selected ? "opacity-100" : "opacity-80",
        selected && "ring-2 ring-accent ring-offset-1 ring-offset-surface-2",
        className,
      )}
    >
      {letters.slice(0, 2)}
    </span>
  );
}

/** The glyph, the engine's name and an optional account ("Claude Code · Work"). */
export function EngineBadge({
  letters,
  engine,
  name,
  account,
  size = 16,
}: {
  letters: string;
  engine?: string;
  name: ReactNode;
  account?: string;
  size?: 14 | 16 | 20;
}) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-sm text-fg">
      <EngineGlyph letters={letters} engine={engine} size={size} />
      <span className="truncate">
        {name}
        {account && <span className="text-fg-muted"> · {account}</span>}
      </span>
    </span>
  );
}
