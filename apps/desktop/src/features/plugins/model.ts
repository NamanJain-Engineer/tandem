import type { PluginApprovalDetail, PluginCard, RiskClass } from "@tandem/protocol";
import type { BadgeVariant } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";

/** UI-90's category filter, in order (keys 1–7). */
export const CATEGORIES = ["all", "dev", "business", "communication", "research", "media", "growth"] as const;
export type Category = (typeof CATEGORIES)[number];

/** Cards the search and category keep. */
export function filterCards(cards: PluginCard[], query: string, category: Category): PluginCard[] {
  const q = query.trim().toLowerCase();
  return cards.filter(
    (c) =>
      (category === "all" || c.category === category) &&
      (!q || `${c.label} ${c.description} ${c.pluginId}`.toLowerCase().includes(q)),
  );
}

/** Connected first (and those needing attention), then the rest A–Z. */
export function splitCards(cards: PluginCard[]): { connected: PluginCard[]; browse: PluginCard[] } {
  const isConnected = (c: PluginCard) => c.state === "connected" || c.state === "needs_attention";
  const byLabel = (a: PluginCard, b: PluginCard) => a.label.localeCompare(b.label);
  return {
    connected: cards.filter(isConnected).sort(byLabel),
    // Ready before preview, so what can be connected comes first.
    browse: cards
      .filter((c) => !isConnected(c))
      .sort((a, b) => (a.status === b.status ? byLabel(a, b) : a.status === "ready" ? -1 : 1)),
  };
}

/** UI-93's row names for the risk classes. */
export function riskLabel(r: RiskClass): string {
  return t(`plugins.risk.${r}` as MessageKey);
}

export function riskVariant(r: RiskClass): BadgeVariant {
  switch (r) {
    case "read":
      return "neutral";
    case "write":
    case "credit_consuming":
      return "waiting";
    case "external_publish":
      return "accent";
    default:
      return "error";
  }
}

/** The card fields of a plugin approval, or null when `detail` isn't one. */
export function parseApproval(detail: string): PluginApprovalDetail | null {
  try {
    const v = JSON.parse(detail) as Partial<PluginApprovalDetail>;
    if (typeof v?.pluginId === "string" && typeof v.action === "string" && typeof v.request === "string") {
      return v as PluginApprovalDetail;
    }
  } catch {
    // Not a plugin approval.
  }
  return null;
}

/** The connect sheet's methods for a card: what this build offers (D-960). */
export function methods(card: PluginCard): Array<"oauth" | "key" | "none"> {
  return card.methods.filter((m): m is "oauth" | "key" | "none" => m !== "local");
}
