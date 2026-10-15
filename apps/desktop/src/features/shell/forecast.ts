import type { AccountSessions, UsageView, UsageWindow, WindowForecast } from "@tandem/protocol";
import { t } from "../../i18n";
import { resetClock } from "../../stores/limits";

/** "1 h 30 min", "45 min", "2 d 3 h" (F-AC-08). */
export function durationText(ms: number): string {
  const minutes = Math.floor((Math.max(0, ms) + 30_000) / 60_000);
  const d = Math.floor(minutes / 1_440);
  const h = Math.floor((minutes % 1_440) / 60);
  const m = minutes % 60;
  if (d > 0) return h > 0 ? t("forecast.dh", { d, h }) : t("forecast.d", { d });
  if (h > 0) return m > 0 ? t("forecast.hm", { h, m }) : t("forecast.h", { h });
  return t("forecast.m", { m });
}

/** A window's forecast line in the usage popover. */
export function forecastLine(f: WindowForecast | undefined, now = Date.now()): string | null {
  if (!f) return null;
  if (f.state === "not_enough_data") return t("usage.forecast.notEnough");
  const basis = t("usage.forecast.basis", { n: f.basisMinutes });
  if (f.state === "on_track") return t("usage.forecast.onTrack", { basis });
  const time = f.runsOutAt !== null ? resetClock(f.runsOutAt, now) : "";
  return f.beforeResetMs !== null
    ? t("usage.forecast.runsOut", { time, before: durationText(f.beforeResetMs), basis })
    : t("usage.forecast.runsOutNoReset", { time, basis });
}

/** The forecast beside a window (same kind). */
export function forecastFor(view: UsageView | undefined, w: UsageWindow): WindowForecast | undefined {
  return view?.forecasts.find((f) => f.kind === w.kind);
}

/** True when a window is at 80% or more, or forecast to run out before its reset. */
export function isTight(view: UsageView | undefined): { pct: number } | null {
  if (!view) return null;
  let top: number | null = null;
  view.windows.forEach((w, i) => {
    const short = view.forecasts[i]?.state === "runs_out";
    if (w.usedPct >= 80 || short) top = Math.max(top ?? 0, w.usedPct);
  });
  return top === null ? null : { pct: Math.round(top) };
}

/** Below 50% in every window (and with at least one reading). */
export function hasRoom(view: UsageView | undefined): boolean {
  return !!view && view.windows.length > 0 && view.windows.every((w) => w.usedPct < 50);
}

/** "Who's using it" rows: name and share, largest first. */
export function whoRows(
  row: AccountSessions | undefined,
  paneName: (paneId: string) => string | null,
): { key: string; label: string }[] {
  if (!row) return [];
  return row.sessions.map((s) => ({
    key: s.sessionId,
    label: t("usage.who.row", {
      name: (s.paneId ? paneName(s.paneId) : null) ?? t("usage.who.other"),
      pct: Math.round(s.share * 100),
    }),
  }));
}
