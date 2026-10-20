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

/** UI_SPEC §2.15 + §1.3: a small coloured mark, always paired with a shape
 * so colour never carries the meaning alone (D-2601): working is a filled dot
 * with a breathing halo, waiting a diamond, done a check, idle a hollow ring,
 * error a triangle and offline a dashed ring. Role img with an aria-label. */
export function StatusDot({ status, size = 8, label, className }: { status: Status; size?: 6 | 8 | 10; label?: string; className?: string }) {
  const s = size;
  return (
    <svg
      role="img"
      aria-label={label ?? STATUS_LABEL[status]}
      width={s}
      height={s}
      viewBox="0 0 10 10"
      overflow="visible"
      data-status={status}
      className={cx("inline-block shrink-0 transition-colors duration-[var(--dur-base)]", COLOR[status], status === "waiting" && "tn-attention", className)}
    >
      {status === "working" && (
        <g fill="currentColor">
          <circle cx="5" cy="5" r="4.6" className="tn-breathe" opacity="0.3" />
          <circle cx="5" cy="5" r="3" />
        </g>
      )}
      {status === "waiting" && <path d="M5 0.6 9.4 5 5 9.4 0.6 5Z" fill="currentColor" />}
      {status === "done" && <path d="M1.5 5.2 4 7.7 8.6 2.4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />}
      {status === "idle" && <circle cx="5" cy="5" r="3.3" fill="none" stroke="currentColor" strokeWidth="1.5" />}
      {status === "error" && <path d="M5 0.9 9.3 8.9H0.7Z" fill="currentColor" strokeLinejoin="round" />}
      {status === "offline" && <circle cx="5" cy="5" r="3.3" fill="none" stroke="currentColor" strokeWidth="1.4" strokeDasharray="2 1.6" />}
    </svg>
  );
}
