import { eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { adminActions, creditLedger, user } from "../src/db/schema";
import { balance } from "../src/services/ledger";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

async function admin() {
  const a = await signUpVerified(t, "admin@tandem.test");
  await t.deps.db.update(user).set({ role: "admin" }).where(eq(user.id, a.userId));
  return a;
}

describe("F-BE-12 admin", () => {
  it("F-BE-12.1 a credit grant writes a ledger entry with the admin id and reason; a non-admin gets 403", async () => {
    const a = await admin();
    const { userId, jar: userJar } = await signUpVerified(t, "support@tandem.test");
    const res = await json(t, "POST", `/v1/admin/users/${userId}/credits`, {
      jar: a.jar,
      body: { amount: 2_500, reason: "Make-good for a failed cloud take" },
    });
    expect(res.status).toBe(200);
    expect(await balance(t.deps.db, { kind: "user", id: userId })).toMatchObject({ purchased: 2_500 });
    const [entry] = await t.deps.db.select().from(creditLedger).where(eq(creditLedger.userId, userId));
    expect(entry).toMatchObject({
      reason: "admin_grant",
      delta: 2_500,
      adminId: a.userId,
      note: "Make-good for a failed cloud take",
    });
    const [action] = await t.deps.db.select().from(adminActions).where(eq(adminActions.targetUserId, userId));
    expect(action).toMatchObject({ adminId: a.userId, action: "grant_credits", reason: "Make-good for a failed cloud take" });

    const denied = await json(t, "POST", `/v1/admin/users/${userId}/credits`, {
      jar: userJar,
      body: { amount: 1_000, reason: "free credits please" },
    });
    expect(denied.status).toBe(403);
    // A desktop bearer, even an admin's, isn't a portal session.
    const bearer = await json(t, "GET", `/v1/admin/users?q=support`, { bearer: await bearerFor(t.deps, a.userId) });
    expect(bearer.status).toBe(401);
  });

  it("looks a user up and shows plan, ledger and the admin actions on them", async () => {
    const a = await admin();
    const { userId } = await signUpVerified(t, "lookup@tandem.test");
    await setSubscription(t.deps, userId, { firstPaidAt: t.clock.now() });
    await json(t, "POST", `/v1/admin/users/${userId}/credits`, {
      jar: a.jar,
      body: { amount: 100, reason: "test grant", bucket: "monthly" },
    });
    const search = (await (await json(t, "GET", "/v1/admin/users?q=LOOKUP", { jar: a.jar })).json()) as {
      users: Array<{ id: string; email: string }>;
    };
    expect(search.users.map((u) => u.email)).toEqual(["lookup@tandem.test"]);
    // A wildcard in the query is matched literally.
    const wild = (await (await json(t, "GET", "/v1/admin/users?q=%25", { jar: a.jar })).json()) as { users: unknown[] };
    expect(wild.users).toEqual([]);
    const detail = (await (await json(t, "GET", `/v1/admin/users/${userId}`, { jar: a.jar })).json()) as {
      plan: string;
      subscription: { status: string };
      credits: { monthly: number };
      ledger: Array<{ reason: string; note: string }>;
      actions: Array<{ action: string; adminEmail: string }>;
    };
    expect(detail.plan).toBe("pro");
    expect(detail.credits.monthly).toBe(100);
    expect(detail.ledger[0]).toMatchObject({ reason: "admin_grant", note: "test grant" });
    expect(detail.actions[0]).toMatchObject({ action: "grant_credits", adminEmail: "admin@tandem.test" });

    await json(t, "POST", `/v1/admin/users/${userId}/refund`, { jar: a.jar, body: { reason: "asked in time" } });
    const after = (await (await json(t, "GET", `/v1/admin/users/${userId}`, { jar: a.jar })).json()) as {
      plan: string;
      actions: Array<{ action: string }>;
    };
    expect(after.plan).toBe("free");
    expect(after.actions.map((x) => x.action)).toEqual(["refund_plan", "grant_credits"]);
  });

  it("a grant to a Team member lands in the org's pool", async () => {
    const a = await admin();
    const owner = await signUpVerified(t, "own@tandem.test");
    const res = await json(t, "POST", "/v1/orgs", { jar: owner.jar, body: { name: "Acme" } });
    const org = (await res.json()) as { id: string };
    await json(t, "POST", `/v1/admin/users/${owner.userId}/credits`, {
      jar: a.jar,
      body: { amount: 700, reason: "team goodwill" },
    });
    expect(await balance(t.deps.db, { kind: "org", id: org.id })).toMatchObject({ purchased: 700 });
  });

  it("validates amounts and reasons, and unknown users are 404", async () => {
    const a = await admin();
    const { userId } = await signUpVerified(t, "val@tandem.test");
    for (const body of [{ amount: 0, reason: "zero" }, { amount: 10, reason: "x" }, { amount: 2_000_000, reason: "too many" }]) {
      const res = await json(t, "POST", `/v1/admin/users/${userId}/credits`, { jar: a.jar, body });
      expect(res.status).toBe(422);
    }
    const missing = await json(t, "GET", "/v1/admin/users/nope", { jar: a.jar });
    expect(missing.status).toBe(404);
  });
});
