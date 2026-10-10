// `/__test/*`: clock, plan, team, credits, audit and mail helpers for E2E tests. Mounted only
// with TANDEM_API_TEST_ROUTES=1 and never with NODE_ENV=production.
import { and, eq } from "drizzle-orm";
import { DEFAULT_POLICY } from "@tandem/protocol/team";
import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import {
  auditEvents,
  creditBuckets,
  creditLedger,
  creditReservations,
  orgMembers,
  orgPolicies,
  orgs,
  subscriptions,
  user,
} from "../db/schema";
import { MockBilling } from "../providers/billing/mock";
import { notifyCredits } from "../services/credits";
import { grantMonthly, type Owner } from "../services/ledger";
import { creditOwnerOf, membershipOf } from "../services/membership";
import { createOrg, policySchema } from "../services/orgs";
import { markVerified } from "../services/sso";
import { mockIdpCode, mockIdpNext } from "../providers/sso/mock";
import { orgSso, orgSsoDomains } from "../db/schema";
import { newId } from "../lib/ids";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import type { AppEnv } from "../middleware/context";
import { MemoryEmail } from "../providers/email/smtp";

const MAILPIT = process.env.MAILPIT_URL ?? "http://localhost:8025";

async function userIdFor(deps: Deps, email: string): Promise<string> {
  const [u] = await deps.db.select({ id: user.id }).from(user).where(eq(user.email, email)).limit(1);
  if (!u) throw new ApiError(404, "NOT_FOUND", "Not Found", `No user ${email}`);
  return u.id;
}

