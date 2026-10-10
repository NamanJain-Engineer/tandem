// Carries out account deletions (F-TL-05): within 30 days of "Delete
// account", the account and its personal data are gone. Money records stay
// (the ledger is append-only), keyed by an id that no longer leads to a
// person once the user row is gone (D-813).
import { and, eq, inArray, isNotNull, lte, ne, sql } from "drizzle-orm";
import type { Deps } from "../deps";
import {
  aiRequests,
  billingCustomers,
  dailyUsage,
  deviceCodes,
  dictionarySync,
  orgInvites,
  orgMembers,
  orgs,
  pairings,
  subscriptions,
  user,
  voiceSessions,
} from "../db/schema";
import { logger } from "../lib/logger";
import { withLock } from "./lock";

export const PURGE_INTERVAL_MS = 60 * 60_000;
/** A request is carried out after 29 days: inside the promised 30, with a day to spare for the hourly pass. */
export const PURGE_AFTER_MS = 29 * 86_400_000;
const LOCK = "job:deletion";

export type PurgeResult = { purged: number; blocked: number; failed: number };

/** Whether `userId` owns a Team that still has other members (they must transfer it first). */
export async function ownsTeamWithMembers(db: Deps["db"], userId: string): Promise<boolean> {
  const rows = await db
    .select({ orgId: orgMembers.orgId })
    .from(orgMembers)
    .innerJoin(orgs, eq(orgs.id, orgMembers.orgId))
    .where(and(eq(orgs.ownerId, userId), ne(orgMembers.userId, userId)))
    .limit(1);
  return rows.length > 0;
}

/**
 * Cancels the subscriptions of the Teams `userId` owns, at the request: with
 * no other members (the request checks that first), they're the user's own (F-TL-05).
 */
export async function cancelOwnedTeams(deps: Deps, userId: string): Promise<void> {
  const owned = (await deps.db.select({ id: orgs.id }).from(orgs).where(eq(orgs.ownerId, userId))).map(
    (o) => o.id,
  );
  if (owned.length === 0) return;
  const live = await deps.db
    .select({ id: subscriptions.stripeSubscriptionId })
    .from(subscriptions)
    .where(
      and(
        eq(subscriptions.ownerKind, "org"),
        inArray(subscriptions.ownerId, owned),
        ne(subscriptions.status, "canceled"),
      ),
    );
  for (const s of live) await deps.billing.cancelNow(s.id);
}

/** One pass; null when another replica holds the lock. */
export async function purgeAccounts(deps: Deps): Promise<PurgeResult | null> {
  return withLock(deps.redis, LOCK, PURGE_INTERVAL_MS, async () => {
    const cutoff = new Date(deps.clock.nowMs() - PURGE_AFTER_MS);
    const due = await deps.db
      .select({ id: user.id, email: user.email })
      .from(user)
      .where(and(isNotNull(user.deletionRequestedAt), lte(user.deletionRequestedAt, cutoff)));
    let purged = 0;
    let blocked = 0;
    let failed = 0;
    for (const u of due) {
      try {
        if (await purgeOne(deps, u.id, u.email)) purged++;
        else blocked++;
      } catch {
        // Retried on the next pass; the others still go now.
        failed++;
      }
    }
    // Counts only: no ids, addresses or error text (it can quote them) in the logs.
    if (purged + blocked + failed > 0)
      logger.info({ purged, blocked, failed }, "account deletions carried out");
    return { purged, blocked, failed };
  });
}

async function purgeOne(deps: Deps, userId: string, email: string): Promise<boolean> {
  // A Team that gained members after the request waits for its owner to transfer it.
  if (await ownsTeamWithMembers(deps.db, userId)) return false;
  const owned = (await deps.db.select({ id: orgs.id }).from(orgs).where(eq(orgs.ownerId, userId))).map(
    (o) => o.id,
  );
  // Stripe first: the customer holds the billing details there, and deleting
  // it also cancels any subscription started after the request.
  const customers = [
    ...(await deps.db
      .select()
      .from(billingCustomers)
      .where(and(eq(billingCustomers.ownerKind, "user"), eq(billingCustomers.ownerId, userId)))),
    ...(owned.length > 0
      ? await deps.db
          .select()
          .from(billingCustomers)
          .where(and(eq(billingCustomers.ownerKind, "org"), inArray(billingCustomers.ownerId, owned)))
      : []),
  ];
  for (const c of customers) await deps.billing.deleteCustomer(c.customerId);
  await deps.db.transaction(async (tx) => {
    if (owned.length > 0) {
      await tx.delete(orgInvites).where(inArray(orgInvites.orgId, owned));
      // Cascades the org's members, policy, SSO connection and domains.
      await tx.delete(orgs).where(inArray(orgs.id, owned));
    }
    await tx.delete(orgInvites).where(sql`lower(${orgInvites.email}) = lower(${email})`);
    await tx.delete(deviceCodes).where(eq(deviceCodes.userId, userId));
    await tx.delete(dictionarySync).where(eq(dictionarySync.userId, userId));
    await tx.delete(pairings).where(eq(pairings.userId, userId));
    await tx.delete(voiceSessions).where(eq(voiceSessions.userId, userId));
    await tx.delete(aiRequests).where(eq(aiRequests.userId, userId));
    await tx.delete(dailyUsage).where(eq(dailyUsage.userId, userId));
    for (const c of customers)
      await tx.delete(billingCustomers).where(eq(billingCustomers.customerId, c.customerId));
    // Cascades sessions, sign-in links, refresh tokens, desktop login codes and memberships.
    await tx.delete(user).where(eq(user.id, userId));
  });
  return true;
}

/** Starts the purge; the returned function stops it. */
export function startDeletionJob(deps: Deps): () => void {
  const timer = setInterval(() => {
    purgeAccounts(deps).catch((err: unknown) =>
      logger.error({ err: String(err) }, "account deletion pass failed"),
    );
  }, PURGE_INTERVAL_MS);
  timer.unref();
  return () => clearInterval(timer);
}
