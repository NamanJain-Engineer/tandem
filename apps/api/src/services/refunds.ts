// Refunds (F-BL-05, ARCHITECTURE §14.7): a first Pro or Team charge within
// 14 days ends paid access and zeroes the monthly credits; an unused credit
// pack is refundable. Each refund is an admin action with a reason.
import { and, desc, eq } from "drizzle-orm";
import type { Deps } from "../deps";
import { adminActions, creditBuckets, creditLedger, subscriptions } from "../db/schema";
import { newId } from "../lib/ids";
import { ApiError } from "../lib/problem";
import { latestSubscription } from "./entitlements";
import { zeroMonthly, type Owner } from "./ledger";
import { membershipOf, usersOf } from "./membership";
import { orgSubscription } from "./orgs";

export const REFUND_WINDOW_DAYS = 14;

/** The subscription a refund for `userId` applies to: theirs, or the org they own. */
async function refundableSubscription(deps: Deps, userId: string) {
  const own = await latestSubscription(deps, userId);
  if (own) return { sub: own, owner: { kind: "user", id: userId } as Owner };
  const m = await membershipOf(deps.db, userId);
  if (m?.role === "owner") {
    const sub = await orgSubscription(deps, m.orgId);
    if (sub) return { sub, owner: { kind: "org", id: m.orgId } as Owner };
  }
  return null;
}

async function notify(deps: Deps, owner: Owner) {
  for (const id of await usersOf(deps.db, owner)) await deps.events.publish(id, "entitlement");
}

export async function refundPlan(deps: Deps, adminId: string, userId: string, reason: string, force = false) {
  const found = await refundableSubscription(deps, userId);
  if (!found || found.sub.refundedAt) {
    throw new ApiError(409, "NOTHING_TO_REFUND", "Conflict", "There's no subscription charge to refund.");
  }
  const { sub, owner } = found;
  if (!sub.firstPaidAt) {
    throw new ApiError(409, "NOTHING_TO_REFUND", "Conflict", "The subscription hasn't been charged yet.");
  }
  const age = deps.clock.nowMs() - sub.firstPaidAt.getTime();
  if (age > REFUND_WINDOW_DAYS * 86_400_000 && !force) {
    throw new ApiError(
      409,
      "REFUND_WINDOW_CLOSED",
      "Conflict",
      `The first charge was more than ${REFUND_WINDOW_DAYS} days ago. Send force to refund anyway.`,
    );
  }
  await deps.billing.refundLatestCharge(sub.stripeSubscriptionId);
  if (sub.status !== "canceled") await deps.billing.cancelNow(sub.stripeSubscriptionId);
  const now = deps.clock.now();
  await deps.db.transaction(async (tx) => {
    await tx
      .update(subscriptions)
      .set({
        status: "canceled",
        refundedAt: now,
        cancelAtPeriodEnd: false,
        currentPeriodEnd: now,
        updatedAt: now,
      })
      .where(eq(subscriptions.id, sub.id));
    await zeroMonthly(tx, owner, sub.stripeSubscriptionId, "plan_refund");
    await tx.insert(adminActions).values({
      id: newId("adm"),
      adminId,
      action: "refund_plan",
      targetUserId: userId,
      reason,
      meta: { subscription: sub.stripeSubscriptionId, plan: sub.plan, forced: force },
    });
  });
  await notify(deps, owner);
  return { refunded: "plan" as const, plan: sub.plan };
}

/** Refunds the latest credit pack while its credits are still unspent. */
export async function refundPack(deps: Deps, adminId: string, userId: string, reason: string) {
  const m = await membershipOf(deps.db, userId);
  const owner: Owner = m && m.role !== "member" ? { kind: "org", id: m.orgId } : { kind: "user", id: userId };
  const refunded = await deps.db.transaction(async (tx) => {
    const [bucket] = await tx
      .select()
      .from(creditBuckets)
      .where(
        and(
          eq(creditBuckets.ownerKind, owner.kind),
          eq(creditBuckets.ownerId, owner.id),
          eq(creditBuckets.kind, "purchased"),
        ),
      )
      .for("update");
    if (!bucket)
      throw new ApiError(409, "NOTHING_TO_REFUND", "Conflict", "There's no credit pack to refund.");
    const rows = await tx
      .select()
      .from(creditLedger)
      .where(eq(creditLedger.bucketId, bucket.id))
      .orderBy(desc(creditLedger.createdAt));
    const done = new Set(rows.filter((r) => r.reason === "pack_refund").map((r) => r.refId));
    const pack = rows.find((r) => r.reason === "pack_purchase" && !done.has(r.refId));
    if (!pack?.refId)
      throw new ApiError(409, "NOTHING_TO_REFUND", "Conflict", "There's no credit pack to refund.");
    if (bucket.balance < pack.delta) {
      throw new ApiError(409, "PACK_USED", "Conflict", "Some of the pack's credits were already spent.", {
        balance: bucket.balance,
        pack: pack.delta,
      });
    }
    await tx.insert(creditLedger).values({
      id: newId("cl"),
      bucketId: bucket.id,
      delta: -pack.delta,
      reason: "pack_refund",
      refType: "checkout",
      refId: pack.refId,
      adminId,
      note: reason,
    });
    await tx
      .update(creditBuckets)
      .set({ balance: bucket.balance - pack.delta })
      .where(eq(creditBuckets.id, bucket.id));
    await tx.insert(adminActions).values({
      id: newId("adm"),
      adminId,
      action: "refund_pack",
      targetUserId: userId,
      reason,
      meta: { checkout: pack.refId, credits: pack.delta },
    });
    return { session: pack.refId, credits: pack.delta };
  });
  // The ledger moved first: a failed refund at the provider is retried by hand, never double-credited.
  await deps.billing.refundPayment(refunded.session);
  for (const id of await usersOf(deps.db, owner)) await deps.events.publish(id, "credits");
  return { refunded: "pack" as const, credits: refunded.credits };
}
