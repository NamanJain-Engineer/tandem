import { t } from "../../i18n";

/** "670 MB", "1.5 GB". */
export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return t("dictate.size.gb", { n: Math.round(bytes / 1e8) / 10 });
  return t("dictate.size.mb", { n: Math.max(1, Math.round(bytes / 1e6)) });
}

/** "5h 48m", "14s". */
export function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return t("dictate.duration.s", { s });
  const m = Math.floor(s / 60);
  if (m < 60) return t("dictate.duration.m", { m, s: s % 60 });
  return t("dictate.duration.h", { h: Math.floor(m / 60), m: m % 60 });
}
