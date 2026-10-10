// UI-52 CronInput: Daily, Weekly, Interval or Cron, a time zone, and the
// resolved preview (words, the next 3 runs, a clock-change note) that the
// core computes as you type. Each tab keeps its own values, so switching
// tabs never loses a cron expression.
import { useEffect, useMemo, useState } from "react";
import type { Schedule, SchedulePreview, ScheduleSpec } from "@tandem/protocol";
import { Button, Input, Popover, SegmentedControl, Select } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { isIpcError, previewSchedule } from "../../stores/routines";
import {
  WEEK,
  WEEKDAYS,
  WEEKENDS,
  clockChange,
  describe,
  firstTime,
  formatDay,
  formatRun,
  time12h,
  zones,
} from "./schedule";

type Kind = Schedule["kind"];

export type CronDraft = {
  kind: Kind;
  dailyTime: string;
  weeklyDays: number[];
  weeklyTime: string;
  every: number;
  unit: "minutes" | "hours";
  cron: string;
  timezone: string;
};

export function draftOf(spec: ScheduleSpec): CronDraft {
  const d: CronDraft = {
    kind: spec.schedule.kind,
    dailyTime: "09:00",
    weeklyDays: WEEKDAYS,
    weeklyTime: "09:00",
    every: 6,
    unit: "hours",
    cron: "0 9 * * 1-5",
    timezone: spec.timezone,
  };
  const s = spec.schedule;
  if (s.kind === "daily") d.dailyTime = s.times[0] ?? "09:00";
  if (s.kind === "weekly") {
    d.weeklyDays = s.days;
    d.weeklyTime = s.times[0] ?? "09:00";
  }
  if (s.kind === "interval") {
    const hours = s.every_minutes % 60 === 0;
    d.every = hours ? s.every_minutes / 60 : s.every_minutes;
    d.unit = hours ? "hours" : "minutes";
  }
  if (s.kind === "cron") d.cron = s.expr;
  return d;
}

export function specOf(d: CronDraft): ScheduleSpec {
  const schedule: Schedule =
    d.kind === "daily"
      ? { kind: "daily", times: [d.dailyTime] }
      : d.kind === "weekly"
        ? { kind: "weekly", days: [...d.weeklyDays].sort((a, b) => a - b), times: [d.weeklyTime] }
        : d.kind === "interval"
          ? {
              kind: "interval",
              every_minutes: Math.max(0, Math.round(d.every)) * (d.unit === "hours" ? 60 : 1),
            }
          : { kind: "cron", expr: d.cron.trim() };
  return { schedule, timezone: d.timezone };
}

const same = (a: number[], b: number[]) => a.length === b.length && a.every((x) => b.includes(x));

/** The preview for a spec, refreshed 150 ms after the last change. */
export function useSchedulePreview(spec: ScheduleSpec): {
  preview: SchedulePreview | null;
  error: string | null;
  loading: boolean;
} {
  const key = JSON.stringify(spec);
  const [state, setState] = useState<{ key: string; preview: SchedulePreview | null; error: string | null }>({
    key: "",
    preview: null,
    error: null,
  });
  useEffect(() => {
    let live = true;
    const id = window.setTimeout(() => {
      void previewSchedule(JSON.parse(key) as ScheduleSpec).then((r) => {
        if (!live) return;
        if (isIpcError(r)) setState({ key, preview: null, error: r.message });
        else setState({ key, preview: r, error: null });
      });
    }, 150);
    return () => {
      live = false;
      window.clearTimeout(id);
    };
  }, [key]);
  return { preview: state.preview, error: state.error, loading: state.key !== key };
}

