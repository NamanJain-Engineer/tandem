import { useEffect, useState } from "react";
import { cx } from "../cx";

export type AgoLabels = { now: string; minutes: (n: number) => string; hours: (n: number) => string; days: (n: number) => string };

export const AGO_LABELS: AgoLabels = {
  now: "now",
  minutes: (n) => `${n}m`,
  hours: (n) => `${n}h`,
  days: (n) => `${n}d`,
};

/** Compact relative time: "now", "4m", "2h", "3d" (D-2601). */
export function compactAgo(at: number, now: number, labels: AgoLabels = AGO_LABELS): string {
  const m = Math.max(0, Math.floor((now - at) / 60_000));
  if (m < 1) return labels.now;
  if (m < 60) return labels.minutes(m);
  const h = Math.floor(m / 60);
  return h < 24 ? labels.hours(h) : labels.days(Math.floor(h / 24));
}

/** The current time, refreshed every `everyMs` (a minute by default) while
 * `enabled`. */
export function useNow(everyMs = 60_000, enabled = true): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!enabled) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), everyMs);
    return () => clearInterval(id);
  }, [everyMs, enabled]);
  return now;
}

/** A quiet, tabular relative time ("2h"). The full date is its tooltip and,
 * for screen readers, its text, so it needs no tab stop. Nothing renders
 * for a time that isn't a number. */
export function TimeAgo({
  at,
  now,
  labels,
  formatFull = (d) => d.toLocaleString(),
  className,
}: {
  at: number;
  /** Pin "now" (tests, lists that share one clock); otherwise it ticks each minute. */
  now?: number;
  labels?: AgoLabels;
  /** The full date, for the tooltip and screen readers. */
  formatFull?: (date: Date) => string;
  className?: string;
}) {
  const ticking = useNow(60_000, now === undefined);
  if (!Number.isFinite(at)) return null;
  const date = new Date(at);
  const full = formatFull(date);
  return (
    <time
      dateTime={date.toISOString()}
      title={full}
      className={cx("shrink-0 text-xs tabular-nums text-fg-subtle", className)}
    >
      <span aria-hidden="true">{compactAgo(at, now ?? ticking, labels)}</span>
      <span className="sr-only">{full}</span>
    </time>
  );
}
