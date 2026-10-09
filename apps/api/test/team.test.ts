import { eq } from "drizzle-orm";
import { decodeJwt } from "jose";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_POLICY } from "@tandem/protocol/team";
import { auditEvents, orgMembers, subscriptions, user } from "../src/db/schema";
import { balance, reserve } from "../src/services/ledger";
import { membershipOf } from "../src/services/membership";
import {
  bearerFor,
  json,
  makeTestApp,
  setSubscription,
  signed,
  signUpVerified,
  type Jar,
  type TestEnv,
} from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

type Me = {
  plan: string;
  limits: { admin: boolean };
  policy: typeof DEFAULT_POLICY | null;
  org: { id: string; role: string } | null;
  credits: { monthly: number; purchased: number; cap: number | null; spent: number | null };
  entitlement_token: string;
};

async function postWebhook(event: unknown) {
  const { body, signature } = signed(t, event);
  return t.app.request("/v1/webhooks/stripe", {
    method: "POST",
    headers: { "stripe-signature": signature, "content-type": "application/json" },
    body,
  });
}

/** An owner who bought Team through (mock) checkout, with `seats` seats. */
async function teamOwner(email: string, seats = 3) {
  const owner = await signUpVerified(t, email);
  const res = await json(t, "POST", "/v1/billing/checkout", {
    jar: owner.jar,
    body: { plan: "team", interval: "month", seats },
  });
  expect(res.status).toBe(200);
  const sessionId = new URL(((await res.json()) as { url: string }).url).searchParams.get("session") ?? "";
  const done = await json(t, "POST", "/v1/billing/mock/complete", { jar: owner.jar, body: { sessionId } });
  expect(done.status).toBe(204);
  const orgId = (await membershipOf(t.deps.db, owner.userId))?.orgId ?? "";
  return { ...owner, orgId };
}

async function join(owner: { jar: Jar; orgId: string }, email: string, role: "admin" | "member" = "member") {
  const res = await json(t, "POST", `/v1/orgs/${owner.orgId}/invites`, {
    jar: owner.jar,
    body: { email, role },
  });
  expect(res.status).toBe(201);
  const token = t.email.latest(email)?.text.match(/token=([\w-]+)/)?.[1] ?? "";
  const member = await signUpVerified(t, email);
  const accept = await json(t, "POST", "/v1/orgs/invites/accept", { jar: member.jar, body: { token } });
  expect(accept.status).toBe(200);
  return member;
}

async function me(bearerUser: string): Promise<Me> {
  const res = await json(t, "GET", "/v1/me", { bearer: await bearerFor(t.deps, bearerUser) });
  expect(res.status).toBe(200);
  return (await res.json()) as Me;
}

