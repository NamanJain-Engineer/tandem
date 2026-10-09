import { CREDIT_PRICES } from "@tandem/protocol/plans";

export type LedgerRow = {
  id: string;
  bucket: "monthly" | "purchased";
  delta: number;
  reason: string;
  feature: string | null;
  createdAt: string;
};

export type Credits = {
  balance: { monthly: number; purchased: number; resetsAt: string | null };
  org: { id: string; cap: number | null; spent: number } | null;
  ledger: LedgerRow[];
};

const FEATURES: Record<string, string> = {
  stt: "Cloud dictation",
  rewrite: "Enhance or Polish",
  teammate_draft: "Teammate draft",
};

const REASONS: Record<string, string> = {
  reserve: "Used",
  settle_refund: "Unused part returned",
  refund: "Returned (the call failed)",
  expire_sweep: "Returned (the call never finished)",
  monthly_grant: "Monthly credits",
  monthly_reset: "Last month's credits expired",
  pack_purchase: "Credit pack",
  pack_refund: "Credit pack refunded",
  plan_refund: "Plan refunded",
  admin_grant: "Adjustment",
};

export function ledgerLabel(row: LedgerRow): string {
  const reason = REASONS[row.reason] ?? row.reason;
  const feature = row.feature ? FEATURES[row.feature] : undefined;
  return feature ? `${reason} · ${feature}` : reason;
}

/** The hosted features that cost credits today, and their prices (from plans.ts, PRODUCT_SPEC §3.3). */
export const PRICE_LIST = [
  { feature: "Cloud dictation", price: `${CREDIT_PRICES.stt_15s} per started 15 seconds of audio` },
  { feature: "Enhance or Polish", price: `${CREDIT_PRICES.rewrite} per call` },
  { feature: "Create a teammate with AI", price: `${CREDIT_PRICES.teammate_draft} per draft` },
  { feature: "Thread titles", price: "Free" },
] as const;

export const formatCredits = (v: number) => new Intl.NumberFormat("en-US").format(v);
