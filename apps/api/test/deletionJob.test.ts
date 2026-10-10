import { eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  billingCustomers,
  creditLedger,
  dailyUsage,
  deviceCodes,
  dictionarySync,
  orgInvites,
  orgMembers,
  orgPolicies,
  orgs,
  pairings,
  subscriptions,
  user,
} from "../src/db/schema";
import { purgeAccounts } from "../src/jobs/deletionJob";
import { newId } from "../src/lib/ids";
import { grantMonthly } from "../src/services/ledger";
import { json, makeTestApp, PASSWORD, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  vi.restoreAllMocks();
  await t.close();
});

const DAY = 86_400_000;

/** A user with something in every table the purge touches. */
async function withData(email: string) {
  const { userId, jar } = await signUpVerified(t, email);
  const db = t.deps.db;
  const now = t.clock.now();
  await db
    .insert(dictionarySync)
    .values({ userId, ruleId: "r1", payload: { from: "vibe zeo", to: "Vibezeo" }, rev: 1, updatedAt: now });
  await db
    .insert(pairings)
    .values({ id: newId("pair"), userId, pairingTokenHash: `h-${userId}`, deviceLabel: "Ana's phone" });
  await db
    .insert(deviceCodes)
    .values({
      deviceCodeHash: `d-${userId}`,
      userCode: `U-${userId.slice(-6)}`,
      userId,
      status: "consumed",
      expiresAt: now,
    });
  await db.insert(dailyUsage).values({ userId, feature: "stt", day: "2026-10-01", units: 30 });
  await db
    .insert(orgInvites)
    .values({
      id: newId("inv"),
      orgId: "org_elsewhere",
      email: email.toUpperCase(),
      role: "member",
      tokenHash: `i-${userId}`,
      expiresAt: now,
    });
  await db
    .insert(billingCustomers)
    .values({ ownerKind: "user", ownerId: userId, customerId: `cus_${userId}` });
  await db.transaction((tx) =>
    grantMonthly(tx, { kind: "user", id: userId }, 1, new Date(t.clock.nowMs() + 30 * DAY), `inv_${userId}`),
  );
  return { userId, jar };
}

async function org(ownerId: string, memberIds: string[] = []) {
  const orgId = newId("org");
  await t.deps.db.insert(orgs).values({ id: orgId, name: "Acme", ownerId });
  await t.deps.db.insert(orgMembers).values({ orgId, userId: ownerId, role: "owner" });
  for (const id of memberIds)
    await t.deps.db.insert(orgMembers).values({ orgId, userId: id, role: "member" });
  await t.deps.db
    .insert(orgPolicies)
    .values({ orgId, policy: { forceTelemetryOff: true }, updatedAt: t.clock.now() });
  return orgId;
}