describe("F-BL-06 Team plan", () => {
  it("Team checkout makes an org-owned subscription with pooled credits for every seat", async () => {
    const owner = await teamOwner("own@tandem.test", 3);
    const [sub] = await t.deps.db.select().from(subscriptions).where(eq(subscriptions.ownerId, owner.orgId));
    expect(sub).toMatchObject({ ownerKind: "org", plan: "team", seats: 3 });
    expect(await balance(t.deps.db, { kind: "org", id: owner.orgId })).toMatchObject({ monthly: 30_000 });
    const view = await me(owner.userId);
    expect(view).toMatchObject({ plan: "team", org: { id: owner.orgId, role: "owner" } });
    expect(view.limits.admin).toBe(true);
    expect(view.credits.monthly).toBe(30_000);
  });

  it("invites by email: the link joins the org, the wrong account can't use it, seats run out", async () => {
    const owner = await teamOwner("own2@tandem.test", 2);
    const res = await json(t, "POST", `/v1/orgs/${owner.orgId}/invites`, {
      jar: owner.jar,
      body: { email: "Mem@Tandem.test" },
    });
    expect(res.status).toBe(201);
    const mail = t.email.latest("mem@tandem.test");
    expect(mail?.subject).toContain("invited");
    const token = mail?.text.match(/token=([\w-]+)/)?.[1] ?? "";
    // Both seats are now used (the owner and the pending invite).
    const full = await json(t, "POST", `/v1/orgs/${owner.orgId}/invites`, {
      jar: owner.jar,
      body: { email: "third@tandem.test" },
    });
    expect(full.status).toBe(409);
    expect(await full.json()).toMatchObject({ code: "SEATS_FULL" });
    const stranger = await signUpVerified(t, "stranger@tandem.test");
    const wrong = await json(t, "POST", "/v1/orgs/invites/accept", { jar: stranger.jar, body: { token } });
    expect(wrong.status).toBe(403);
    const member = await signUpVerified(t, "mem@tandem.test");
    const ok = await json(t, "POST", "/v1/orgs/invites/accept", { jar: member.jar, body: { token } });
    expect(ok.status).toBe(200);
    const again = await json(t, "POST", "/v1/orgs/invites/accept", { jar: member.jar, body: { token } });
    expect(again.status).toBe(404);
    expect(await me(member.userId)).toMatchObject({ plan: "team", org: { role: "member" } });
  });

  it("F-BL-06.1 the policy travels in /me and the signed entitlement claim; client claims are never read", async () => {
    const owner = await teamOwner("own3@tandem.test");
    const member = await join(owner, "m3@tandem.test");
    const policy = {
      ...DEFAULT_POLICY,
      maxAccess: "edits" as const,
      allowedEngines: ["claude"],
      auditEnabled: true,
    };
    const put = await json(t, "PUT", `/v1/orgs/${owner.orgId}/policy`, { jar: owner.jar, body: policy });
    expect(put.status).toBe(200);
    // A forged header or query claiming otherwise changes nothing: the server recomputes.
    const res = await json(t, "GET", "/v1/me?policy=none", {
      bearer: await bearerFor(t.deps, member.userId),
      headers: { "x-tandem-plan": "pro", "x-tandem-policy": JSON.stringify(DEFAULT_POLICY) },
    });
    const view = (await res.json()) as Me;
    expect(view.policy).toEqual(policy);
    expect(decodeJwt(view.entitlement_token).policy).toEqual(policy);
    // A Pro user outside any org has no policy.
    const { userId: pro } = await signUpVerified(t, "solo@tandem.test");
    await setSubscription(t.deps, pro);
    expect((await me(pro)).policy).toBeNull();
  });

  it("only owners and admins change the org; only the owner makes admins", async () => {
    const owner = await teamOwner("own4@tandem.test", 4);
    const admin = await join(owner, "adm4@tandem.test", "admin");
    const member = await join(owner, "m4@tandem.test");
    const byMember = await json(t, "PUT", `/v1/orgs/${owner.orgId}/policy`, {
      jar: member.jar,
      body: DEFAULT_POLICY,
    });
    expect(byMember.status).toBe(403);
    const promote = await json(t, "PATCH", `/v1/orgs/${owner.orgId}/members/${member.userId}`, {
      jar: admin.jar,
      body: { role: "admin" },
    });
    expect(promote.status).toBe(403);
    const cap = await json(t, "PATCH", `/v1/orgs/${owner.orgId}/members/${member.userId}`, {
      jar: admin.jar,
      body: { monthly_cap: 1_000 },
    });
    expect(cap.status).toBe(204);
    const removeOwner = await json(t, "DELETE", `/v1/orgs/${owner.orgId}/members/${owner.userId}`, {
      jar: admin.jar,
    });
    expect(removeOwner.status).toBe(403);
    // A member sees only their own row; admins see everyone and the invites.
    const asMember = (await (
      await json(t, "GET", `/v1/orgs/${owner.orgId}`, { jar: member.jar })
    ).json()) as {
      members: unknown[];
    };
    expect(asMember.members).toHaveLength(1);
    const leave = await json(t, "DELETE", `/v1/orgs/${owner.orgId}/members/${member.userId}`, {
      jar: member.jar,
    });
    expect(leave.status).toBe(204);
    expect(await membershipOf(t.deps.db, member.userId)).toBeNull();
  });

  it("F-BL-06.2 a member cap of 1,000 stops the member at 1,000 with CREDITS_EXHAUSTED", async () => {
    const owner = await teamOwner("own5@tandem.test");
    const member = await join(owner, "m5@tandem.test");
    await json(t, "PATCH", `/v1/orgs/${owner.orgId}/members/${member.userId}`, {
      jar: owner.jar,
      body: { monthly_cap: 1_000 },
    });
    await reserve(t.deps.db, { userId: member.userId, feature: "rewrite", amount: 997, now: t.clock.now() });
    const bearer = await bearerFor(t.deps, member.userId);
    const ok = await json(t, "POST", "/v1/ai/rewrite", { bearer, body: { mode: "polish", text: "hello" } });
    expect(ok.status).toBe(200);
    const stopped = await json(t, "POST", "/v1/ai/rewrite", {
      bearer,
      body: { mode: "polish", text: "hello" },
    });
    expect(stopped.status).toBe(402);
    expect(await stopped.json()).toMatchObject({ code: "CREDITS_EXHAUSTED", cap: 1_000, spent: 1_000 });
    expect((await me(member.userId)).credits).toMatchObject({ cap: 1_000, spent: 1_000 });
    // The pool itself still has credits for the owner.
    const ownerCall = await json(t, "POST", "/v1/ai/rewrite", {
      bearer: await bearerFor(t.deps, owner.userId),
      body: { mode: "polish", text: "hello" },
    });
    expect(ownerCall.status).toBe(200);
  });

  it("F-BL-06.3 a seat change updates the Stripe quantity, and the webhook keeps seats in step", async () => {
    const owner = await teamOwner("own6@tandem.test", 3);
    const [sub] = await t.deps.db.select().from(subscriptions).where(eq(subscriptions.ownerId, owner.orgId));
    const res = await json(t, "PUT", `/v1/orgs/${owner.orgId}/seats`, { jar: owner.jar, body: { seats: 5 } });
    expect(res.status).toBe(200);
    expect(t.billing.calls).toContain(`quantity:${sub?.stripeSubscriptionId}:5`);
    // Stripe's own change (the portal, a fixture) arrives as subscription.updated.
    const hook = await postWebhook({
      id: "evt_seats_1",
      type: "customer.subscription.updated",
      created: t.clock.nowSec(),
      data: {
        object: {
          id: sub?.stripeSubscriptionId,
          status: "active",
          metadata: { owner_kind: "org", owner_id: owner.orgId, plan: "team", interval: "month" },
          items: {
            data: [
              {
                quantity: 7,
                current_period_start: t.clock.nowSec(),
                current_period_end: t.clock.nowSec() + 86_400,
              },
            ],
          },
        },
      },
    });
    expect(hook.status).toBe(200);
    const [after] = await t.deps.db
      .select()
      .from(subscriptions)
      .where(eq(subscriptions.ownerId, owner.orgId));
    expect(after?.seats).toBe(7);
    await join(owner, "m6@tandem.test");
    await join(owner, "n6@tandem.test");
    // Three seats are in use; the count can't drop below that.
    const tooFew = await json(t, "PUT", `/v1/orgs/${owner.orgId}/seats`, {
      jar: owner.jar,
      body: { seats: 2 },
    });
    expect(tooFew.status).toBe(409);
    expect(await tooFew.json()).toMatchObject({ code: "SEATS_IN_USE", used: 3 });
  });
});