export function CronInput({ value, onChange }: { value: CronDraft; onChange: (d: CronDraft) => void }) {
  const set = (patch: Partial<CronDraft>) => onChange({ ...value, ...patch });
  const spec = useMemo(() => specOf(value), [value]);
  const { preview, error, loading } = useSchedulePreview(spec);
  const tzOptions = useMemo(
    () => zones(value.timezone).map((z) => ({ value: z, label: z })),
    [value.timezone],
  );
  const change = useMemo(
    () => (preview ? clockChange(value.timezone, Date.now()) : null),
    [preview, value.timezone],
  );
  const keep = firstTime(spec.schedule);
  const toggleDay = (d: number) =>
    set({
      weeklyDays: value.weeklyDays.includes(d)
        ? value.weeklyDays.filter((x) => x !== d)
        : [...value.weeklyDays, d],
    });
  return (
    <div className="flex flex-col gap-3" data-testid="cron-input">
      <SegmentedControl
        label={t("routines.schedule.label")}
        value={value.kind}
        onValueChange={(k) => set({ kind: k as Kind })}
        items={(["daily", "weekly", "interval", "cron"] as const).map((k) => ({
          id: k,
          label: t(`routines.schedule.${k}` as MessageKey),
        }))}
      />
      {value.kind === "daily" && (
        <Input
          type="time"
          label={t("routines.schedule.at")}
          value={value.dailyTime}
          onChange={(e) => set({ dailyTime: e.target.value })}
          data-testid="cron-time"
        />
      )}
      {value.kind === "weekly" && (
        <div className="flex flex-col gap-2">
          <div
            className="flex flex-wrap items-center gap-1"
            role="group"
            aria-label={t("routines.schedule.days")}
          >
            {WEEK.map((d) => (
              <Button
                key={d}
                size="sm"
                variant={value.weeklyDays.includes(d) ? "primary" : "secondary"}
                aria-pressed={value.weeklyDays.includes(d)}
                aria-label={t(`routines.dayName.${d}` as MessageKey)}
                onClick={() => toggleDay(d)}
                data-testid={`cron-day-${d}`}
              >
                {t(`routines.dayShort.${d}` as MessageKey)}
              </Button>
            ))}
            <span className="w-2" />
            <Button
              size="sm"
              variant={same(value.weeklyDays, WEEKDAYS) ? "primary" : "ghost"}
              onClick={() => set({ weeklyDays: WEEKDAYS })}
            >
              {t("routines.schedule.weekdays")}
            </Button>
            <Button
              size="sm"
              variant={same(value.weeklyDays, WEEKENDS) ? "primary" : "ghost"}
              onClick={() => set({ weeklyDays: WEEKENDS })}
            >
              {t("routines.schedule.weekends")}
            </Button>
          </div>
          <Input
            type="time"
            label={t("routines.schedule.at")}
            value={value.weeklyTime}
            onChange={(e) => set({ weeklyTime: e.target.value })}
            data-testid="cron-time"
          />
        </div>
      )}
      {value.kind === "interval" && (
        <div className="flex items-end gap-2">
          <Input
            type="number"
            min={1}
            label={t("routines.schedule.every")}
            value={String(value.every)}
            onChange={(e) => set({ every: Number(e.target.value) })}
            data-testid="cron-every"
          />
          <Select
            label={t("routines.schedule.unit")}
            value={value.unit}
            options={[
              { value: "minutes", label: t("routines.schedule.minutes") },
              { value: "hours", label: t("routines.schedule.hours") },
            ]}
            onValueChange={(u) => set({ unit: u as CronDraft["unit"] })}
          />
        </div>
      )}
      {value.kind === "cron" && (
        <div className="flex items-end gap-2">
          <Input
            monospace
            label={t("routines.schedule.cron")}
            hint={t("routines.schedule.cronHint")}
            value={value.cron}
            spellCheck={false}
            onChange={(e) => set({ cron: e.target.value })}
            data-testid="cron-expr"
          />
          <Popover
            label={t("routines.schedule.cheatSheet")}
            trigger={
              <Button size="sm" variant="ghost">
                {t("routines.schedule.cheatSheet")}
              </Button>
            }
          >
            <ul className="flex flex-col gap-1 p-2 text-sm text-fg-muted">
              {(["a", "b", "c", "d", "e"] as const).map((k) => (
                <li key={k}>{t(`routines.schedule.cheat.${k}` as MessageKey)}</li>
              ))}
            </ul>
          </Popover>
        </div>
      )}
      <Select
        label={t("routines.schedule.timezone")}
        value={value.timezone}
        options={tzOptions}
        onValueChange={(z) => set({ timezone: z })}
      />
      <div
        className="flex flex-col gap-1 rounded-md border border-border bg-surface-2 px-3 py-2"
        aria-live="polite"
        data-testid="cron-preview"
        data-loading={loading ? "true" : "false"}
      >
        {error && !loading ? (
          <p className="text-sm text-danger" data-testid="cron-error">
            {error}
          </p>
        ) : preview ? (
          <>
            <p className="text-base font-medium text-fg" data-testid="cron-words">
              {t("routines.schedule.inZone", { words: describe(preview.words), zone: value.timezone })}
            </p>
            <p className="text-sm text-fg-muted" data-testid="cron-next">
              {t("routines.schedule.next", {
                runs: preview.next.map((n) => formatRun(n, value.timezone)).join(" · "),
              })}
            </p>
            {change !== null && (
              <p className="text-sm text-fg-muted" data-testid="cron-dst">
                {keep
                  ? t("routines.schedule.dstKeep", {
                      date: formatDay(change, value.timezone),
                      time: time12h(keep),
                    })
                  : t("routines.schedule.dst", { date: formatDay(change, value.timezone) })}
              </p>
            )}
          </>
        ) : (
          <p className="text-sm text-fg-muted">{t("routines.schedule.resolving")}</p>
        )}
      </div>
    </div>
  );
}
