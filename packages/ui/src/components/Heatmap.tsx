import { cx } from "../cx";

export type HeatmapDay = { date: string; value: number };

/** Five levels: none, then quartiles of the busiest day. */
export function heatLevel(value: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (value <= 0 || max <= 0) return 0;
  const r = value / max;
  return r > 0.75 ? 4 : r > 0.5 ? 3 : r > 0.25 ? 2 : 1;
}

const LEVEL = [
  "bg-surface-3",
  "bg-accent opacity-25",
  "bg-accent opacity-50",
  "bg-accent opacity-75",
  "bg-accent",
] as const;

/**
 * A calendar heatmap (F-AC-07): one column per week, Monday at the top.
 * `days` are consecutive dates, oldest first; `describe` names each cell for
 * its tooltip and screen readers.
 */
export function Heatmap({
  days,
  label,
  describe,
  className,
}: {
  days: HeatmapDay[];
  label: string;
  describe: (day: HeatmapDay) => string;
  className?: string;
}) {
  const max = Math.max(0, ...days.map((d) => d.value));
  const first = days[0] ? new Date(`${days[0].date}T00:00:00Z`).getUTCDay() : 1;
  // Blank cells before the first day so rows line up Monday to Sunday.
  const lead = (first + 6) % 7;
  const cells: Array<HeatmapDay | null> = [...Array.from({ length: lead }, () => null), ...days];
  return (
    <div
      role="img"
      aria-label={label}
      className={cx("grid grid-flow-col grid-rows-7 gap-[3px]", className)}
      data-testid="heatmap"
    >
      {cells.map((d, i) =>
        d ? (
          <span
            key={d.date}
            title={describe(d)}
            data-date={d.date}
            data-level={heatLevel(d.value, max)}
            className={cx("size-3 rounded-[2px]", LEVEL[heatLevel(d.value, max)])}
          />
        ) : (
          <span key={`lead-${i}`} className="size-3" aria-hidden="true" />
        ),
      )}
    </div>
  );
}
