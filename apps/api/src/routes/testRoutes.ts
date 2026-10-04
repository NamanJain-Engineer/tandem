// `/__test/*`: clock, plan, credits and mail helpers for E2E tests. Mounted only
// with TANDEM_API_TEST_ROUTES=1 and never with NODE_ENV=production.
import { eq } from "drizzle-orm";
import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { subscriptions, user } from "../db/schema";
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

  app.post("/plan", async (c) => {
    const body = await parseJson(
      c,
      z.object({
        email: z.string(),
        plan: z.enum(["free", "pro", "team"]),
        status: z.enum(["trialing", "active", "past_due", "canceled"]).default("active"),
        periodEndDays: z.number().default(30),
        cancelAtPeriodEnd: z.boolean().default(false),
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

  return app;
}
