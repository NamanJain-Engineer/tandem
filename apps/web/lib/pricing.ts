/** The plans (PRODUCT_SPEC §3.1–3.3). Free is available now; Pro and Team start with accounts: the
 * site shows the plans so people know what Vibezeo will cost. M15 generates
 * /pricing from `packages/protocol/src/plans.ts` and checks it against the
 * API's config (F-WB-01 AC3). */

export type PlanFeature = { text: string; planned?: boolean };

export type Plan = {
  id: "free" | "pro" | "team";
  name: string;
  price: string;
  cadence: string;
  summary: string;
  trial?: string;
  features: readonly PlanFeature[];
};

export const PLANS: readonly Plan[] = [
  {
    id: "free",
    name: "Free",
    price: "$0",
    cadence: "forever",
    summary: "Run your own coding agents side by side.",
    features: [
      { text: "Up to 4 live terminal and agent panes" },
      { text: "3 workspaces" },
      { text: "Every supported engine, on your own accounts" },
      { text: "Dashboard, notifications and crash-safe restore" },
      { text: "Thread mode, one thread at a time" },
      { text: "Signed updates, always, even after a paid plan ends" },
    ],
  },
  {
    id: "pro",
    name: "Pro",
    price: "$29",
    cadence: "per month, or $290 a year",
    summary: "For developers who run many agents every day.",
    trial: "7-day free trial · 14-day money-back on the first charge",
    features: [
      { text: "Everything in Free" },
      { text: "Unlimited panes and workspaces" },
      { text: "Up to 6 threads side by side" },
    ],
  },
  {
    id: "team",
    name: "Team",
    price: "$49",
    cadence: "per seat per month, or $490 a year · 2 seats minimum",
    summary: "For teams that share agents, policy and credits.",
    trial: "14-day free trial · 14-day money-back",
    features: [{ text: "Everything in Pro" }],
  },
];

/** 1 credit is US$0.001 of list value (PRODUCT_SPEC §3.3). */
export const CREDIT_VALUE_USD = 0.001;
