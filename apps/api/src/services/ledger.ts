// Credits ledger (ARCHITECTURE §14.5): buckets, the monthly grant, pack
// purchases, and reserve → settle / refund for metered hosted features.
import { and, asc, desc, eq, inArray, lt, sql } from "drizzle-orm";
import { MONTHLY_CREDITS } from "@tandem/protocol/plans";
import type { Db } from "../db/client";
import { creditBuckets, creditLedger, creditReservations, memberUsage } from "../db/schema";
import { newId } from "../lib/ids";
import { ApiError } from "../lib/problem";
import { membershipOf } from "./membership";

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
    feature: r.reason === "reserve" ? r.refType : null,
    createdAt: r.createdAt.toISOString(),
  }));
}

/** A held reservation expires (and the sweeper refunds it) after 15 minutes. */
export const RESERVATION_TTL_MS = 15 * 60_000;

type Allocation = { bucketId: string; amount: number };

export type ReserveArgs = {
  /** Who pays; defaults to the user's credit owner (their org's pool when they're a member). */
  owner?: Owner;
  userId: string;
  feature: string;
  amount: number;
  now: Date;
  /** A retried call with the same key gets the same reservation back. */
  idempotencyKey?: string;
};

const exhausted = (needed: number, extra: Record<string, unknown> = {}, detail?: string) =>
  new ApiError(402, "CREDITS_EXHAUSTED", "Payment Required", detail ?? "You're out of credits for now.", {
    needed,
    ...extra,
  });

/** The member-cap period: the org's current monthly grant period. */
function capPeriodOf(monthly: { periodEnd: Date | null }): string {
  return monthly.periodEnd ? monthly.periodEnd.toISOString().slice(0, 10) : "open";
}

/**
 * Counts `amount` against a Team member's monthly cap, or throws 402 with
 * `cap` set. The row is locked, so parallel reserves can't pass the cap together.
 */
async function takeCap(tx: Tx, orgId: string, userId: string, period: string, cap: number, amount: number) {
  await tx.insert(memberUsage).values({ orgId, userId, period, spent: 0 }).onConflictDoNothing();
  const [row] = await tx
    .select()
    .from(memberUsage)
    .where(and(eq(memberUsage.orgId, orgId), eq(memberUsage.userId, userId), eq(memberUsage.period, period)))
    .for("update");
  const spent = row?.spent ?? 0;
  if (spent + amount > cap) {
    throw exhausted(
      amount,
      { cap, spent },
      `You've used ${spent.toLocaleString("en-US")} of the ${cap.toLocaleString("en-US")} credits your organization allows you this month.`,
    );
  }
  await tx
    .update(memberUsage)
    .set({ spent: sql`${memberUsage.spent} + ${amount}` })
    .where(and(eq(memberUsage.orgId, orgId), eq(memberUsage.userId, userId), eq(memberUsage.period, period)));
}

async function releaseCap(
  tx: Tx,
  row: { ownerId: string; userId: string; capPeriod: string | null },
  amount: number,
) {
  if (!row.capPeriod || amount <= 0) return;
  await tx
    .update(memberUsage)
    .set({ spent: sql`greatest(${memberUsage.spent} - ${amount}, 0)` })
    .where(
      and(
        eq(memberUsage.orgId, row.ownerId),
        eq(memberUsage.userId, row.userId),
        eq(memberUsage.period, row.capPeriod),
      ),
    );
}

async function existingReservation(db: Db | Tx, userId: string, key: string): Promise<string | null> {
  const [row] = await db
    .select({ id: creditReservations.id })
    .from(creditReservations)
    .where(and(eq(creditReservations.userId, userId), eq(creditReservations.idempotencyKey, key)))
    .limit(1);
  return row?.id ?? null;
}

/**
 * Takes `amount` credits, monthly first, then purchased, in one transaction.
 * The buckets are locked in a fixed order; a short balance (or a Team
 * member's cap) is 402 CREDITS_EXHAUSTED and changes nothing.
 */
