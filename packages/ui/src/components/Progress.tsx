import { cx } from "../cx";

export type ProgressBarProps = {
  /** 0–1; omit for indeterminate. */
  value?: number;
  label: string;
  valueText?: string;
  className?: string;
};

/** UI_SPEC §2.16: 4px track; indeterminate is a sliding 30% segment. */
export function ProgressBar({ value, label, valueText, className }: ProgressBarProps) {
  const pct = value === undefined ? undefined : Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      aria-valuetext={valueText ?? (pct === undefined ? undefined : `${pct}%`)}
      className={cx("relative h-1 w-full overflow-hidden rounded-full bg-surface-4", className)}
    >
      {pct === undefined ? (
        <span className="tn-indeterminate absolute inset-y-0 w-[30%] rounded-full bg-accent" />
      ) : (
        <span className="absolute inset-y-0 left-0 rounded-full bg-accent transition-[width] duration-[var(--dur-base)]" style={{ width: `${pct}%` }} />
      )}
    </div>
  );
}

/** Fill colour per §1.3: <80% muted, 80–94% warning, ≥95% danger (plan
 * windows only; context meters stop at warning). */
export function meterTone(value: number, kind: "plan" | "context" = "plan"): "muted" | "warning" | "danger" {
  if (value >= 0.95 && kind === "plan") return "danger";
  if (value >= 0.8) return "warning";
  return "muted";
}

const FILL = { muted: "bg-fg-muted", warning: "bg-warning", danger: "bg-danger" };

export type UsageMeterProps = {
  label: string;
  value: number;
  /** "resets in 1h 12m" — already formatted by the caller. */
  resetText?: string;
  size?: "xs" | "sm";
  kind?: "plan" | "context";
  showLabel?: boolean;
  className?: string;
};

export function UsageMeter({ label, value, resetText, size = "sm", kind = "plan", showLabel = true, className }: UsageMeterProps) {
  const pct = Math.round(Math.min(1, Math.max(0, value)) * 100);
  const text = `${pct}% used${resetText ? `, ${resetText}` : ""}`;
  return (
    <span className={cx("inline-flex items-center gap-1.5", size === "sm" && "w-full", className)}>
      {showLabel && <span className="text-2xs text-fg-muted">{label}</span>}
      <span
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-valuetext={text}
        title={`${label}: ${text}`}
        className={cx("relative inline-block overflow-hidden rounded-full bg-surface-4", size === "xs" ? "h-1.5 w-9" : "h-1.5 flex-1")}
      >
        <span className={cx("absolute inset-y-0 left-0 rounded-full", FILL[meterTone(value, kind)])} style={{ width: `${pct}%` }} />
      </span>
    </span>
  );
}
