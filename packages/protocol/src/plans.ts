// The single source for plan prices (PRODUCT_SPEC §3.1), the entitlement
// matrix (§3.2) and credit prices (§3.3). The API, the web pricing page,
// llms.txt and docs import it.

export type Plan = "free" | "pro" | "team";
export type BillingInterval = "month" | "year";
export type SubscriptionStatus = "trialing" | "active" | "past_due" | "canceled" | "unpaid" | "incomplete";
/** What the client shows: `none` for Free with no subscription. */
export type PlanStatus = SubscriptionStatus | "none";

/** `null` means unlimited. */
export type Limits = {
  panes: number | null;
  workspaces: number | null;
  threadGrid: number;
  /** Teammates besides the built-in Lead. */
  teammates: number | null;
  routines: boolean;
  backgroundRoutines: boolean;
  taskExecution: boolean;
  pluginConnections: number | null;
  accountsPerEngine: number | null;
  cloudStt: boolean;
  enhance: boolean;
  conductor: boolean;
  auto: boolean;
  phone: boolean;
  admin: boolean;
  /** Chat engines allowed in Thread mode; `null` means all. */
  chatEngines: string[] | null;
  creditPacks: boolean;
  /** The dictionary syncs across devices (F-DI-09). */
  dictionarySync: boolean;
  /** Full-text search across engine sessions (F-TH-17). */
  sessionSearch: boolean;
  /** Worktrees across a multi-repo folder (F-CD-22). */
  multiRepo: boolean;
};

export const LIMITS: Record<Plan, Limits> = {
  free: {
    panes: 4,
    workspaces: 3,
    threadGrid: 1,
    teammates: 1,
    routines: false,
    backgroundRoutines: false,
    taskExecution: false,
    pluginConnections: 1,
    accountsPerEngine: 1,
    cloudStt: false,
    enhance: false,
    conductor: false,
    auto: false,
    phone: false,
    admin: false,
    chatEngines: ["claude", "codex"],
    creditPacks: false,
    dictionarySync: false,
    sessionSearch: false,
    multiRepo: false,
  },
  pro: {
    panes: 64,
    workspaces: null,
    threadGrid: 6,
    teammates: null,
    routines: true,
    backgroundRoutines: true,
    taskExecution: true,
    pluginConnections: null,
    accountsPerEngine: null,
    cloudStt: true,
    enhance: true,
    conductor: true,
    auto: true,
    phone: true,
    admin: false,
    chatEngines: null,
    creditPacks: true,
    dictionarySync: true,
    sessionSearch: true,
    multiRepo: true,
  },
  team: {
    panes: null,
    workspaces: null,
    threadGrid: 6,
    teammates: null,
    routines: true,
    backgroundRoutines: true,
    taskExecution: true,
    pluginConnections: null,
    accountsPerEngine: null,
    cloudStt: true,
    enhance: true,
    conductor: true,
    auto: true,
    phone: true,
    admin: true,
    chatEngines: null,
    creditPacks: true,
    dictionarySync: true,
    sessionSearch: true,
    multiRepo: true,
  },
};

/** List prices in US cents. */
export const PRICES = {
  pro: { month: 2900, year: 29000, trialDays: 7 },
  team: { month: 4900, year: 49000, trialDays: 14, minSeats: 2 },
} as const;

export const MONTHLY_CREDITS = 10_000;

/** One credit is US$0.001 of list value. */
export const CREDIT_PACKS = {
  "5k": { credits: 5_000, priceCents: 500 },
  "12k": { credits: 12_000, priceCents: 1000 },
  "30k": { credits: 30_000, priceCents: 2000 },
} as const;
export type CreditPack = keyof typeof CREDIT_PACKS;

/** Credit prices per hosted feature (§3.3). Served from remote config. */
export const CREDIT_PRICES = {
  stt_15s: 2,
  rewrite: 3,
  voice_10s: 20,
  resolve: 1,
  auto_turn: 6,
  teammate_draft: 10,
  title: 0,
} as const;

export const DAILY_CAPS = { stt_minutes: 120, rewrite_calls: 300 } as const;

/** Past-due accounts keep paid limits this long, then drop to Free. */
export const PAST_DUE_GRACE_DAYS = 7;