describe("F-TL-05 account deletion", () => {
  it("F-TL-05 within 30 days the account and its personal data are gone; money records and other people stay", async () => {
    const gone = await withData("gone@tandem.test");
    const stays = await withData("stays@tandem.test");
    const ledgerBefore = (await t.deps.db.select().from(creditLedger)).length;
    const res = await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: gone.jar });
    expect(res.status).toBe(202);
    // The account closes at once: the web session ends and no way back in works.
    expect((await json(t, "GET", "/v1/me", { jar: gone.jar })).status).toBe(401);
    const again = await json(t, "POST", "/v1/auth/sign-in/email", {
      body: { email: "gone@tandem.test", password: PASSWORD },
    });
    expect(again.status).toBe(403);
    expect(await again.json()).toMatchObject({ code: "ACCOUNT_DELETED" });

    t.clock.advance(28 * DAY);
    expect(await purgeAccounts(t.deps)).toEqual({ purged: 0, blocked: 0, failed: 0 });
    expect(await t.deps.db.select().from(user).where(eq(user.id, gone.userId))).toHaveLength(1);

    t.clock.advance(1 * DAY + 60_000);
    expect(await purgeAccounts(t.deps)).toEqual({ purged: 1, blocked: 0, failed: 0 });
    expect(await t.deps.db.select().from(user).where(eq(user.id, gone.userId))).toHaveLength(0);
    for (const [name, rows] of [
      [
        "dictionary",
        await t.deps.db.select().from(dictionarySync).where(eq(dictionarySync.userId, gone.userId)),
      ],
      ["pairings", await t.deps.db.select().from(pairings).where(eq(pairings.userId, gone.userId))],
      ["device codes", await t.deps.db.select().from(deviceCodes).where(eq(deviceCodes.userId, gone.userId))],
      ["usage", await t.deps.db.select().from(dailyUsage).where(eq(dailyUsage.userId, gone.userId))],
      [
        "customers",
        await t.deps.db.select().from(billingCustomers).where(eq(billingCustomers.ownerId, gone.userId)),
      ],
    ] as const) {
      expect(rows, name).toHaveLength(0);
    }
    expect((await t.deps.db.select().from(orgInvites)).map((i) => i.email)).toEqual(["STAYS@TANDEM.TEST"]);
    expect(t.billing.calls).toContain(`delete_customer:cus_${gone.userId}`);
    // The append-only ledger keeps its rows; the other user keeps everything.
    expect((await t.deps.db.select().from(creditLedger)).length).toBe(ledgerBefore);
    expect(await t.deps.db.select().from(user).where(eq(user.id, stays.userId))).toHaveLength(1);
    expect(
      await t.deps.db.select().from(dictionarySync).where(eq(dictionarySync.userId, stays.userId)),
    ).toHaveLength(1);
  });

  it("F-TL-05 the owner of a Team with members is asked to transfer it first; a solo Team goes with its owner", async () => {
    const owner = await signUpVerified(t, "owner@tandem.test");
    const member = await signUpVerified(t, "member@tandem.test");
    const shared = await org(owner.userId, [member.userId]);
    const refused = await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: owner.jar });
    expect(refused.status).toBe(409);
    expect(((await refused.json()) as { code: string }).code).toBe("TEAM_HAS_MEMBERS");

    const solo = await signUpVerified(t, "solo@tandem.test");
    const soloOrg = await org(solo.userId);
    await t.deps.db
      .insert(billingCustomers)
      .values({ ownerKind: "org", ownerId: soloOrg, customerId: "cus_solo_org" });
    await t.deps.db.insert(subscriptions).values({
      id: newId("sub"),
      stripeSubscriptionId: "sub_solo_team",
      ownerKind: "org",
      ownerId: soloOrg,
      plan: "team",
      interval: "month",
      status: "active",
      seats: 2,
      currentPeriodStart: t.clock.now(),
      currentPeriodEnd: new Date(t.clock.nowMs() + 30 * DAY),
      updatedAt: t.clock.now(),
    });
    expect((await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: solo.jar })).status).toBe(
      202,
    );
    // The solo Team's subscription is the owner's own, so it ends now too.
    expect(t.billing.calls).toContain("cancel:sub_solo_team");
    t.clock.advance(29 * DAY + 60_000);
    expect(await purgeAccounts(t.deps)).toEqual({ purged: 1, blocked: 0, failed: 0 });
    expect(await t.deps.db.select().from(orgs).where(eq(orgs.id, soloOrg))).toHaveLength(0);
    expect(await t.deps.db.select().from(orgPolicies).where(eq(orgPolicies.orgId, soloOrg))).toHaveLength(0);
    expect(t.billing.calls).toContain("delete_customer:cus_solo_org");
    expect(t.billing.calls.filter((c) => c.startsWith("cancel:"))).toEqual(["cancel:sub_solo_team"]);
    // The shared Team and its owner are untouched.
    expect(await t.deps.db.select().from(orgs).where(eq(orgs.id, shared))).toHaveLength(1);
  });

  it("F-TL-05 a Team that gains members after the request waits for its owner, and one replica purges at a time", async () => {
    const owner = await signUpVerified(t, "late@tandem.test");
    const teamOrg = await org(owner.userId);
    expect((await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: owner.jar })).status).toBe(
      202,
    );
    const joiner = await signUpVerified(t, "joiner@tandem.test");
    await t.deps.db.insert(orgMembers).values({ orgId: teamOrg, userId: joiner.userId, role: "member" });
    t.clock.advance(29 * DAY + 60_000);
    expect(await purgeAccounts(t.deps)).toEqual({ purged: 0, blocked: 1, failed: 0 });
    expect(await t.deps.db.select().from(user).where(eq(user.id, owner.userId))).toHaveLength(1);
    await t.deps.redis.set("job:deletion", "other", "PX", 60_000);
    expect(await purgeAccounts(t.deps)).toBeNull();
  });

  it("F-TL-05 a billing failure at the request changes nothing, and one failed purge doesn't hold up the rest", async () => {
    const payer = await withData("payer@tandem.test");
    await t.deps.db.insert(subscriptions).values({
      id: newId("sub"),
      stripeSubscriptionId: "sub_payer",
      ownerKind: "user",
      ownerId: payer.userId,
      plan: "pro",
      interval: "month",
      status: "active",
      seats: 1,
      currentPeriodStart: t.clock.now(),
      currentPeriodEnd: new Date(t.clock.nowMs() + 30 * DAY),
      updatedAt: t.clock.now(),
    });
    vi.spyOn(t.billing, "cancelNow").mockRejectedValueOnce(new Error("billing unavailable"));
    const failed = await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: payer.jar });
    expect(failed.status).toBe(500);
    const [row] = await t.deps.db.select().from(user).where(eq(user.id, payer.userId));
    expect(row?.deletionRequestedAt).toBeNull();
    expect((await json(t, "GET", "/v1/me", { jar: payer.jar })).status).toBe(200);
    expect((await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: payer.jar })).status).toBe(
      202,
    );
    expect(t.billing.calls).toContain("cancel:sub_payer");

    const other = await withData("other@tandem.test");
    expect((await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar: other.jar })).status).toBe(
      202,
    );
    const deleteCustomer = vi.spyOn(t.billing, "deleteCustomer").mockImplementation(async (id) => {
      if (id === `cus_${payer.userId}`) throw new Error("billing unavailable");
    });
    t.clock.advance(29 * DAY + 60_000);
    expect(await purgeAccounts(t.deps)).toEqual({ purged: 1, blocked: 0, failed: 1 });
    expect(await t.deps.db.select().from(user).where(eq(user.id, payer.userId))).toHaveLength(1);
    expect(await t.deps.db.select().from(user).where(eq(user.id, other.userId))).toHaveLength(0);
    // The next pass retries it.
    deleteCustomer.mockRestore();
    expect(await purgeAccounts(t.deps)).toEqual({ purged: 1, blocked: 0, failed: 0 });
    expect(await t.deps.db.select().from(user).where(eq(user.id, payer.userId))).toHaveLength(0);
  });
});
