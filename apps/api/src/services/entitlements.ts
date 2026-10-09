import { and, desc, eq } from "drizzle-orm";
import type { Limits, Plan, PlanStatus } from "@tandem/protocol/plans";
import { signEntitlement } from "../auth/jwt";
import type { Deps } from "../deps";
import { subscriptions, user } from "../db/schema";
import type { OrgPolicy, OrgRole } from "@tandem/protocol/team";
import { orgs } from "../db/schema";
import { creditsFor } from "./credits";
import { membershipOf } from "./membership";
import { orgPlan, policyOf } from "./orgs";
import { effectivePlan, type SubscriptionRow } from "./plan";

export type MeResponse = {
  user: { id: string; email: string; name: string; emailVerified: boolean; role: string };
  plan: Plan;
  status: PlanStatus;
  limits: Limits;
  paid_until: string | null;
  cancel_at_period_end: boolean;
  credits: {
    monthly: number;
    purchased: number;
    resets_at: string | null;
    /** Team members: their monthly cap and what they've spent against it. */
    cap: number | null;
    spent: number | null;
  };
  flags: Record<string, boolean>;
  /** The org's admin policy while its Team plan is paid (F-BL-06). */
  policy: OrgPolicy | null;
  org: { id: string; name: string; role: OrgRole } | null;
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

/**
 * Recomputes the plan from the database. Client-sent claims are never read
 * (F-BE-04 AC4). A member of an org with a paid Team plan is on Team;
 * otherwise their own subscription decides.
 */
export async function planFor(deps: Deps, userId: string) {
  const sub = await latestSubscription(deps, userId);
  const membership = await membershipOf(deps.db, userId);
  if (membership) {
    const team = await orgPlan(deps, membership.orgId);
    if (team.effective.plan === "team") return { sub: team.sub, effective: team.effective, membership };
  }
  return { sub, effective: effectivePlan(sub as SubscriptionRow | null, deps.clock.now()), membership };
}

export async function buildMe(deps: Deps, userId: string): Promise<MeResponse | null> {
  const [u] = await deps.db.select().from(user).where(eq(user.id, userId)).limit(1);
  if (!u) return null;
  const { sub, effective, membership } = await planFor(deps, userId);
  const credits = await creditsFor(deps, userId);
  const policy = membership && effective.plan === "team" ? await policyOf(deps, membership.orgId) : null;
  let org: MeResponse["org"] = null;
  if (membership) {
    const [row] = await deps.db
      .select({ name: orgs.name })
      .from(orgs)
      .where(eq(orgs.id, membership.orgId))
      .limit(1);
    org = { id: membership.orgId, name: row?.name ?? "", role: membership.role };
  }
  const token = await signEntitlement(
    deps.keys,
    { sub: userId, plan: effective.plan, status: effective.status, limits: effective.limits, policy },
    deps.clock.nowSec(),
  );
  return {
    user: { id: u.id, email: u.email, name: u.name, emailVerified: u.emailVerified, role: u.role },
    plan: effective.plan,
    status: effective.status,
    limits: effective.limits,
    paid_until: effective.paidUntil?.toISOString() ?? null,
    cancel_at_period_end: sub?.cancelAtPeriodEnd ?? false,
    credits: {
      monthly: credits.monthly,
      purchased: credits.purchased,
      resets_at: credits.resetsAt,
      cap: credits.org?.cap ?? null,
      spent: credits.org ? credits.org.spent : null,
    },
    flags: deps.config.config.flags,
    policy,
    org,
    entitlement_token: token,
  };
}
