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

/** The current time, refreshed every `everyMs` (a minute by default). */
export function useNow(everyMs = 60_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), everyMs);
    return () => clearInterval(id);
  }, [everyMs]);
  return now;
}

/** A quiet, tabular relative time whose tooltip is the full date. */
export function TimeAgo({
  at,
  now,
  labels,
  className,
}: {
  at: number;
  /** Pin "now" (tests, lists that share one clock); otherwise it ticks each minute. */
  now?: number;
  labels?: AgoLabels;
  className?: string;
}) {
  const ticking = useNow();
  const date = new Date(at);
  return (
    <time
      dateTime={date.toISOString()}
      title={date.toLocaleString()}
      className={cx("shrink-0 text-xs tabular-nums text-fg-subtle", className)}
    >
      {compactAgo(at, now ?? ticking, labels)}
    </time>
  );
}
