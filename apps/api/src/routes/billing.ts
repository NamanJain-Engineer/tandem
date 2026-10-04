import { eq } from "drizzle-orm";
import { Hono } from "hono";
import { z } from "zod";
import { CREDIT_PACKS, PRICES } from "@tandem/protocol/plans";
import type { Deps } from "../deps";
import { subscriptions, user } from "../db/schema";
import { newId } from "../lib/ids";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { MockBilling } from "../providers/billing/mock";
import type { BillingEvent } from "../providers/billing/types";
import { planFor } from "../services/entitlements";
import { customerFor, handleEvent } from "../services/webhooks";

export const checkoutBody = z.object({
  plan: z.enum(["pro", "team"]),
  interval: z.enum(["month", "year"]),
  seats: z.number().int().min(2).max(500).optional(),
  promo: z.string().max(64).optional(),
});
export const packBody = z.object({ pack: z.enum(["5k", "12k", "30k"]) });

function priceFor(deps: Deps, plan: "pro" | "team", interval: "month" | "year"): string {
  const e = deps.env;
  if (plan === "pro") return interval === "month" ? e.STRIPE_PRICE_PRO_MONTH : e.STRIPE_PRICE_PRO_YEAR;
  return interval === "month" ? e.STRIPE_PRICE_TEAM_MONTH : e.STRIPE_PRICE_TEAM_YEAR;
}

function packPrice(deps: Deps, pack: keyof typeof CREDIT_PACKS): string {
  return {
    "5k": deps.env.STRIPE_PRICE_PACK_5K,
    "12k": deps.env.STRIPE_PRICE_PACK_12K,
    "30k": deps.env.STRIPE_PRICE_PACK_30K,
  }[pack];
}

async function emailOf(deps: Deps, userId: string): Promise<string> {
  const [u] = await deps.db.select({ email: user.email }).from(user).where(eq(user.id, userId)).limit(1);
  if (!u) throw new ApiError(401, "UNAUTHENTICATED", "Unauthorized");
  return u.email;
}

/** Synthetic Stripe-shaped events for mock checkout and the mock portal. */
function event(type: string, object: Record<string, unknown>, nowSec: number): BillingEvent {
  return { id: newId("evt_mock"), type, created: nowSec, data: { object } };
}

