import { cx } from "../cx";

export type SpinnerProps = { size?: number; className?: string; label?: string };

/** Indeterminate spinner (UI_SPEC §2.18). Decorative unless `label` is given. */
export function Spinner({ size = 14, className, label }: SpinnerProps) {
  return (
    <svg
      className={cx("animate-spin motion-reduce:animate-none", className)}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <circle cx="8" cy="8" r="6.5" stroke="currentColor" strokeOpacity="0.25" strokeWidth="2" />
      <path d="M14.5 8A6.5 6.5 0 0 0 8 1.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}
