// Support tools (F-BE-12): look a user up, see their plan and ledger, grant
// credits and refund. Every write is an `admin_actions` row with the admin
// and the reason.
import { and, desc, eq, ilike, inArray, sql } from "drizzle-orm";
import type { Deps } from "../deps";
import { adminActions, creditBuckets, creditLedger, orgs, user } from "../db/schema";
import { newId } from "../lib/ids";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import { creditsFor, notifyCredits } from "./credits";
import { planFor } from "./entitlements";
import type { Owner } from "./ledger";
import { membershipOf } from "./membership";

export async function searchUsers(deps: Deps, q: string) {
  const term = q.trim();
  if (term.length < 2) return [];
  const rows = await deps.db
    .select({ id: user.id, email: user.email, name: user.name, role: user.role, createdAt: user.createdAt })
    .from(user)
    .where(ilike(user.email, `%${term.replace(/[%_\\]/g, (c) => `\\${c}`)}%`))
    .orderBy(user.email)
    .limit(20);
  return rows.map((r) => ({ ...r, createdAt: r.createdAt.toISOString() }));
}

const notFound = () => new ApiError(404, "NOT_FOUND", "Not Found", "There's no such user.");

/** Who pays for `userId` (their org's pool, else themselves). */
async function ownerOf(deps: Deps, userId: string): Promise<Owner> {
  const m = await membershipOf(deps.db, userId);
  return m ? { kind: "org", id: m.orgId } : { kind: "user", id: userId };
}

export async function userDetail(deps: Deps, userId: string) {
  const [u] = await deps.db.select().from(user).where(eq(user.id, userId)).limit(1);
  if (!u) throw notFound();
  const { sub, effective, membership } = await planFor(deps, userId);
  const credits = await creditsFor(deps, userId);
  const owner = await ownerOf(deps, userId);
  const buckets = await deps.db
    .select({ id: creditBuckets.id, kind: creditBuckets.kind })
    .from(creditBuckets)
    .where(and(eq(creditBuckets.ownerKind, owner.kind), eq(creditBuckets.ownerId, owner.id)));
  const kinds = new Map(buckets.map((b) => [b.id, b.kind]));
  const ledger = buckets.length
    ? await deps.db
        .select()
        .from(creditLedger)
        .where(inArray(creditLedger.bucketId, [...kinds.keys()]))
        .orderBy(desc(creditLedger.createdAt))
        .limit(50)
    : [];
  const actions = await deps.db
    .select({
      id: adminActions.id,
      action: adminActions.action,
      reason: adminActions.reason,
      meta: adminActions.meta,
      createdAt: adminActions.createdAt,
      adminEmail: user.email,
    })
    .from(adminActions)
    .leftJoin(user, eq(user.id, adminActions.adminId))
    .where(eq(adminActions.targetUserId, userId))
    .orderBy(desc(adminActions.createdAt))
    .limit(50);
  let org: { id: string; name: string; role: string } | null = null;
  if (membership) {
    const [o] = await deps.db.select({ name: orgs.name }).from(orgs).where(eq(orgs.id, membership.orgId)).limit(1);
    org = { id: membership.orgId, name: o?.name ?? "", role: membership.role };
  }
  return {
    user: {
      id: u.id,
      email: u.email,
      name: u.name,
      role: u.role,
      emailVerified: u.emailVerified,
      createdAt: u.createdAt.toISOString(),
      deletionRequestedAt: u.deletionRequestedAt?.toISOString() ?? null,
    },
    plan: effective.plan,
    status: effective.status,
    subscription: sub
      ? {
          id: sub.stripeSubscriptionId,
          plan: sub.plan,
          interval: sub.interval,
          status: sub.status,
          seats: sub.seats,
          currentPeriodEnd: sub.currentPeriodEnd.toISOString(),
          firstPaidAt: sub.firstPaidAt?.toISOString() ?? null,
          refundedAt: sub.refundedAt?.toISOString() ?? null,
        }
      : null,
    org,
    credits: { monthly: credits.monthly, purchased: credits.purchased, resetsAt: credits.resetsAt, payer: owner },
    ledger: ledger.map((l) => ({
      id: l.id,
      bucket: kinds.get(l.bucketId),
      delta: l.delta,
      reason: l.reason,
      feature: l.reason === "reserve" ? l.refType : null,
      note: l.note,
      createdAt: l.createdAt.toISOString(),
    })),
    actions: actions.map((a) => ({ ...a, createdAt: a.createdAt.toISOString() })),
  };
}

/**
 * Adds credits to the bucket that pays for `userId` (purchased by default, so
 * they never expire) with an `admin_grant` row naming the admin and reason.
 */
export async function grantCredits(
  deps: Deps,
  adminId: string,
  userId: string,
  amount: number,
  reason: string,
  kind: "monthly" | "purchased",
) {
  const [u] = await deps.db.select({ id: user.id }).from(user).where(eq(user.id, userId)).limit(1);
  if (!u) throw notFound();
  const owner = await ownerOf(deps, userId);
  const result = await deps.db.transaction(async (tx) => {
    await tx
      .insert(creditBuckets)
      .values({ id: newId("cb"), ownerKind: owner.kind, ownerId: owner.id, kind, balance: 0 })
      .onConflictDoNothing();
    const [bucket] = await tx
      .select()
      .from(creditBuckets)
      .where(and(eq(creditBuckets.ownerKind, owner.kind), eq(creditBuckets.ownerId, owner.id), eq(creditBuckets.kind, kind)))
      .for("update");
    if (!bucket) throw new Error("credit bucket missing");
    const entryId = newId("cl");
    await tx.insert(creditLedger).values({
      id: entryId,
      bucketId: bucket.id,
      userId,
      delta: amount,
      reason: "admin_grant",
      refType: "admin",
      adminId,
      note: reason,
    });
    await tx
      .update(creditBuckets)
      .set({ balance: sql`${creditBuckets.balance} + ${amount}` })
      .where(eq(creditBuckets.id, bucket.id));
    await tx.insert(adminActions).values({
      id: newId("adm"),
      adminId,
      action: "grant_credits",
      targetUserId: userId,
      reason,
      meta: { amount, bucket: kind, payer: `${owner.kind}:${owner.id}`, ledgerEntry: entryId },
    });
    return { ledgerEntry: entryId, balance: bucket.balance + amount };
  });
  logger.info({ admin: adminId, target: userId, amount, bucket: kind }, "admin credit grant");
  await notifyCredits(deps, owner);
  return result;
}