export function testRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/clock", async (c) => {
    const body = await parseJson(
      c,
      z.object({
        set: z.string().optional(),
        advanceMs: z.number().optional(),
        reset: z.boolean().optional(),
      }),
    );
    if (body.reset) deps.clock.reset();
    if (body.set) deps.clock.set(new Date(body.set));
    if (body.advanceMs) deps.clock.advance(body.advanceMs);
    return c.json({ now: deps.clock.now().toISOString() });
  });

  // Engine registry overrides (E2E-M4-2, E2E-M4-7) and the minimum app
  // version every response names (E2E-M5-7) in the signed config.
  app.post("/config", async (c) => {
    const body = await parseJson(
      c,
      z.object({
        registry_overrides: z.array(z.record(z.string(), z.unknown())).optional(),
        min_app_version: z
          .string()
          .regex(/^\d+\.\d+\.\d+$/)
          .optional(),
      }),
    );
    if (body.registry_overrides) deps.config.setRegistryOverrides(body.registry_overrides);
    if (body.min_app_version) deps.config.setMinAppVersion(body.min_app_version);
    return c.json({ etag: deps.config.signed.etag, minAppVersion: deps.config.minAppVersion });
  });

  app.post("/plan", async (c) => {
    const body = await parseJson(
      c,
      z.object({
        email: z.string(),
        plan: z.enum(["free", "pro", "team"]),
        status: z.enum(["trialing", "active", "past_due", "canceled"]).default("active"),
        periodEndDays: z.number().default(30),
        cancelAtPeriodEnd: z.boolean().default(false),
        /** The first charge happened now (refunds need one, F-BL-05). */
        paid: z.boolean().default(false),
      }),
    );
    const userId = await userIdFor(deps, body.email);
    await deps.db.delete(subscriptions).where(eq(subscriptions.ownerId, userId));
    if (body.plan !== "free") {
      const now = deps.clock.now();
      await deps.db.insert(subscriptions).values({
        id: newId("sub"),
        stripeSubscriptionId: newId("sub_test"),
        ownerKind: "user",
        ownerId: userId,
        plan: body.plan,
        interval: "month",
        status: body.status,
        seats: 1,
        currentPeriodStart: now,
        currentPeriodEnd: new Date(now.getTime() + body.periodEndDays * 86_400_000),
        cancelAtPeriodEnd: body.cancelAtPeriodEnd,
        pastDueSince: body.status === "past_due" ? now : null,
        firstPaidAt: body.paid ? now : null,
        updatedAt: now,
      });
    }
    await deps.events.publish(userId, "entitlement");
    return c.body(null, 204);
  });

  app.get("/subscriptions", async (c) => {
    const userId = await userIdFor(deps, c.req.query("email") ?? "");
    const rows = await deps.db
      .select({ id: subscriptions.id })
      .from(subscriptions)
      .where(eq(subscriptions.ownerId, userId));
    return c.json({ count: rows.length });
  });

  app.post("/user/delete", async (c) => {
    const body = await parseJson(c, z.object({ email: z.string() }));
    await deps.db.delete(user).where(eq(user.email, body.email));
    return c.body(null, 204);
  });

  app.get("/mail/latest", async (c) => {
    const to = c.req.query("to") ?? "";
    if (deps.email instanceof MemoryEmail) {
      const mail = deps.email.latest(to);
      if (!mail) throw new ApiError(404, "NOT_FOUND", "Not Found", "No mail yet.");
      return c.json(mail);
    }
    const res = await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:${to}`)}&limit=1`);
    const list = (await res.json()) as { messages?: Array<{ ID: string }> };
    const id = list.messages?.[0]?.ID;
    if (!id) throw new ApiError(404, "NOT_FOUND", "Not Found", "No mail yet.");
    const msg = (await (await fetch(`${MAILPIT}/api/v1/message/${id}`)).json()) as {
      Subject: string;
      Text: string;
    };
    return c.json({ to, subject: msg.Subject, text: msg.Text });
  });

  // M13: a Team org in one call, balances, audit rows and what the billing mock was asked.
  app.post("/team", async (c) => {
    const body = await parseJson(
      c,
      z.object({
        owner: z.string(),
        members: z.array(z.string()).default([]),
        seats: z.number().int().min(2).default(3),
        policy: policySchema.partial().optional(),
        credits: z.number().int().min(0).default(-1),
      }),
    );
    const ownerId = await userIdFor(deps, body.owner);
    const existing = await membershipOf(deps.db, ownerId);
    if (existing) await deps.db.delete(orgs).where(eq(orgs.id, existing.orgId));
    const orgId = await createOrg(deps, ownerId, "Test team");
    for (const email of body.members) {
      const id = await userIdFor(deps, email);
      await deps.db.delete(orgMembers).where(eq(orgMembers.userId, id));
      await deps.db.insert(orgMembers).values({ orgId, userId: id, role: "member" });
    }
    const now = deps.clock.now();
    const end = new Date(now.getTime() + 30 * 86_400_000);
    const subId = newId("sub_test");
    await deps.db.insert(subscriptions).values({
      id: newId("sub"),
      stripeSubscriptionId: subId,
      ownerKind: "org",
      ownerId: orgId,
      plan: "team",
      interval: "month",
      status: "active",
      seats: body.seats,
      currentPeriodStart: now,
      currentPeriodEnd: end,
      firstPaidAt: now,
      updatedAt: now,
    });
    await deps.db.transaction((tx) =>
      grantMonthly(tx, { kind: "org", id: orgId }, body.seats, end, `test_${subId}`),
    );
    if (body.credits >= 0) await setBalance(deps, { kind: "org", id: orgId }, body.credits, 0);
    if (body.policy) {
      await deps.db
        .update(orgPolicies)
        .set({ policy: { ...DEFAULT_POLICY, ...body.policy }, updatedAt: now })
        .where(eq(orgPolicies.orgId, orgId));
    }
    for (const id of [ownerId, ...(await Promise.all(body.members.map((e) => userIdFor(deps, e))))]) {
      await deps.events.publish(id, "entitlement");
    }
    return c.json({ orgId, subscription: subId });
  });

  app.post("/credits", async (c) => {
    const body = await parseJson(
      c,
      z.object({
        email: z.string(),
        monthly: z.number().int().min(0),
        purchased: z.number().int().min(0).default(0),
      }),
    );
    const userId = await userIdFor(deps, body.email);
    const owner = await creditOwnerOf(deps.db, userId);
    await setBalance(deps, owner, body.monthly, body.purchased);
    await notifyCredits(deps, owner);
    return c.body(null, 204);
  });

  // Team SSO (F-BL-06 P2): the owner's org signs in through the test
  // identity provider for `domain`, verified without DNS.
  app.post("/sso", async (c) => {
    const body = await parseJson(
      c,
      z.object({ email: z.string(), domain: z.string().toLowerCase(), enforce: z.boolean().default(false) }),
    );
    const orgId = (await membershipOf(deps.db, await userIdFor(deps, body.email)))?.orgId;
    if (!orgId) throw new ApiError(404, "NOT_FOUND", "Not Found", "That user has no org.");
    await deps.db.delete(orgSsoDomains).where(eq(orgSsoDomains.domain, body.domain));
    await deps.db.insert(orgSsoDomains).values({ orgId, domain: body.domain, verifyToken: "test" });
    await markVerified(deps, orgId, body.domain);
    const values = { provider: "mock" as const, enforce: body.enforce, updatedAt: deps.clock.now() };
    await deps.db.insert(orgSso).values({ orgId, ...values }).onConflictDoUpdate({ target: orgSso.orgId, set: values });
    return c.json({ orgId });
  });

  // The test identity provider: who signs in next, and its authorize step.
  app.post("/idp/next", async (c) => {
    const body = await parseJson(c, z.object({ email: z.string(), name: z.string().nullable().default(null) }));
    await mockIdpNext(deps.redis, { subject: `mock-${body.email}`, email: body.email.toLowerCase(), name: body.name });
    return c.body(null, 204);
  });

  app.get("/idp/authorize", async (c) => {
    const redirect = new URL(c.req.query("redirect_uri") ?? "");
    redirect.searchParams.set("state", c.req.query("state") ?? "");
    const code = await mockIdpCode(deps.redis);
    if (code) redirect.searchParams.set("code", code);
    else redirect.searchParams.set("error", "access_denied");
    return c.redirect(redirect.toString(), 302);
  });

  // Support tools need an admin (F-BL-05): a fresh E2E account becomes one.
  app.post("/admin", async (c) => {
    const body = await parseJson(c, z.object({ email: z.string() }));
    await deps.db.update(user).set({ role: "admin" }).where(eq(user.id, await userIdFor(deps, body.email)));
    return c.body(null, 204);
  });

  app.get("/user", async (c) => c.json({ id: await userIdFor(deps, c.req.query("email") ?? "") }));

  app.get("/billing-calls", (c) => {
    const calls = deps.billing instanceof MockBilling ? deps.billing.calls : [];
    return c.json({ calls });
  });

  app.get("/audit", async (c) => {
    const userId = await userIdFor(deps, c.req.query("email") ?? "");
    const m = await membershipOf(deps.db, userId);
    const rows = m
      ? await deps.db.select().from(auditEvents).where(eq(auditEvents.orgId, m.orgId))
      : await deps.db.select().from(auditEvents).where(eq(auditEvents.actorUserId, userId));
    return c.json({ events: rows.map((r) => ({ event: r.event, actor: r.actorUserId, meta: r.meta })) });
  });

  app.get("/reservations", async (c) => {
    const userId = await userIdFor(deps, c.req.query("email") ?? "");
    const rows = await deps.db.select().from(creditReservations).where(eq(creditReservations.userId, userId));
    return c.json({
      reservations: rows.map((r) => ({
        feature: r.feature,
        amount: r.amount,
        state: r.state,
        settled: r.settledAmount,
      })),
    });
  });

  return app;
}

/** Sets an owner's buckets to exact balances, through the ledger (`admin_grant`). */
async function setBalance(deps: Deps, owner: Owner, monthly: number, purchased: number): Promise<void> {
  await deps.db.transaction(async (tx) => {
    for (const [kind, target] of [
      ["monthly", monthly],
      ["purchased", purchased],
    ] as const) {
      await tx
        .insert(creditBuckets)
        .values({ id: newId("cb"), ownerKind: owner.kind, ownerId: owner.id, kind, balance: 0 })
        .onConflictDoNothing();
      const [b] = await tx
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
      if (!b || b.balance === target) continue;
      await tx.insert(creditLedger).values({
        id: newId("cl"),
        bucketId: b.id,
        delta: target - b.balance,
        reason: "admin_grant",
        note: "test route",
      });
      await tx.update(creditBuckets).set({ balance: target }).where(eq(creditBuckets.id, b.id));
    }
  });
}
