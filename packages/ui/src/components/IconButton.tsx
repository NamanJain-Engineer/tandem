import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cx } from "../cx";
import { Tooltip, type TooltipProps } from "./Tooltip";

export type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  icon: ReactNode;
  /** Required: the accessible name and the tooltip (UI_SPEC §2.2). */
  label: string;
  variant?: "ghost" | "secondary" | "primary";
  size?: "sm" | "md";
  /** A count or a dot at the top-right. */
  badge?: number | "dot";
  badgeTone?: "neutral" | "waiting";
  /** Toggle state → aria-pressed. */
  pressed?: boolean;
  kbd?: string[];
  tooltip?: boolean;
  /** Where the tooltip goes (below by default). */
  tooltipSide?: TooltipProps["side"];
};

const VARIANTS = {
  ghost: "bg-transparent text-fg-muted hover:bg-surface-4 hover:text-fg",
  secondary: "bg-surface-4 text-fg border border-border hover:border-border-strong",
  primary: "bg-accent text-accent-fg hover:bg-accent-hover active:bg-accent-press",
};

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { icon, label, variant = "ghost", size = "md", badge, badgeTone = "neutral", pressed, kbd, tooltip = true, tooltipSide, className, type = "button", ...rest },
  ref,
) {
  const button = (
    <button
      ref={ref}
      type={type}
      aria-label={label}
      aria-pressed={pressed}
      className={cx(
        "relative inline-flex shrink-0 items-center justify-center rounded-sm transition-colors duration-[var(--dur-instant)]",
        "focus-visible:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-40",
        size === "sm" ? "size-7" : "size-8",
        VARIANTS[variant],
        pressed && "bg-accent-soft text-accent hover:bg-accent-soft hover:text-accent",
        className,
      )}
      {...rest}
    >
      <span aria-hidden="true" className="inline-flex">
        {icon}
      </span>
      {badge !== undefined && (
        <span
          aria-hidden="true"
          className={cx(
            "absolute -right-0.5 -top-0.5 inline-flex min-w-[14px] items-center justify-center rounded-full px-1 text-2xs font-semibold tabular-nums",
            badge === "dot" ? "size-2 min-w-0 p-0" : "h-[14px]",
            badgeTone === "waiting" ? "bg-surface-1 text-waiting-text ring-1 ring-waiting" : "bg-surface-4 text-fg ring-1 ring-border",
          )}
        >
          {badge === "dot" ? null : badge > 99 ? "99+" : badge}
        </span>
      )}
    </button>
  );
  return tooltip ? (
    <Tooltip label={label} kbd={kbd} side={tooltipSide}>
      {button}
    </Tooltip>
  ) : (
    button
  );
});
