// Tandem tables (ARCHITECTURE §14.2). better-auth tables are in auth-schema.ts.
import { sql } from "drizzle-orm";
import {
  bigint,
  boolean,
  check,
  date,
  index,
  integer,
  jsonb,
  pgTable,
  primaryKey,
  text,
  timestamp,
  uniqueIndex,
} from "drizzle-orm/pg-core";
import { user } from "./auth-schema";

export * from "./auth-schema";

export const refreshTokens = pgTable(
  "refresh_tokens",
  {
    id: text("id").primaryKey(),
    familyId: text("family_id").notNull(),
    userId: text("user_id")
      .notNull()
      .references(() => user.id, { onDelete: "cascade" }),
    tokenHash: text("token_hash").notNull().unique(),
    deviceFingerprint: text("device_fingerprint").notNull(),
    deviceName: text("device_name"),
    platform: text("platform"),
    appVersion: text("app_version"),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
    expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
    rotatedAt: timestamp("rotated_at", { withTimezone: true }),
    revokedAt: timestamp("revoked_at", { withTimezone: true }),
    lastUsedAt: timestamp("last_used_at", { withTimezone: true }),
  },
  (t) => [index("rt_family").on(t.familyId), index("rt_user").on(t.userId)],
);
export const desktopLoginCodes = pgTable("desktop_login_codes", {
  codeHash: text("code_hash").primaryKey(),
  userId: text("user_id")
    .notNull()
    .references(() => user.id, { onDelete: "cascade" }),
  challenge: text("challenge").notNull(),
  redirectUri: text("redirect_uri").notNull(),
  deviceFingerprint: text("device_fingerprint").notNull(),
  deviceName: text("device_name"),
  platform: text("platform"),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  usedAt: timestamp("used_at", { withTimezone: true }),
});
export const deviceCodes = pgTable("device_codes", {
  deviceCodeHash: text("device_code_hash").primaryKey(),
  userCode: text("user_code").notNull().unique(),
  userId: text("user_id"),
  status: text("status", { enum: ["pending", "approved", "denied", "consumed"] })
    .notNull()
    .default("pending"),
  intervalSec: integer("interval_sec").notNull().default(5),
  lastPollAt: timestamp("last_poll_at", { withTimezone: true }),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  clientInfo: jsonb("client_info"),
});
export const orgs = pgTable("orgs", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  ownerId: text("owner_id").notNull(),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});
export const orgMembers = pgTable(
  "org_members",
  {
    orgId: text("org_id")
      .notNull()
      .references(() => orgs.id, { onDelete: "cascade" }),
    userId: text("user_id")
      .notNull()
      .references(() => user.id, { onDelete: "cascade" }),
    role: text("role", { enum: ["owner", "admin", "member"] }).notNull(),
    monthlyCap: integer("monthly_cap"),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  },
  (t) => [primaryKey({ columns: [t.orgId, t.userId] }), uniqueIndex("one_org_per_user").on(t.userId)],
);
export const orgInvites = pgTable("org_invites", {
  id: text("id").primaryKey(),
  orgId: text("org_id").notNull(),
  email: text("email").notNull(),
  role: text("role").notNull(),
  tokenHash: text("token_hash").notNull().unique(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  acceptedAt: timestamp("accepted_at", { withTimezone: true }),
});
export const orgPolicies = pgTable("org_policies", {
  orgId: text("org_id")
    .primaryKey()
    .references(() => orgs.id, { onDelete: "cascade" }),
  policy:
    jsonb(
      "policy",
    ).notNull() /* {allowedEngines?, allowedPlugins?, maxAccess?, forceTelemetryOff, disableAuto, disableConductor, disablePhone, auditEnabled} */,
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull(),
});
export const billingCustomers = pgTable(
  "billing_customers",
  {
    ownerKind: text("owner_kind", { enum: ["user", "org"] }).notNull(),
    ownerId: text("owner_id").notNull(),
    customerId: text("customer_id").notNull().unique(),
  },
  (t) => [primaryKey({ columns: [t.ownerKind, t.ownerId] })],
);
export const subscriptions = pgTable(
  "subscriptions",
  {
    id: text("id").primaryKey(),
    stripeSubscriptionId: text("stripe_subscription_id").notNull().unique(),
    ownerKind: text("owner_kind", { enum: ["user", "org"] }).notNull(),
    ownerId: text("owner_id").notNull(),
    plan: text("plan", { enum: ["pro", "team"] }).notNull(),
    interval: text("interval", { enum: ["month", "year"] }).notNull(),
    status: text("status", {
      enum: ["trialing", "active", "past_due", "canceled", "unpaid", "incomplete"],
    }).notNull(),
    seats: integer("seats").notNull().default(1),
    currentPeriodStart: timestamp("cps", { withTimezone: true }).notNull(),
    currentPeriodEnd: timestamp("cpe", { withTimezone: true }).notNull(),
    cancelAtPeriodEnd: boolean("cancel_at_period_end").notNull().default(false),
    trialEnd: timestamp("trial_end", { withTimezone: true }),
    pastDueSince: timestamp("past_due_since", { withTimezone: true }),
    firstPaidAt: timestamp("first_paid_at", { withTimezone: true }),
    updatedAt: timestamp("updated_at", { withTimezone: true }).notNull(),
  },
  (t) => [index("sub_owner").on(t.ownerKind, t.ownerId)],
);
export const webhookEvents = pgTable("webhook_events", {
  id: text("id").primaryKey() /* Stripe event id */,
  type: text("type").notNull(),
  receivedAt: timestamp("received_at", { withTimezone: true }).notNull().defaultNow(),
  processedAt: timestamp("processed_at", { withTimezone: true }),
  error: text("error"),
});
export const creditBuckets = pgTable(
  "credit_buckets",
  {
    id: text("id").primaryKey(),
    ownerKind: text("owner_kind", { enum: ["user", "org"] }).notNull(),
    ownerId: text("owner_id").notNull(),
    kind: text("kind", { enum: ["monthly", "purchased"] }).notNull(),
    balance: bigint("balance", { mode: "number" }).notNull().default(0),
    periodEnd: timestamp("period_end", { withTimezone: true }),
  },
  (t) => [
    uniqueIndex("bucket_owner_kind").on(t.ownerKind, t.ownerId, t.kind),
    check("balance_nonneg", sql`${t.balance} >= 0`),
  ],
);
export const creditLedger = pgTable(
  "credit_ledger",
  {
    id: text("id").primaryKey(),
    bucketId: text("bucket_id")
      .notNull()
      .references(() => creditBuckets.id),
    userId: text("user_id"),
    delta: bigint("delta", { mode: "number" }).notNull(),
    reason: text("reason", {
      enum: [
        "reserve",
        "settle_refund",
        "refund",
        "monthly_grant",
        "monthly_reset",
        "pack_purchase",
        "admin_grant",
        "plan_refund",
        "expire_sweep",
      ],
    }).notNull(),
    refType: text("ref_type"),
    refId: text("ref_id"),
    reservationId: text("reservation_id"),
    adminId: text("admin_id"),
    note: text("note"),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  },
  (t) => [index("ledger_bucket_time").on(t.bucketId, t.createdAt)],
); // append-only: no UPDATE/DELETE grants for the app role
export const creditReservations = pgTable(
  "credit_reservations",
  {
    id: text("id").primaryKey(),
    userId: text("user_id").notNull(),
    ownerKind: text("owner_kind").notNull(),
    ownerId: text("owner_id").notNull(),
    feature: text("feature").notNull(),
    amount: integer("amount").notNull(),
    allocations: jsonb("allocations").notNull() /* [{bucketId, amount}] */,
    state: text("state", { enum: ["held", "settled", "refunded"] }).notNull(),
    settledAmount: integer("settled_amount"),
    idempotencyKey: text("idempotency_key"),
    expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  },
  (t) => [
    uniqueIndex("resv_idem").on(t.userId, t.idempotencyKey),
    index("resv_expiry").on(t.state, t.expiresAt),
  ],
);
export const memberUsage = pgTable(
  "member_usage",
  {
    orgId: text("org_id").notNull(),
    userId: text("user_id").notNull(),
    period: text("period").notNull(),
    spent: integer("spent").notNull().default(0),
  },
  (t) => [primaryKey({ columns: [t.orgId, t.userId, t.period] })],
);
export const dailyUsage = pgTable(
  "daily_usage",
  {
    userId: text("user_id").notNull(),
    feature: text("feature").notNull(),
    day: date("day").notNull(),
    units: integer("units").notNull().default(0) /* stt: seconds; rewrite: calls */,
  },
  (t) => [primaryKey({ columns: [t.userId, t.feature, t.day] })],
);
export const aiRequests = pgTable("ai_requests", {
  id: text("id").primaryKey(),
  userId: text("user_id").notNull(),
  feature: text("feature").notNull(),
  model: text("model").notNull(),
  tokensIn: integer("tokens_in"),
  tokensOut: integer("tokens_out"),
  latencyMs: integer("latency_ms"),
  status: integer("status").notNull(),
  credits: integer("credits").notNull().default(0),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});
export const voiceSessions = pgTable("voice_sessions", {
  id: text("id").primaryKey(),
  userId: text("user_id").notNull(),
  provider: text("provider").notNull(),
  reservationId: text("reservation_id").notNull(),
  maxSeconds: integer("max_seconds").notNull(),
  startedAt: timestamp("started_at", { withTimezone: true }).notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  settledAt: timestamp("settled_at", { withTimezone: true }),
  seconds: integer("seconds"),
});
export const dictionarySync = pgTable(
  "dictionary_sync",
  {
    userId: text("user_id").notNull(),
    ruleId: text("rule_id").notNull(),
    payload: jsonb("payload").notNull(),
    rev: bigint("rev", { mode: "number" }).notNull(),
    deleted: boolean("deleted").notNull().default(false),
    updatedAt: timestamp("updated_at", { withTimezone: true }).notNull(),
  },
  (t) => [primaryKey({ columns: [t.userId, t.ruleId] }), index("dict_rev").on(t.userId, t.rev)],
);
export const pairings = pgTable("pairings", {
  id: text("id").primaryKey(),
  userId: text("user_id").notNull(),
  pairingTokenHash: text("pairing_token_hash").notNull().unique(),
  deviceLabel: text("device_label"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  revokedAt: timestamp("revoked_at", { withTimezone: true }),
});
export const auditEvents = pgTable(
  "audit_events",
  {
    id: text("id").primaryKey(),
    orgId: text("org_id").notNull(),
    actorUserId: text("actor_user_id"),
    event: text("event").notNull(),
    meta: jsonb("meta").notNull(),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  },
  (t) => [index("audit_org_time").on(t.orgId, t.createdAt)],
);
export const adminActions = pgTable("admin_actions", {
  id: text("id").primaryKey(),
  adminId: text("admin_id").notNull(),
  action: text("action").notNull(),
  targetUserId: text("target_user_id"),
  reason: text("reason").notNull(),
  meta: jsonb("meta"),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
});
export const releases = pgTable(
  "releases",
  {
    version: text("version").notNull(),
    channel: text("channel", { enum: ["stable", "beta"] }).notNull(),
    target: text("target").notNull() /* darwin|windows|linux */,
    arch: text("arch").notNull() /* universal|x86_64 */,
    url: text("url").notNull(),
    signature: text("signature").notNull(),
    sha256: text("sha256").notNull(),
    notes: text("notes").notNull(),
    pubDate: timestamp("pub_date", { withTimezone: true }).notNull(),
  },
  (t) => [primaryKey({ columns: [t.version, t.channel, t.target, t.arch] })],
);
