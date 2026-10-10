import type { UsageReport } from "@tandem/protocol";
import { tokenCount } from "@tandem/ui";
import { t } from "../../i18n";

export type ShareCard = {
  title: string;
  total: string;
  range: string;
  /** Only engines with usage (F-AC-07). */
  engines: Array<{ label: string; tokens: string }>;
  /** Empty unless the user chose to include them. */
  workspaces: Array<{ name: string; tokens: string }>;
  footer: string;
};

/** What the share card says: totals, engines with non-zero usage, and workspace names only when included. */
export function shareCard(report: UsageReport, includeWorkspaces: boolean): ShareCard {
  return {
    title: t("usage.card.title"),
    total: t("usage.card.total", { tokens: tokenCount(report.total) }),
    range: t("usage.range", { since: report.since, until: report.until }),
    engines: report.engines
      .filter((e) => e.tokens > 0)
      .map((e) => ({ label: e.label, tokens: tokenCount(e.tokens) })),
    workspaces: includeWorkspaces
      ? report.workspaces.slice(0, 5).map((w) => ({ name: w.name, tokens: tokenCount(w.tokens) }))
      : [],
    footer: t("usage.card.footer"),
  };
}

const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** Draws the card on a canvas in the theme's own colours, as a PNG (base64, no data: prefix). */
export function drawShareCard(card: ShareCard, report: UsageReport): string {
  const w = 1200;
  const h = 630;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const g = canvas.getContext("2d");
  if (!g) throw new Error("no canvas");
  const font = css("--font-sans") || "sans-serif";
  g.fillStyle = css("--tn-surface-1");
  g.fillRect(0, 0, w, h);
  g.fillStyle = css("--tn-text-muted");
  g.font = `600 28px ${font}`;
  g.fillText(card.title, 64, 88);
  g.fillStyle = css("--tn-text");
  g.font = `700 72px ${font}`;
  g.fillText(card.total, 64, 180);
  g.fillStyle = css("--tn-text-subtle");
  g.font = `400 24px ${font}`;
  g.fillText(card.range, 64, 222);
  // The heatmap, 12 weeks by 7 days.
  const max = Math.max(1, ...report.days.map((d) => d.tokens));
  const lead = report.days[0] ? (new Date(`${report.days[0].date}T00:00:00Z`).getUTCDay() + 6) % 7 : 0;
  report.days.forEach((d, i) => {
    const cell = i + lead;
    const x = 64 + Math.floor(cell / 7) * 26;
    const y = 270 + (cell % 7) * 26;
    const r = d.tokens / max;
    g.globalAlpha = d.tokens <= 0 ? 1 : r > 0.75 ? 1 : r > 0.5 ? 0.75 : r > 0.25 ? 0.5 : 0.25;
    g.fillStyle = d.tokens <= 0 ? css("--tn-surface-3") : css("--tn-accent");
    g.fillRect(x, y, 22, 22);
  });
  g.globalAlpha = 1;
  let y = 290;
  g.font = `500 26px ${font}`;
  for (const e of card.engines.slice(0, 6)) {
    g.fillStyle = css("--tn-text");
    g.fillText(e.label, 520, y);
    g.fillStyle = css("--tn-text-muted");
    g.fillText(e.tokens, 900, y);
    y += 40;
  }
  for (const ws of card.workspaces) {
    g.fillStyle = css("--tn-text-subtle");
    g.fillText(`${ws.name} · ${ws.tokens}`, 520, y);
    y += 36;
  }
  g.fillStyle = css("--tn-text-subtle");
  g.font = `400 20px ${font}`;
  g.fillText(card.footer, 64, h - 48);
  return canvas.toDataURL("image/png").replace(/^data:image\/png;base64,/, "");
}
