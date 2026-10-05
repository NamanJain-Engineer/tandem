import { cx } from "../cx";

export type Status = "working" | "waiting" | "done" | "idle" | "error" | "offline";

export const STATUS_LABEL: Record<Status, string> = {
  working: "Working",
  waiting: "Waiting for you",
  done: "Done",
  idle: "Idle",
  error: "Error",
  offline: "Offline",
};

const COLOR: Record<Status, string> = {
  working: "text-working",
  waiting: "text-waiting",
  done: "text-done",
  idle: "text-idle",
  error: "text-error",
  offline: "text-idle",
};

/** UI_SPEC §2.15 + §1.3: colour is always paired with a shape — working
 * three-dot pulse, waiting diamond, done check, idle hollow circle, error
 * triangle. Role img with an aria-label. */
export function StatusDot({ status, size = 8, label, className }: { status: Status; size?: 6 | 8 | 10; label?: string; className?: string }) {
  const s = size;
  return (
    <svg
      role="img"
      aria-label={label ?? STATUS_LABEL[status]}
      width={s}
      height={s}
      viewBox="0 0 10 10"
      className={cx("inline-block shrink-0 transition-colors duration-[var(--dur-base)]", COLOR[status], status === "waiting" && "tn-attention", className)}
    >
      {status === "working" && (
        <g fill="currentColor">
          <circle cx="1.5" cy="5" r="1.4" className="tn-dot tn-dot-1" />
          <circle cx="5" cy="5" r="1.4" className="tn-dot tn-dot-2" />
          <circle cx="8.5" cy="5" r="1.4" className="tn-dot tn-dot-3" />
        </g>
      )}
      {status === "waiting" && <path d="M5 0.5 9.5 5 5 9.5 0.5 5Z" fill="currentColor" />}
      {status === "done" && <path d="M1.5 5.2 4 7.7 8.6 2.4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />}
      {status === "idle" && <circle cx="5" cy="5" r="3.6" fill="none" stroke="currentColor" strokeWidth="1.6" />}
      {status === "error" && <path d="M5 0.8 9.4 9H0.6Z" fill="currentColor" />}
      {status === "offline" && <circle cx="5" cy="5" r="3.6" fill="none" stroke="currentColor" strokeWidth="1.4" strokeDasharray="2 1.6" />}
    </svg>
  );
}