describe("F-TL-10 audit log", () => {
  async function teamWithAudit(on: boolean) {
    const owner = await teamOwner(`own-audit-${on}@tandem.test`);
    const member = await join(owner, `m-audit-${on}@tandem.test`);
    await json(t, "PUT", `/v1/orgs/${owner.orgId}/policy`, {
      jar: owner.jar,
      body: { ...DEFAULT_POLICY, auditEnabled: on },
    });
    return { owner, member };
  }
  const fullAccess = {
    event: "full_access_used",
    meta: { engine: "claude", surface: "thread" },
    ts: new Date().toISOString(),
  };

  it("F-TL-10.1 a Team member with audit on records a Full-access start; a Pro user records nothing", async () => {
    const { owner, member } = await teamWithAudit(true);
    const res = await json(t, "POST", "/v1/audit/events", {
      bearer: await bearerFor(t.deps, member.userId),
      body: { events: [fullAccess, { event: "made_up", meta: {}, ts: fullAccess.ts }] },
    });
    expect(res.status).toBe(204);
    const rows = await t.deps.db.select().from(auditEvents).where(eq(auditEvents.orgId, owner.orgId));
    const client = rows.filter((r) => r.actorUserId === member.userId && r.event === "full_access_used");
    expect(client).toHaveLength(1);
    expect(client[0]?.meta).toMatchObject({ engine: "claude", surface: "thread" });
    expect(rows.some((r) => r.event === "made_up")).toBe(false);

    const { userId: pro } = await signUpVerified(t, "pro-audit@tandem.test");
    await setSubscription(t.deps, pro);
    const before = (await t.deps.db.select().from(auditEvents)).length;
    const proRes = await json(t, "POST", "/v1/audit/events", {
      bearer: await bearerFor(t.deps, pro),
      body: { events: [fullAccess] },
    });
    expect(proRes.status).toBe(204);
    expect((await t.deps.db.select().from(auditEvents)).length).toBe(before);
  });

  it("an org with audit off keeps nothing a member's app sends", async () => {
    const { owner, member } = await teamWithAudit(false);
    await json(t, "POST", "/v1/audit/events", {
      bearer: await bearerFor(t.deps, member.userId),
      body: { events: [fullAccess] },
    });
    const rows = await t.deps.db.select().from(auditEvents).where(eq(auditEvents.orgId, owner.orgId));
    expect(rows.some((r) => r.event === "full_access_used")).toBe(false);
  });

  it("metadata only: long text in meta is refused", async () => {
    const { member } = await teamWithAudit(true);
    const res = await json(t, "POST", "/v1/audit/events", {
      bearer: await bearerFor(t.deps, member.userId),
      body: { events: [{ ...fullAccess, meta: { prompt: "x".repeat(500) } }] },
    });
    expect(res.status).toBe(422);
  });

  it("the org log holds member and policy changes, for owners and admins only", async () => {
    const { owner, member } = await teamWithAudit(true);
    const log = await json(t, "GET", `/v1/orgs/${owner.orgId}/audit`, { jar: owner.jar });
    const events = ((await log.json()) as { events: Array<{ event: string }> }).events.map((e) => e.event);
    expect(events).toEqual(
      expect.arrayContaining(["org_created", "member_invited", "member_added", "policy_changed"]),
    );
    const denied = await json(t, "GET", `/v1/orgs/${owner.orgId}/audit`, { jar: member.jar });
    expect(denied.status).toBe(403);
  });
});

