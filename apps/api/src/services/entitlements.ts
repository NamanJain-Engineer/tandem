import { and, desc, eq } from "drizzle-orm";
import type { Limits, Plan, PlanStatus } from "@tandem/protocol/plans";
import { signEntitlement } from "../auth/jwt";
import type { Deps } from "../deps";
import { subscriptions, user } from "../db/schema";
import { balance } from "./ledger";
import { effectivePlan, type SubscriptionRow } from "./plan";

export type MeResponse = {
  user: { id: string; email: string; name: string; emailVerified: boolean; role: string };
  plan: Plan;
  status: PlanStatus;
  limits: Limits;
  paid_until: string | null;
  cancel_at_period_end: boolean;
  credits: { monthly: number; purchased: number; resets_at: string | null };
  flags: Record<string, boolean>;
  policy: Record<string, unknown> | null;
  org: null;
  entitlement_token: string;
};

export async function latestSubscription(deps: Deps, userId: string) {
  const [sub] = await deps.db
    .select()
    .from(subscriptions)
    .where(and(eq(subscriptions.ownerKind, "user"), eq(subscriptions.ownerId, userId)))
    .orderBy(desc(subscriptions.updatedAt))
    .limit(1);
  return sub ?? null;
}

/** Recomputes the plan from the database. Client-sent claims are never read (F-BE-04 AC4). */
export async function planFor(deps: Deps, userId: string) {
  const sub = await latestSubscription(deps, userId);
  return { sub, effective: effectivePlan(sub as SubscriptionRow | null, deps.clock.now()) };
}

export async function buildMe(deps: Deps, userId: string): Promise<MeResponse | null> {
  const [u] = await deps.db.select().from(user).where(eq(user.id, userId)).limit(1);
  if (!u) return null;
  const { sub, effective } = await planFor(deps, userId);
  const credits = await balance(deps.db, { kind: "user", id: userId });
  const token = await signEntitlement(
    deps.keys,
    { sub: userId, plan: effective.plan, status: effective.status, limits: effective.limits, policy: null },
    deps.clock.nowSec(),
  );
  return {
    user: { id: u.id, email: u.email, name: u.name, emailVerified: u.emailVerified, role: u.role },
    plan: effective.plan,
    status: effective.status,
    limits: effective.limits,
    paid_until: effective.paidUntil?.toISOString() ?? null,
    cancel_at_period_end: sub?.cancelAtPeriodEnd ?? false,
    credits: { monthly: credits.monthly, purchased: credits.purchased, resets_at: credits.resetsAt },
    flags: deps.config.config.flags,
    policy: null,
    org: null,
    entitlement_token: token,
  };
}
