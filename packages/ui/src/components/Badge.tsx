import { X } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../cx";

export type BadgeVariant = "neutral" | "accent" | "working" | "waiting" | "done" | "idle" | "error" | "outline";

// Status and accent badges keep their *-text colour on the surface behind
// them, with a tinted border: the §7.3 text tokens pass AA on surfaces but not
// on their own soft tints at badge size.
const BADGE: Record<BadgeVariant, string> = {
  neutral: "bg-surface-4 text-fg-muted",
  accent: "border border-accent/40 text-accent",
  working: "border border-working/40 text-working-text",
  waiting: "border border-waiting/50 text-waiting-text",
  done: "border border-done/40 text-done-text",
  idle: "border border-idle/40 text-fg-muted",
  error: "border border-error/40 text-error",
  outline: "border border-border-strong text-fg-muted",
};

const DOT: Partial<Record<BadgeVariant, string>> = {
  accent: "bg-accent",
  working: "bg-working",
  waiting: "bg-waiting",
  done: "bg-done",
  idle: "bg-idle",
  error: "bg-error",
};

/** UI_SPEC §2.13: 18px, text-2xs medium. Status badges use the *-text tokens. */
export function Badge({ variant = "neutral", dot, children, className }: { variant?: BadgeVariant; dot?: boolean; children: ReactNode; className?: string }) {
  return (
    <span className={cx("inline-flex h-[18px] shrink-0 items-center gap-1 rounded-xs px-1.5 text-2xs font-medium uppercase tracking-caps", BADGE[variant], className)}>
      {dot && <span aria-hidden="true" className={cx("size-1.5 rounded-full", DOT[variant] ?? "bg-fg-muted")} />}
      {children}
    </span>
  );
}

export type ChipProps = {
  label: string;
  icon?: ReactNode;
  onRemove?: () => void;
  removeLabel?: (label: string) => string;
  selected?: boolean;
  className?: string;
};

/** 24px, fully rounded, removable. */
export function Chip({ label, icon, onRemove, removeLabel = (l) => `Remove ${l}`, selected, className }: ChipProps) {
  return (
    <span
      className={cx(
        "inline-flex h-6 max-w-full items-center gap-1.5 rounded-full border border-border bg-surface-4 pl-2.5 text-sm text-fg",
        onRemove ? "pr-1" : "pr-2.5",
        selected && "border-accent bg-accent-soft",
        className,
      )}
    >
      {icon && <span aria-hidden="true" className="inline-flex">{icon}</span>}
      <span className="truncate" title={label}>
        {label}
      </span>
      {onRemove && (
        <button
          type="button"
          aria-label={removeLabel(label)}
          onClick={onRemove}
          className="inline-flex size-4 items-center justify-center rounded-full text-fg-subtle hover:bg-surface-3 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          <X size={12} aria-hidden="true" />
        </button>
      )}
    </span>
  );
}
