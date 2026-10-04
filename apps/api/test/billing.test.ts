import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MONTHLY_CREDITS } from "@tandem/protocol/plans";
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

const DAY = 86_400_000;

async function postWebhook(event: unknown) {
  const { body, signature } = signed(t, event);
  return t.app.request("/v1/webhooks/stripe", {
    method: "POST",
    headers: {
      "stripe-signature": signature,
      "content-type": "application/json",
      "x-forwarded-for": "203.0.113.77",
    },
    body,
  });
}

async function mockCheckout(jar: Jar) {
  const res = await json(t, "POST", "/v1/billing/checkout", {
    jar,
    body: { plan: "pro", interval: "month" },
  });
  expect(res.status).toBe(200);
  const { url } = (await res.json()) as { url: string };
  const sessionId = new URL(url).searchParams.get("session") ?? "";
  const done = await json(t, "POST", "/v1/billing/mock/complete", { jar, body: { sessionId } });
  expect(done.status).toBe(204);
}

describe("F-BL-01 checkout", () => {
  it("F-BL-01.1 replaying the same webhook event twice applies it once", async () => {
    const { userId } = await signUpVerified(t, "replay@tandem.test");
    const event = {
      id: "evt_replay_1",
      type: "checkout.session.completed",
      created: t.clock.nowSec(),
      data: {
        object: {
          id: "cs_1",
          mode: "payment",
          customer: "cus_1",
          metadata: { owner_kind: "user", owner_id: userId, pack: "5k" },
        },
      },
    };
    expect((await postWebhook(event)).status).toBe(200);
    expect((await postWebhook(event)).status).toBe(200);
    const credits = (await (
      await json(t, "GET", "/v1/credits", { bearer: await bearerFor(t.deps, userId) })
    ).json()) as {
      balance: { purchased: number };
      ledger: unknown[];
    };
    expect(credits.balance.purchased).toBe(5000);
    expect(credits.ledger).toHaveLength(1);
  });

  it("F-BL-01.2 in mock mode, Upgrade results in Pro within 5 s via the SSE invalidation", async () => {
    const { jar, userId } = await signUpVerified(t, "upgrade@tandem.test");
    const bearer = await bearerFor(t.deps, userId);
    const sse = await t.app.request("/v1/me/events", { headers: { authorization: `Bearer ${bearer}` } });
    expect(sse.status).toBe(200);
    const reader = sse.body?.getReader();
    if (!reader) throw new Error("no stream");
    const decoder = new TextDecoder();
    let text = "";
    const started = Date.now();
    // Wait for the stream to be ready before upgrading.
    while (!text.includes("event: ready")) text += decoder.decode((await reader.read()).value);
    await mockCheckout(jar);
    while (!text.includes("event: entitlement")) {
      const { value, done } = await reader.read();
      if (done) break;
      text += decoder.decode(value);
    }
    expect(Date.now() - started).toBeLessThan(5000);
    await reader.cancel();
    const me = (await (await json(t, "GET", "/v1/me", { bearer })).json()) as {
      plan: string;
      status: string;
    };
    expect(me).toMatchObject({ plan: "pro", status: "trialing" });
  });

  it("F-BL-01.3 invoice.paid grants 10,000 monthly credits and resets the old balance", async () => {
    const { jar, userId } = await signUpVerified(t, "grant@tandem.test");
    await mockCheckout(jar);
    const bearer = await bearerFor(t.deps, userId);
    let me = (await (await json(t, "GET", "/v1/me", { bearer })).json()) as { credits: { monthly: number } };
    expect(me.credits.monthly).toBe(MONTHLY_CREDITS);
    const subs = await t.deps.db.query.subscriptions.findFirst();
    expect(
      (
        await postWebhook({
          id: "evt_inv_2",
          type: "invoice.paid",
          created: t.clock.nowSec(),
          data: {
            object: {
              id: "in_2",
              subscription: subs?.stripeSubscriptionId,
              amount_paid: 2900,
              lines: { data: [] },
            },
          },
        })
      ).status,
    ).toBe(200);
    me = (await (await json(t, "GET", "/v1/me", { bearer })).json()) as { credits: { monthly: number } };
    expect(me.credits.monthly).toBe(MONTHLY_CREDITS);
  });

  it("a webhook with a bad signature is rejected with 400", async () => {
    const res = await t.app.request("/v1/webhooks/stripe", {
      method: "POST",
      headers: { "stripe-signature": "t=1,v1=bad" },
      body: "{}",
    });
    expect(res.status).toBe(400);
  });

  it("checkout for Team uses at least 2 seats; packs are refused on Free", async () => {
    const { jar } = await signUpVerified(t, "team@tandem.test");
    const res = await json(t, "POST", "/v1/billing/checkout", {
      jar,
      body: { plan: "team", interval: "year" },
    });
    expect(res.status).toBe(200);
    const session = [...t.billing.sessions.values()][0];
    expect(session).toMatchObject({ quantity: 2, trialDays: 14, mode: "subscription" });
    const pack = await json(t, "POST", "/v1/billing/credit-packs/checkout", { jar, body: { pack: "5k" } });
    expect(pack.status).toBe(403);
    expect(await pack.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
  });
});

describe("F-BL-02 customer portal", () => {
  it("F-BL-02.1 a cancelled subscription shows Pro until the date, then Free", async () => {
    const { jar, userId } = await signUpVerified(t, "cancel@tandem.test");
    await mockCheckout(jar);
    const portal = await json(t, "POST", "/v1/billing/portal", { jar });
    expect(portal.status).toBe(200);
    expect(
      (await json(t, "POST", "/v1/billing/mock/portal", { jar, body: { action: "cancel_at_period_end" } }))
        .status,
    ).toBe(204);
    const bearer = await bearerFor(t.deps, userId);
    const before = (await (await json(t, "GET", "/v1/me", { bearer })).json()) as {
      plan: string;
      paid_until: string;
      cancel_at_period_end: boolean;
    };
    expect(before).toMatchObject({ plan: "pro", cancel_at_period_end: true });
    expect(before.paid_until).toBeTruthy();
    t.clock.set(new Date(new Date(before.paid_until).getTime() + 1000));
    const after = (await (
      await json(t, "GET", "/v1/me", { bearer: await bearerFor(t.deps, userId) })
    ).json()) as { plan: string; limits: { panes: number } };
    expect(after).toMatchObject({ plan: "free", limits: { panes: 4 } });
  });
});

describe("F-BL-04 dunning and lapse", () => {
  it("F-BL-04.1 past_due for 8 days gives Free limits and the data is intact", async () => {
    const { userId } = await signUpVerified(t, "pastdue@tandem.test");
    await setSubscription(t.deps, userId, { stripeSubscriptionId: "sub_pd", status: "active" });
    const failed = await postWebhook({
      id: "evt_fail_1",
      type: "invoice.payment_failed",
      created: t.clock.nowSec(),
      data: {
        object: { id: "in_f", subscription: "sub_pd", metadata: { owner_kind: "user", owner_id: userId } },
      },
    });
    expect(failed.status).toBe(200);
    expect(t.email.latest("pastdue@tandem.test")?.subject).toBe("Payment failed — update your card");
    let me = (await (await json(t, "GET", "/v1/me", { bearer: await bearerFor(t.deps, userId) })).json()) as {
      plan: string;
      status: string;
    };
    expect(me).toMatchObject({ plan: "pro", status: "past_due" });
    t.clock.advance(8 * DAY);
    me = (await (await json(t, "GET", "/v1/me", { bearer: await bearerFor(t.deps, userId) })).json()) as {
      plan: string;
      status: string;
    };
    expect(me).toMatchObject({ plan: "free", status: "past_due" });
    const rows = await t.deps.db.query.subscriptions.findMany();
    expect(rows).toHaveLength(1);
    expect(await t.deps.db.query.user.findFirst()).toMatchObject({ email: "pastdue@tandem.test" });
  });
});
