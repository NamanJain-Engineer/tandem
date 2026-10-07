// Credits ledger (ARCHITECTURE §14.5): buckets, the monthly grant, pack
// purchases, and reserve → settle / refund for metered hosted features.
import { and, desc, eq, inArray, sql } from "drizzle-orm";
import { MONTHLY_CREDITS } from "@tandem/protocol/plans";
import type { Db } from "../db/client";
import { creditBuckets, creditLedger, creditReservations } from "../db/schema";
import { newId } from "../lib/ids";
import { ApiError } from "../lib/problem";

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

/** A held reservation expires (and the sweeper refunds it) after 15 minutes. */
export const RESERVATION_TTL_MS = 15 * 60_000;

type Allocation = { bucketId: string; amount: number };

/**
 * Takes `amount` credits, monthly first, then purchased, in one transaction.
 * The buckets are locked in a fixed order; a short balance is 402
 * CREDITS_EXHAUSTED and changes nothing.
 */
export async function reserve(
  db: Db,
  args: { owner: Owner; userId: string; feature: string; amount: number; now: Date },
): Promise<string> {
  return db.transaction(async (tx) => {
    const monthly = await bucket(tx, args.owner, "monthly");
    const purchased = await bucket(tx, args.owner, "purchased");
    if (monthly.balance + purchased.balance < args.amount) {
      throw new ApiError(402, "CREDITS_EXHAUSTED", "Payment Required", "You're out of credits for now.", {
        needed: args.amount,
      });
    }
    const id = newId("resv");
    const allocations: Allocation[] = [];
    let left = args.amount;
    for (const b of [monthly, purchased]) {
      const take = Math.min(b.balance, left);
      if (take <= 0) continue;
      allocations.push({ bucketId: b.id, amount: take });
      left -= take;
      await tx
        .update(creditBuckets)
        .set({ balance: sql`${creditBuckets.balance} - ${take}` })
        .where(eq(creditBuckets.id, b.id));
      await tx.insert(creditLedger).values({
        id: newId("cl"),
        bucketId: b.id,
        userId: args.userId,
        delta: -take,
        reason: "reserve",
        refType: args.feature,
        reservationId: id,
      });
    }
    await tx.insert(creditReservations).values({
      id,
      userId: args.userId,
      ownerKind: args.owner.kind,
      ownerId: args.owner.id,
      feature: args.feature,
      amount: args.amount,
      allocations,
      state: "held",
      expiresAt: new Date(args.now.getTime() + RESERVATION_TTL_MS),
    });
    return id;
  });
}

/** Gives `amount` back across `allocations`, purchased first (the reverse of spending). */
async function giveBack(
  tx: Tx,
  allocations: Allocation[],
  amount: number,
  reason: "settle_refund" | "refund",
  reservationId: string,
  userId: string,
): Promise<void> {
  let left = amount;
  for (const a of [...allocations].reverse()) {
    const back = Math.min(a.amount, left);
    if (back <= 0) continue;
    left -= back;
    await tx
      .update(creditBuckets)
      .set({ balance: sql`${creditBuckets.balance} + ${back}` })
      .where(eq(creditBuckets.id, a.bucketId));
    await tx.insert(creditLedger).values({
      id: newId("cl"),
      bucketId: a.bucketId,
      userId,
      delta: back,
      reason,
      reservationId,
    });
  }
}

async function heldReservation(tx: Tx, id: string) {
  const [row] = await tx.select().from(creditReservations).where(eq(creditReservations.id, id)).for("update");
  return row?.state === "held" ? row : null;
}

/** Charges `actual` (≤ the reserved amount) and returns the rest. Idempotent. */
export async function settle(db: Db, reservationId: string, actual: number): Promise<void> {
  await db.transaction(async (tx) => {
    const row = await heldReservation(tx, reservationId);
    if (!row) return;
    const charged = Math.max(0, Math.min(actual, row.amount));
    await giveBack(
      tx,
      row.allocations as Allocation[],
      row.amount - charged,
      "settle_refund",
      row.id,
      row.userId,
    );
    await tx
      .update(creditReservations)
      .set({ state: "settled", settledAmount: charged })
      .where(eq(creditReservations.id, row.id));
  });
}

/** Returns everything a failed call reserved. Idempotent. */
export async function refund(db: Db, reservationId: string): Promise<void> {
  await db.transaction(async (tx) => {
    const row = await heldReservation(tx, reservationId);
    if (!row) return;
    await giveBack(tx, row.allocations as Allocation[], row.amount, "refund", row.id, row.userId);
    await tx
      .update(creditReservations)
      .set({ state: "refunded", settledAmount: 0 })
      .where(eq(creditReservations.id, row.id));
  });
}
