// Schedules in words and times (F-RT-01 AC3, UI-52): the core resolves a
// schedule into `ScheduleWords`; the UI says them through i18n, formats run
// times in the routine's own time zone and notes a clock change ahead.
import type { Schedule, ScheduleWords } from "@tandem/protocol";
import { t, type MessageKey } from "../../i18n";

/** "09:00" → "9:00 AM". */
export function time12h(hhmm: string): string {
  const [h, m] = hhmm.split(":").map(Number);
  if (h === undefined || m === undefined || Number.isNaN(h) || Number.isNaN(m)) return hhmm;
  const suffix = h < 12 ? "AM" : "PM";
  const hour = h % 12 === 0 ? 12 : h % 12;
  return `${hour}:${String(m).padStart(2, "0")} ${suffix}`;
}

function list(items: string[]): string {
  return new Intl.ListFormat("en", { style: "long", type: "conjunction" }).format(items);
}

const dayPlural = (d: number) => t(`routines.days.${d % 7}` as MessageKey);

/** A schedule in words: "Weekdays at 9:00 AM". */
export function describe(w: ScheduleWords): string {
  const at = (times: string[]) => list(times.map(time12h));
  switch (w.kind) {
    case "every_day":
      return t("routines.words.everyDay", { times: at(w.times) });
    case "weekdays":
      return t("routines.words.weekdays", { times: at(w.times) });
    case "weekends":
      return t("routines.words.weekends", { times: at(w.times) });
    case "days":
      return t("routines.words.days", { days: list(w.days.map(dayPlural)), times: at(w.times) });
    case "every_minutes":
      if (w.minutes >= 60 && w.minutes % 60 === 0) {
        const h = w.minutes / 60;
        return h === 1 ? t("routines.words.everyHour") : t("routines.words.everyHours", { n: h });
      }
      return t("routines.words.everyMinutes", { n: w.minutes });
    case "hourly":
      return t("routines.words.hourly", { minute: String(w.minute).padStart(2, "0") });
    case "cron":
      return t("routines.words.cron", { expr: w.expr });
  }
}

/** A run time in the routine's zone: "Mon 29 Sep, 9:00 AM". */
export function formatRun(ms: number, timeZone: string): string {
  try {
    return new Intl.DateTimeFormat("en-GB", {
      timeZone,
      weekday: "short",
      day: "numeric",
      month: "short",
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    }).format(new Date(ms));
  } catch {
    return new Date(ms).toLocaleString();
  }
}

/** One offset formatter per zone: building one is far slower than using
 * it, and `clockChange` asks a zone up to 84 times. `null` for a zone the
 * runtime doesn't know. */
const offsetFormats = new Map<string, Intl.DateTimeFormat | null>();

function offsetFormat(timeZone: string): Intl.DateTimeFormat | null {
  let format = offsetFormats.get(timeZone);
  if (format === undefined) {
    try {
      format = new Intl.DateTimeFormat("en-US", { timeZone, timeZoneName: "longOffset" });
    } catch {
      format = null;
    }
    offsetFormats.set(timeZone, format);
  }
  return format;
}

/** The zone's UTC offset at `ms`, in minutes. */
export function offsetAt(timeZone: string, ms: number): number | null {
  const format = offsetFormat(timeZone);
  if (!format) return null;
  try {
    const part = format.formatToParts(new Date(ms)).find((p) => p.type === "timeZoneName")?.value;
    if (!part) return null;
    const m = /GMT(?:([+-])(\d{2}):?(\d{2}))?/.exec(part);
    if (!m) return null;
    if (!m[1]) return 0;
    const v = Number(m[2]) * 60 + Number(m[3]);
    return m[1] === "-" ? -v : v;
  } catch {
    return null;
  }
}

/** The first day the zone's clocks change within `days` of `from`, if any
 * (UI-52's DST note). Day steps, then hours, so it names the right date. */
export function clockChange(timeZone: string, from: number, days = 60): number | null {
  const start = offsetAt(timeZone, from);
  if (start === null) return null;
  const DAY = 86_400_000;
  for (let d = 1; d <= days; d++) {
    const at = from + d * DAY;
    if (offsetAt(timeZone, at) !== start) {
      for (let h = 1; h <= 24; h++) {
        const hour = at - DAY + h * 3_600_000;
        if (offsetAt(timeZone, hour) !== start) return hour;
      }
      return at;
    }
  }
  return null;
}

export function formatDay(ms: number, timeZone: string): string {
  try {
    return new Intl.DateTimeFormat("en-GB", { timeZone, day: "numeric", month: "short" }).format(
      new Date(ms),
    );
  } catch {
    return new Date(ms).toDateString();
  }
}

/** The machine's IANA zone. */
export function localZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

/** Every zone the runtime knows, the given one included. */
export function zones(current: string): string[] {
  let all: string[] = [];
  try {
    all =
      (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.("timeZone") ??
      [];
  } catch {
    all = [];
  }
  if (!all.includes("UTC")) all = ["UTC", ...all];
  return all.includes(current) ? all : [current, ...all];
}

/** The time a schedule's preview DST note keeps ("runs stay at 9:00 AM"). */
export function firstTime(s: Schedule): string | null {
  if (s.kind === "daily" || s.kind === "weekly") return s.times[0] ?? null;
  return null;
}

/** Weekly days in the order the toggles show them: Monday first. */
export const WEEK = [1, 2, 3, 4, 5, 6, 0];
export const WEEKDAYS = [1, 2, 3, 4, 5];
export const WEEKENDS = [6, 0];