describe("F-BL-05 refunds", () => {
  async function admin() {
    const a = await signUpVerified(t, "admin@tandem.test");
    await t.deps.db.update(user).set({ role: "admin" }).where(eq(user.id, a.userId));
    return a;
  }

  async function paidPro(email: string) {
    const { userId } = await signUpVerified(t, email);
    await setSubscription(t.deps, userId, { firstPaidAt: t.clock.now() });
    const [sub] = await t.deps.db.select().from(subscriptions).where(eq(subscriptions.ownerId, userId));
    await postWebhook({
      id: `evt_paid_${email}`,
      type: "invoice.paid",
      created: t.clock.nowSec(),
      data: { object: { subscription: sub?.stripeSubscriptionId, amount_paid: 2900, lines: { data: [] } } },
    });
    return { userId, sub: sub?.stripeSubscriptionId ?? "" };
  }

  it("F-BL-05.1 an admin refund gives Free limits and a monthly bucket of 0", async () => {
    const a = await admin();
    const { userId, sub } = await paidPro("refund@tandem.test");
    expect((await me(userId)).plan).toBe("pro");
    const res = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: a.jar,
      body: { reason: "Asked within 14 days" },
    });
    expect(res.status).toBe(200);
    const view = await me(userId);
    expect(view.plan).toBe("free");
    expect(view.credits.monthly).toBe(0);
    expect(t.billing.calls).toEqual(expect.arrayContaining([`refund:${sub}`, `cancel:${sub}`]));
    // Stripe's later deletion event, with its original period end, doesn't bring Pro back.
    await postWebhook({
      id: "evt_deleted_after_refund",
      type: "customer.subscription.deleted",
      created: t.clock.nowSec(),
      data: {
        object: {
          id: sub,
          status: "canceled",
          metadata: { owner_kind: "user", owner_id: userId, plan: "pro", interval: "month" },
          items: {
            data: [
              {
                quantity: 1,
                current_period_start: t.clock.nowSec(),
                current_period_end: t.clock.nowSec() + 25 * 86_400,
              },
            ],
          },
        },
      },
    });
    expect((await me(userId)).plan).toBe("free");
    const again = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: a.jar,
      body: { reason: "twice" },
    });
    expect(again.status).toBe(409);
  });

  it("refunds need an admin, and a first charge over 14 days old needs force", async () => {
    const a = await admin();
    const { userId } = await paidPro("late@tandem.test");
    const notAdmin = await signUpVerified(t, "nosy@tandem.test");
    const denied = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: notAdmin.jar,
      body: { reason: "please" },
    });
    expect(denied.status).toBe(403);
    t.clock.advance(15 * 86_400_000);
    const late = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: a.jar,
      body: { reason: "late" },
    });
    expect(late.status).toBe(409);
    expect(await late.json()).toMatchObject({ code: "REFUND_WINDOW_CLOSED" });
    const forced = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: a.jar,
      body: { reason: "goodwill", force: true },
    });
    expect(forced.status).toBe(200);
  });

  it("an unused pack is refundable; a partly spent one isn't", async () => {
    const a = await admin();
    const { userId } = await signUpVerified(t, "packer@tandem.test");
    await setSubscription(t.deps, userId);
    for (const [id, session] of [
      ["evt_pack_a", "cs_pack_a"],
      ["evt_pack_b", "cs_pack_b"],
    ]) {
      await postWebhook({
        id,
        type: "checkout.session.completed",
        created: t.clock.nowSec(),
        data: {
          object: {
            id: session,
            mode: "payment",
            metadata: { owner_kind: "user", owner_id: userId, pack: "5k" },
          },
        },
      });
    }
    const ok = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: a.jar,
      body: { reason: "bought twice", scope: "pack" },
    });
    expect(ok.status).toBe(200);
    expect(t.billing.calls).toContain("refund_payment:cs_pack_b");
    expect((await balance(t.deps.db, { kind: "user", id: userId })).purchased).toBe(5_000);
    await reserve(t.deps.db, {
      userId,
      feature: "x",
      amount: 1,
      now: t.clock.now(),
      owner: { kind: "user", id: userId },
    });
    // Monthly was 0, so the reserve came out of the purchased pack.
    const used = await json(t, "POST", `/v1/admin/users/${userId}/refund`, {
      jar: a.jar,
      body: { reason: "again", scope: "pack" },
    });
    expect(used.status).toBe(409);
    expect(await used.json()).toMatchObject({ code: "PACK_USED" });
  });
});