export async function reserve(db: Db, args: ReserveArgs): Promise<string> {
  if (args.idempotencyKey) {
    const found = await existingReservation(db, args.userId, args.idempotencyKey);
    if (found) return found;
  }
  try {
    return await db.transaction(async (tx) => {
      const membership = await membershipOf(tx, args.userId);
      const owner: Owner =
        args.owner ??
        (membership ? { kind: "org", id: membership.orgId } : { kind: "user", id: args.userId });
      const monthly = await bucket(tx, owner, "monthly");
      const purchased = await bucket(tx, owner, "purchased");
      let capPeriod: string | null = null;
      if (owner.kind === "org" && membership?.orgId === owner.id && membership.monthlyCap !== null) {
        capPeriod = capPeriodOf(monthly);
        await takeCap(tx, owner.id, args.userId, capPeriod, membership.monthlyCap, args.amount);
      }
      if (monthly.balance + purchased.balance < args.amount) throw exhausted(args.amount);
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
        ownerKind: owner.kind,
        ownerId: owner.id,
        feature: args.feature,
        amount: args.amount,
        allocations,
        state: "held",
        idempotencyKey: args.idempotencyKey ?? null,
        capPeriod,
        expiresAt: new Date(args.now.getTime() + RESERVATION_TTL_MS),
      });
      return id;
    });
  } catch (err) {
    // Two calls with one key raced: the loser returns the winner's reservation.
    if (args.idempotencyKey && !(err instanceof ApiError)) {
      const found = await existingReservation(db, args.userId, args.idempotencyKey);
      if (found) return found;
    }
    throw err;
  }
}

/** Gives `amount` back across `allocations`, purchased first (the reverse of spending). */
async function giveBack(
  tx: Tx,
  allocations: Allocation[],
  amount: number,
  reason: "settle_refund" | "refund" | "expire_sweep",
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

export type Settled = { owner: Owner; charged: number } | null;

/** Charges `actual` (≤ the reserved amount) and returns the rest. Idempotent: null when already done. */
export async function settle(db: Db, reservationId: string, actual: number): Promise<Settled> {
  return db.transaction(async (tx) => {
    const row = await heldReservation(tx, reservationId);
    if (!row) return null;
    const charged = Math.max(0, Math.min(actual, row.amount));
    await giveBack(
      tx,
      row.allocations as Allocation[],
      row.amount - charged,
      "settle_refund",
      row.id,
      row.userId,
    );
    await releaseCap(tx, row, row.amount - charged);
    await tx
      .update(creditReservations)
      .set({ state: "settled", settledAmount: charged })
      .where(eq(creditReservations.id, row.id));
    return { owner: { kind: row.ownerKind as Owner["kind"], id: row.ownerId }, charged };
  });
}

/** Returns everything a failed call reserved. Idempotent: null when already done. */
export async function refund(
  db: Db,
  reservationId: string,
  reason: "refund" | "expire_sweep" = "refund",
): Promise<Settled> {
  return db.transaction(async (tx) => {
    const row = await heldReservation(tx, reservationId);
    if (!row) return null;
    await giveBack(tx, row.allocations as Allocation[], row.amount, reason, row.id, row.userId);
    await releaseCap(tx, row, row.amount);
    await tx
      .update(creditReservations)
      .set({ state: "refunded", settledAmount: 0 })
      .where(eq(creditReservations.id, row.id));
    return { owner: { kind: row.ownerKind as Owner["kind"], id: row.ownerId }, charged: 0 };
  });
}

/** Held reservations past their 15 minutes, oldest first. */
export async function expiredReservations(db: Db, now: Date, limit = 500): Promise<string[]> {
  const rows = await db
    .select({ id: creditReservations.id })
    .from(creditReservations)
    .where(and(eq(creditReservations.state, "held"), lt(creditReservations.expiresAt, now)))
    .orderBy(asc(creditReservations.expiresAt))
    .limit(limit);
  return rows.map((r) => r.id);
}

/** A Team member's cap and what they've spent against it this period. */
export async function memberSpend(db: Db, orgId: string, userId: string): Promise<number> {
  const [monthly] = await db
    .select()
    .from(creditBuckets)
    .where(
      and(
        eq(creditBuckets.ownerKind, "org"),
        eq(creditBuckets.ownerId, orgId),
        eq(creditBuckets.kind, "monthly"),
      ),
    )
    .limit(1);
  const period = capPeriodOf({ periodEnd: monthly?.periodEnd ?? null });
  const [row] = await db
    .select()
    .from(memberUsage)
    .where(and(eq(memberUsage.orgId, orgId), eq(memberUsage.userId, userId), eq(memberUsage.period, period)))
    .limit(1);
  return row?.spent ?? 0;
}
