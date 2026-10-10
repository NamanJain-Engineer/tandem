import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cx } from "../cx";
import { Kbd } from "./Kbd";
import { Spinner } from "./Spinner";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "link";
export type ButtonSize = "sm" | "md" | "lg";

export type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  iconLeft?: ReactNode;
  iconRight?: ReactNode;
  loading?: boolean;
  /** Shortcut hint rendered as a trailing Kbd, e.g. "⌘↩". */
  kbd?: string;
  fullWidth?: boolean;
  /** Explains why the button is disabled (tooltip). */
  disabledReason?: string;
  children?: ReactNode;
};

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover active:bg-accent-press",
  secondary: "bg-surface-4 text-fg border border-border hover:border-border-strong",
  ghost: "bg-transparent text-fg hover:bg-surface-4",
  danger: "bg-danger text-danger-fg hover:bg-danger-hover",
  link: "bg-transparent text-accent hover:underline px-0",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "h-7 px-2.5 text-sm gap-1.5",
  md: "h-8 px-3 text-base gap-1.5",
  lg: "h-10 px-4 text-md gap-2",
};

/** UI_SPEC §2.1. Loading keeps the label, swaps the left icon for a spinner,
 * sets aria-busy and blocks clicks. Disabled is 40% opacity with no hover. */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = "secondary",
    size = "md",
    iconLeft,
    iconRight,
    loading = false,
    kbd,
    fullWidth = false,
    disabledReason,
    disabled,
    className,
    children,
    onClick,
    type = "button",
    ...rest
  },
  ref,
) {
  const inert = disabled || loading;
  return (
    <button
      ref={ref}
      type={type}
      data-variant={variant}
      className={cx(
        "inline-flex select-none items-center justify-center whitespace-nowrap rounded-sm font-medium",
        "transition-colors duration-[var(--dur-instant)] focus-visible:shadow-focus focus-visible:outline-none",
        VARIANTS[variant],
        SIZES[size],
        fullWidth && "w-full",
        disabled && "pointer-events-auto cursor-not-allowed opacity-40 hover:bg-[inherit]",
        loading && "cursor-progress",
        className,
      )}
      disabled={disabled}
      aria-disabled={inert || undefined}
      aria-busy={loading || undefined}
      title={disabled ? disabledReason : rest.title}
      onClick={(event) => {
        if (inert) {
          event.preventDefault();
          return;
        }
        onClick?.(event);
      }}
      {...rest}
    >
      {loading ? <Spinner /> : iconLeft}
      {children !== undefined && <span className="truncate">{children}</span>}
      {iconRight}
      {kbd && <Kbd keys={kbd} className="ml-1" />}
    </button>
  );
});
