import { eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { creditLedger, creditReservations, orgMembers, orgs } from "../src/db/schema";
import { newId } from "../src/lib/ids";
import { sweepReservations } from "../src/jobs/reservationSweeper";
import { addPurchased, balance, grantMonthly, reserve, settle, type Owner } from "../src/services/ledger";
import { bearerFor, json, makeTestApp, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

const MIN = 60_000;
const grant = (owner: Owner, seats = 1, refId = "inv_1") =>
  t.deps.db.transaction((tx) =>
    grantMonthly(tx, owner, seats, new Date(t.clock.nowMs() + 30 * 86_400_000), refId),
  );

async function orgWith(members: Array<{ userId: string; role: "owner" | "admin" | "member"; cap?: number }>) {
  const orgId = newId("org");
  await t.deps.db.insert(orgs).values({ id: orgId, name: "Acme", ownerId: members[0]?.userId ?? "" });
  for (const m of members) {
    await t.deps.db
      .insert(orgMembers)
      .values({ orgId, userId: m.userId, role: m.role, monthlyCap: m.cap ?? null });
  }
  return orgId;
}

describe("F-BE-05 credits ledger", () => {
  it("F-BE-05.2 a reservation not settled within 15 min is refunded by the sweeper", async () => {
    const { userId } = await signUpVerified(t, "sweep@tandem.test");
    const owner = { kind: "user" as const, id: userId };
    await grant(owner);
    const stale = await reserve(t.deps.db, { userId, feature: "stt", amount: 40, now: t.clock.now() });
    t.clock.advance(10 * MIN);
    const fresh = await reserve(t.deps.db, { userId, feature: "stt", amount: 4, now: t.clock.now() });
    t.clock.advance(5 * MIN + 1000);
    expect(await sweepReservations(t.deps)).toBe(1);
    const rows = await t.deps.db.select().from(creditReservations);
    expect(rows.find((r) => r.id === stale)?.state).toBe("refunded");
    expect(rows.find((r) => r.id === fresh)?.state).toBe("held");
    expect((await balance(t.deps.db, owner)).monthly).toBe(10_000 - 4);
    const sweep = await t.deps.db.select().from(creditLedger).where(eq(creditLedger.reservationId, stale));
    expect(sweep.map((l) => [l.reason, l.delta]).sort()).toEqual([
      ["expire_sweep", 40],
      ["reserve", -40],
    ]);
    // Settling a swept reservation afterwards changes nothing.
    expect(await settle(t.deps.db, stale, 40)).toBeNull();
    expect((await balance(t.deps.db, owner)).monthly).toBe(9_996);
  });

  it("F-BE-05.2 one replica at a time: a held lock skips the sweep", async () => {
    await t.deps.redis.set("job:reservation-sweeper", "other", "PX", 60_000);
    expect(await sweepReservations(t.deps)).toBeNull();
  });

  it("F-BE-05.3 the monthly reset zeroes the monthly bucket and leaves purchased untouched", async () => {
    const { userId } = await signUpVerified(t, "reset@tandem.test");
    const owner = { kind: "user" as const, id: userId };
    await grant(owner, 1, "inv_a");
    await t.deps.db.transaction((tx) => addPurchased(tx, owner, 5_000, "cs_pack"));
    const r = await reserve(t.deps.db, { userId, feature: "rewrite", amount: 1_234, now: t.clock.now() });
    await settle(t.deps.db, r, 1_234);
    expect(await balance(t.deps.db, owner)).toMatchObject({ monthly: 8_766, purchased: 5_000 });
    await grant(owner, 1, "inv_b");
    expect(await balance(t.deps.db, owner)).toMatchObject({ monthly: 10_000, purchased: 5_000 });
    const reasons = (await t.deps.db.select().from(creditLedger)).map((l) => [l.reason, l.delta, l.refId]);
    expect(reasons).toContainEqual(["monthly_reset", -8_766, "inv_b"]);
    expect(reasons).toContainEqual(["monthly_grant", 10_000, "inv_b"]);
  });

  it("spends monthly first, then purchased, and refunds purchased first", async () => {
    const { userId } = await signUpVerified(t, "order@tandem.test");
    const owner = { kind: "user" as const, id: userId };
    await grant(owner);
    await t.deps.db.transaction((tx) => addPurchased(tx, owner, 100, "cs_1"));
    const r = await reserve(t.deps.db, { userId, feature: "x", amount: 10_050, now: t.clock.now() });
    expect(await balance(t.deps.db, owner)).toMatchObject({ monthly: 0, purchased: 50 });
    await settle(t.deps.db, r, 10_000);
    expect(await balance(t.deps.db, owner)).toMatchObject({ monthly: 0, purchased: 100 });
  });

  it("an idempotency key returns the same reservation instead of charging twice", async () => {
    const { userId } = await signUpVerified(t, "idem@tandem.test");
    await grant({ kind: "user", id: userId });
    const ids = await Promise.all(
      Array.from({ length: 5 }, () =>
        reserve(t.deps.db, {
          userId,
          feature: "x",
          amount: 10,
          now: t.clock.now(),
          idempotencyKey: "take-1",
        }),
      ),
    );
    expect(new Set(ids).size).toBe(1);
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(9_990);
  });

  it("a Team member spends from the org pool, and parallel reserves can't pass the member cap", async () => {
    const { userId: owner } = await signUpVerified(t, "own@tandem.test");
    const { userId: member } = await signUpVerified(t, "mem@tandem.test");
    const orgId = await orgWith([
      { userId: owner, role: "owner" },
      { userId: member, role: "member", cap: 1_000 },
    ]);
    await grant({ kind: "org", id: orgId }, 2);
    const results = await Promise.allSettled(
      Array.from({ length: 20 }, () =>
        reserve(t.deps.db, { userId: member, feature: "x", amount: 100, now: t.clock.now() }),
      ),
    );
    expect(results.filter((r) => r.status === "fulfilled")).toHaveLength(10);
    const rejected = results.find((r) => r.status === "rejected") as PromiseRejectedResult;
    expect(rejected.reason).toMatchObject({ status: 402, code: "CREDITS_EXHAUSTED", extra: { cap: 1_000 } });
    expect((await balance(t.deps.db, { kind: "org", id: orgId })).monthly).toBe(19_000);
    // The owner has no cap and keeps spending from the same pool.
    await reserve(t.deps.db, { userId: owner, feature: "x", amount: 5_000, now: t.clock.now() });
    expect((await balance(t.deps.db, { kind: "org", id: orgId })).monthly).toBe(14_000);
  });

  it("a refunded call gives its credits back to the member's cap", async () => {
    const { userId: owner } = await signUpVerified(t, "own2@tandem.test");
    const { userId: member } = await signUpVerified(t, "mem2@tandem.test");
    const orgId = await orgWith([
      { userId: owner, role: "owner" },
      { userId: member, role: "member", cap: 100 },
    ]);
    await grant({ kind: "org", id: orgId }, 2);
    const r = await reserve(t.deps.db, { userId: member, feature: "x", amount: 100, now: t.clock.now() });
    await settle(t.deps.db, r, 30);
    await reserve(t.deps.db, { userId: member, feature: "x", amount: 70, now: t.clock.now() });
    await expect(
      reserve(t.deps.db, { userId: member, feature: "x", amount: 1, now: t.clock.now() }),
    ).rejects.toMatchObject({ code: "CREDITS_EXHAUSTED" });
  });

  it("GET /v1/credits shows a member the org pool, their cap and the last ledger entries", async () => {
    const { userId: owner } = await signUpVerified(t, "own3@tandem.test");
    const { userId: member } = await signUpVerified(t, "mem3@tandem.test");
    const orgId = await orgWith([
      { userId: owner, role: "owner" },
      { userId: member, role: "member", cap: 500 },
    ]);
    await grant({ kind: "org", id: orgId }, 2);
    const r = await reserve(t.deps.db, { userId: member, feature: "rewrite", amount: 3, now: t.clock.now() });
    await settle(t.deps.db, r, 3);
    const res = await json(t, "GET", "/v1/credits", { bearer: await bearerFor(t.deps, member) });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      balance: { monthly: number };
      org: { id: string; cap: number; spent: number };
      ledger: Array<{ reason: string; feature: string | null }>;
    };
    expect(body.balance.monthly).toBe(19_997);
    expect(body.org).toEqual({ id: orgId, cap: 500, spent: 3 });
    expect(body.ledger[0]).toMatchObject({ reason: "reserve", feature: "rewrite" });
  });
});