describe("F-BL-03 credit packs", () => {
  it("F-BL-03.1 buying the 5,000 pack adds 5,000 to the purchased bucket exactly once", async () => {
    const buyer = await signUpVerified(t, "pack@tandem.test");
    await setSubscription(t.deps, buyer.userId);
    const res = await json(t, "POST", "/v1/billing/credit-packs/checkout", {
      jar: buyer.jar,
      body: { pack: "5k" },
    });
    expect(res.status).toBe(200);
    const sessionId = new URL(((await res.json()) as { url: string }).url).searchParams.get("session") ?? "";
    expect(t.billing.sessions.get(sessionId)).toMatchObject({ mode: "payment", metadata: { pack: "5k" } });
    const event = {
      id: "evt_pack_once",
      type: "checkout.session.completed",
      created: t.clock.nowSec(),
      data: {
        object: {
          id: sessionId,
          mode: "payment",
          metadata: { owner_kind: "user", owner_id: buyer.userId, pack: "5k" },
        },
      },
    };
    expect((await postWebhook(event)).status).toBe(200);
    expect((await postWebhook(event)).status).toBe(200);
    expect(await balance(t.deps.db, { kind: "user", id: buyer.userId })).toMatchObject({ purchased: 5_000 });
  });

  it("a Team admin buys packs for the org pool; a member is sent to the admin", async () => {
    const owner = await teamOwner("own-pack@tandem.test");
    const member = await join(owner, "m-pack@tandem.test");
    const res = await json(t, "POST", "/v1/billing/credit-packs/checkout", {
      jar: owner.jar,
      body: { pack: "12k" },
    });
    expect(res.status).toBe(200);
    const session = [...t.billing.sessions.values()].find((s) => s.mode === "payment");
    expect(session).toMatchObject({ ownerKind: "org", ownerId: owner.orgId });
    const denied = await json(t, "POST", "/v1/billing/credit-packs/checkout", {
      jar: member.jar,
      body: { pack: "5k" },
    });
    expect(denied.status).toBe(403);
    const members = await t.deps.db.select().from(orgMembers).where(eq(orgMembers.orgId, owner.orgId));
    expect(members).toHaveLength(2);
  });
});