export function billingRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/checkout", requireUser(), async (c) => {
    const body = await parseJson(c, checkoutBody);
    const userId = c.get("user")?.id ?? "";
    const owner = { kind: "user" as const, id: userId };
    const quantity =
      body.plan === "team" ? Math.max(PRICES.team.minSeats, body.seats ?? PRICES.team.minSeats) : 1;
    const session = await deps.billing.createCheckout({
      ownerKind: owner.kind,
      ownerId: owner.id,
      email: await emailOf(deps, userId),
      customerId: (await customerFor(deps, owner)) ?? undefined,
      priceId: priceFor(deps, body.plan, body.interval),
      quantity,
      trialDays: PRICES[body.plan].trialDays,
      promo: body.promo,
      successUrl: `${deps.env.WEB_PUBLIC_URL}/billing/success?session_id={CHECKOUT_SESSION_ID}`,
      cancelUrl: `${deps.env.WEB_PUBLIC_URL}/billing`,
      mode: "subscription",
      metadata: { owner_kind: owner.kind, owner_id: owner.id, plan: body.plan, interval: body.interval },
    });
    return c.json({ url: session.url });
  });

  app.post("/portal", requireUser(), async (c) => {
    const customer = await customerFor(deps, { kind: "user", id: c.get("user")?.id ?? "" });
    if (!customer) throw new ApiError(404, "NOT_FOUND", "Not Found", "There's no billing account yet.");
    return c.json(await deps.billing.createPortal(customer, `${deps.env.WEB_PUBLIC_URL}/billing`));
  });

  app.post("/credit-packs/checkout", requireUser(), async (c) => {
    const body = await parseJson(c, packBody);
    const userId = c.get("user")?.id ?? "";
    const { effective } = await planFor(deps, userId);
    if (!effective.limits.creditPacks) {
      throw new ApiError(403, "FORBIDDEN_PLAN", "Forbidden", "Credit packs are included with Pro.", {
        feature: "creditPacks",
      });
    }
    const owner = { kind: "user" as const, id: userId };
    const session = await deps.billing.createCheckout({
      ownerKind: owner.kind,
      ownerId: owner.id,
      email: await emailOf(deps, userId),
      customerId: (await customerFor(deps, owner)) ?? undefined,
      priceId: packPrice(deps, body.pack),
      quantity: 1,
      successUrl: `${deps.env.WEB_PUBLIC_URL}/credits?purchased=${body.pack}`,
      cancelUrl: `${deps.env.WEB_PUBLIC_URL}/credits`,
      mode: "payment",
      metadata: { owner_kind: owner.kind, owner_id: owner.id, pack: body.pack },
    });
    return c.json({ url: session.url });
  });

  // Dev only: BILLING_PROVIDER=mock completes checkout instantly (F-BL-01).
  app.post("/mock/complete", requireUser("cookie"), async (c) => {
    const billing = deps.billing;
    if (!(billing instanceof MockBilling)) throw new ApiError(404, "NOT_FOUND", "Not Found");
    const { sessionId } = await parseJson(c, z.object({ sessionId: z.string() }));
    const pending = billing.sessions.get(sessionId);
    if (!pending || pending.ownerId !== c.get("user")?.id)
      throw new ApiError(404, "NOT_FOUND", "Not Found", "Unknown checkout session.");
    billing.sessions.delete(sessionId);
    const now = deps.clock.nowSec();
    const customer = pending.customerId ?? newId("cus_mock");
    const meta = pending.metadata;
    await handleEvent(
      deps,
      event(
        "checkout.session.completed",
        { id: sessionId, mode: pending.mode, customer, metadata: meta },
        now,
      ),
    );
    if (pending.mode === "subscription") {
      const subId = newId("sub_mock");
      const trialEnd = now + (pending.trialDays ?? 0) * 86_400;
      const periodEnd = now + (meta.interval === "year" ? 365 : 30) * 86_400;
      await handleEvent(
        deps,
        event(
          "customer.subscription.created",
          {
            id: subId,
            customer,
            status: pending.trialDays ? "trialing" : "active",
            metadata: meta,
            items: {
              data: [
                { quantity: pending.quantity, current_period_start: now, current_period_end: periodEnd },
              ],
            },
            cancel_at_period_end: false,
            trial_end: pending.trialDays ? trialEnd : null,
          },
          now,
        ),
      );
      await handleEvent(
        deps,
        event(
          "invoice.paid",
          {
            id: newId("in_mock"),
            customer,
            subscription: subId,
            amount_paid: 0,
            lines: { data: [{ period: { end: periodEnd } }] },
          },
          now,
        ),
      );
    }
    return c.body(null, 204);
  });

  // Dev only: the mock billing portal's actions.
  app.post("/mock/portal", requireUser("cookie"), async (c) => {
    if (!(deps.billing instanceof MockBilling)) throw new ApiError(404, "NOT_FOUND", "Not Found");
    const { action } = await parseJson(
      c,
      z.object({ action: z.enum(["cancel_at_period_end", "resume", "cancel_now"]) }),
    );
    const userId = c.get("user")?.id ?? "";
    const [sub] = await deps.db
      .select()
      .from(subscriptions)
      .where(eq(subscriptions.ownerId, userId))
      .limit(1);
    if (!sub) throw new ApiError(404, "NOT_FOUND", "Not Found", "There's no subscription.");
    const customer = (await customerFor(deps, { kind: "user", id: userId })) ?? undefined;
    const now = deps.clock.nowSec();
    const object = {
      id: sub.stripeSubscriptionId,
      customer,
      status: action === "cancel_now" ? "canceled" : sub.status,
      metadata: { owner_kind: "user", owner_id: userId, plan: sub.plan, interval: sub.interval },
      items: {
        data: [
          {
            quantity: sub.seats,
            current_period_start: Math.floor(sub.currentPeriodStart.getTime() / 1000),
            current_period_end: Math.floor(sub.currentPeriodEnd.getTime() / 1000),
          },
        ],
      },
      cancel_at_period_end: action === "cancel_at_period_end",
      trial_end: sub.trialEnd ? Math.floor(sub.trialEnd.getTime() / 1000) : null,
    };
    await handleEvent(
      deps,
      event(
        action === "cancel_now" ? "customer.subscription.deleted" : "customer.subscription.updated",
        object,
        now,
      ),
    );
    return c.body(null, 204);
  });

  return app;
}
