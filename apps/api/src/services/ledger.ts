// Credits ledger (ARCHITECTURE §14.5). M1 builds buckets, the monthly grant and
// pack purchases; reserve/settle/refund land with the hosted features in M13.
import { and, desc, eq, inArray, sql } from "drizzle-orm";
import { MONTHLY_CREDITS } from "@tandem/protocol/plans";
import type { Db } from "../db/client";
import { creditBuckets, creditLedger } from "../db/schema";
import { newId } from "../lib/ids";

export type Owner = { kind: "user" | "org"; id: string };
type Tx = Parameters<Parameters<Db["transaction"]>[0]>[0];

async function bucket(tx: Tx | Db, owner: Owner, kind: "monthly" | "purchased") {
  const existing = await tx
    .select()
    .from(creditBuckets)
    .where(
      and(
        eq(creditBuckets.ownerKind, owner.kind),
        eq(creditBuckets.ownerId, owner.id),
        eq(creditBuckets.kind, kind),
      ),
    )
    .for("update");
  if (existing[0]) return existing[0];
  const [created] = await tx
    .insert(creditBuckets)
    .values({ id: newId("cb"), ownerKind: owner.kind, ownerId: owner.id, kind, balance: 0 })
    .onConflictDoNothing()
    .returning();
  if (created) return created;
  const [again] = await tx
    .select()
    .from(creditBuckets)
    .where(
      and(
        eq(creditBuckets.ownerKind, owner.kind),
        eq(creditBuckets.ownerId, owner.id),
        eq(creditBuckets.kind, kind),
      ),
    )
    .for("update");
  if (!again) throw new Error("credit bucket missing");
  return again;
}

/**
 * Monthly grant on `invoice.paid`: ledger `monthly_reset` (−old balance), then
 * `monthly_grant` (+10,000 × seats); purchased credits are untouched.
 */
export async function grantMonthly(
  tx: Tx,
  owner: Owner,
  seats: number,
  periodEnd: Date,
  refId: string,
): Promise<void> {
  const monthly = await bucket(tx, owner, "monthly");
  if (monthly.balance > 0) {
    await tx.insert(creditLedger).values({
      id: newId("cl"),
      bucketId: monthly.id,
      delta: -monthly.balance,
      reason: "monthly_reset",
      refType: "invoice",
      refId,
    });
  }
  const amount = MONTHLY_CREDITS * Math.max(1, seats);
  await tx.insert(creditLedger).values({
    id: newId("cl"),
    bucketId: monthly.id,
    delta: amount,
    reason: "monthly_grant",
    refType: "invoice",
    refId,
  });
  await tx.update(creditBuckets).set({ balance: amount, periodEnd }).where(eq(creditBuckets.id, monthly.id));
}

export async function addPurchased(tx: Tx, owner: Owner, credits: number, refId: string): Promise<void> {
  const purchased = await bucket(tx, owner, "purchased");
  await tx.insert(creditLedger).values({
    id: newId("cl"),
    bucketId: purchased.id,
    delta: credits,
    reason: "pack_purchase",
    refType: "checkout",
    refId,
  });
  await tx
    .update(creditBuckets)
    .set({ balance: sql`${creditBuckets.balance} + ${credits}` })
    .where(eq(creditBuckets.id, purchased.id));
}

/** Zeroes the monthly bucket (refunds, F-BL-05). */
export async function zeroMonthly(
  tx: Tx,
  owner: Owner,
  refId: string,
  reason: "plan_refund" | "monthly_reset",
): Promise<void> {
  const monthly = await bucket(tx, owner, "monthly");
  if (monthly.balance === 0) return;
  await tx.insert(creditLedger).values({
    id: newId("cl"),
    bucketId: monthly.id,
    delta: -monthly.balance,
    reason,
    refType: "subscription",
    refId,
  });
  await tx.update(creditBuckets).set({ balance: 0 }).where(eq(creditBuckets.id, monthly.id));
}

export type Balance = { monthly: number; purchased: number; resetsAt: string | null };

export async function balance(db: Db, owner: Owner): Promise<Balance> {
  const rows = await db
    .select()
    .from(creditBuckets)
    .where(and(eq(creditBuckets.ownerKind, owner.kind), eq(creditBuckets.ownerId, owner.id)));
  const monthly = rows.find((r) => r.kind === "monthly");
  const purchased = rows.find((r) => r.kind === "purchased");
  return {
    monthly: monthly?.balance ?? 0,
    purchased: purchased?.balance ?? 0,
    resetsAt: monthly?.periodEnd?.toISOString() ?? null,
  };
}

export async function recentLedger(db: Db, owner: Owner, limit = 100) {
  const buckets = await db
    .select({ id: creditBuckets.id, kind: creditBuckets.kind })
    .from(creditBuckets)
    .where(and(eq(creditBuckets.ownerKind, owner.kind), eq(creditBuckets.ownerId, owner.id)));
  if (buckets.length === 0) return [];
  const kinds = new Map(buckets.map((b) => [b.id, b.kind]));
  const rows = await db
    .select()
    .from(creditLedger)
    .where(inArray(creditLedger.bucketId, [...kinds.keys()]))
    .orderBy(desc(creditLedger.createdAt))
    .limit(limit);
  return rows.map((r) => ({
    id: r.id,
    bucket: kinds.get(r.bucketId),
    delta: r.delta,
    reason: r.reason,
    createdAt: r.createdAt.toISOString(),
  }));
}
