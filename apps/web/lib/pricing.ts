import registry from "../../../crates/tandem-engines/registry.json";
import { LIMITS, MONTHLY_CREDITS, type Plan as PlanId } from "@tandem/protocol/plans";

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
      { text: "Every supported engine in terminal panes, on your own accounts" },
      { text: "One account per engine" },
      { text: "Dashboard, notifications and crash-safe restore" },
      { text: "Thread mode with Claude Code and Codex, one thread at a time" },
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
      { text: `Up to ${LIMITS.pro.panes} live panes, and unlimited workspaces` },
      { text: "Thread mode with every chat engine: Cursor Agent, Grok and OpenCode too" },
      { text: "Up to 6 threads side by side" },
      { text: "Several accounts per engine" },
    ],
  },
  {
    id: "team",
    name: "Team",
    price: "$49",
    cadence: "per seat per month, or $490 a year · 2 seats minimum",
    summary: "For teams that share agents, policy and credits.",
    trial: "14-day free trial · 14-day money-back",
    features: [
      { text: "Everything in Pro" },
      ...(LIMITS.team.panes === null ? [{ text: "No limit on live panes" }] : []),
      { text: `${MONTHLY_CREDITS.toLocaleString("en-US")} credits a month per seat, shared by the team` },
    ],
  },
];

/** 1 credit is US$0.001 of list value (PRODUCT_SPEC §3.3). */
export const CREDIT_VALUE_USD = 0.001;

/** What a hosted feature costs. `key` is the feature's entry in the API's
 * signed remote config (and `CREDIT_PRICES` in plans.ts); a build-time test
 * keeps the three equal (F-WB-01 AC3). `feature` is its PRODUCT_SPEC id: only
 * features ticked in docs/REMAINING.md are listed (a test checks). */
export type CreditRow = {
  key: "title" | "rewrite";
  feature: string;
  label: string;
  unit: string;
  credits: number;
};

export const CREDIT_ROWS: readonly CreditRow[] = [
  { key: "title", feature: "F-BE-06", label: "Thread titles", unit: "per title", credits: 0 },
  { key: "rewrite", feature: "F-DI-10", label: "Dictate Enhance and Polish", unit: "per use", credits: 3 },
];

/** The plan comparison on /pricing (§6.16) and the docs' limits table, both
 * generated from `LIMITS`, `MONTHLY_CREDITS` and the engine registry. Rows
 * in the "switch on" group name their PRODUCT_SPEC feature: only features
 * ticked in docs/REMAINING.md are listed (a test checks). */
export type PlanCell = string | boolean;
export type PlanRow = { key: string; label: string; feature?: string; value: (p: PlanId) => PlanCell };
export type PlanGroup = { title: string; note?: string; rows: readonly PlanRow[] };

const count = (n: number | null): string => (n === null ? "No limit" : n.toLocaleString("en-US"));
const engineNames = (ids: readonly string[] | null): string =>
  ids === null ? "All" : ids.map((id) => registry.find((e) => e.id === id)?.label ?? id).join(", ");

export const PLAN_GROUPS: readonly PlanGroup[] = [
  {
    title: "Workspace",
    rows: [
      { key: "panes", label: "Live terminal panes", value: (p) => count(LIMITS[p].panes) },
      { key: "workspaces", label: "Workspaces", value: (p) => count(LIMITS[p].workspaces) },
      { key: "threadGrid", label: "Threads side by side", value: (p) => count(LIMITS[p].threadGrid) },
      { key: "chatEngines", label: "Thread mode engines", value: (p) => engineNames(LIMITS[p].chatEngines) },
    ],
  },
  {
    title: "Accounts and teammates",
    rows: [
      { key: "accountsPerEngine", label: "Accounts per engine", value: (p) => count(LIMITS[p].accountsPerEngine) },
      { key: "teammates", label: "Teammates besides Lead", value: (p) => count(LIMITS[p].teammates) },
      { key: "pluginConnections", label: "Plugin connections", value: (p) => count(LIMITS[p].pluginConnections) },
    ],
  },
  {
    title: "Switch on with Vibezeo accounts",
    note: "Built into the app; switches on with Vibezeo accounts.",
    rows: [
      { key: "routines", label: "Routines", feature: "F-RT-01", value: (p) => LIMITS[p].routines },
      { key: "taskExecution", label: "Tasks run in their own lanes", feature: "F-TK-04", value: (p) => LIMITS[p].taskExecution },
      { key: "cloudStt", label: "Cloud dictation", feature: "F-DI-05", value: (p) => LIMITS[p].cloudStt },
      { key: "enhance", label: "Enhance and Polish", feature: "F-DI-10", value: (p) => LIMITS[p].enhance },
      {
        key: "monthlyCredits",
        label: "Monthly credits",
        value: (p) =>
          p === "free"
            ? false
            : p === "team"
              ? `${MONTHLY_CREDITS.toLocaleString("en-US")} per seat, shared`
              : MONTHLY_CREDITS.toLocaleString("en-US"),
      },
      { key: "creditPacks", label: "Credit packs", feature: "F-BL-03", value: (p) => LIMITS[p].creditPacks },
    ],
  },
];

export const PLAN_ROWS: readonly PlanRow[] = PLAN_GROUPS.flatMap((g) => g.rows);
