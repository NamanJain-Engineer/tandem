// Balance views and `credits` invalidations (ARCHITECTURE §14.4–14.5).
import type { Deps } from "../deps";
import { logger } from "../lib/logger";
import { balance, memberSpend, type Owner } from "./ledger";
import { membershipOf, usersOf } from "./membership";

/** Tells every desktop that shows `owner`'s balance to refetch it. */
export async function notifyCredits(deps: Deps, owner: Owner): Promise<void> {
  try {
    for (const userId of await usersOf(deps.db, owner)) await deps.events.publish(userId, "credits");
  } catch (err) {
    logger.warn({ err: String(err) }, "credits event not published");
  }
}

export type CreditsView = {
  monthly: number;
  purchased: number;
  resetsAt: string | null;
  /** Set for Team members: the pool is the org's. */
  org: { id: string; cap: number | null; spent: number } | null;
};

/** What a user can spend: their own buckets, or their org's pool and their cap. */
export async function creditsFor(deps: Deps, userId: string): Promise<CreditsView> {
  const m = await membershipOf(deps.db, userId);
  const owner: Owner = m ? { kind: "org", id: m.orgId } : { kind: "user", id: userId };
  const b = await balance(deps.db, owner);
  if (!m) return { ...b, org: null };
  return {
    ...b,
    org: { id: m.orgId, cap: m.monthlyCap, spent: await memberSpend(deps.db, m.orgId, userId) },
  };
}
