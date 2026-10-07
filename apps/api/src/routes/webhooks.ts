import { Hono } from "hono";
import type { Deps } from "../deps";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import type { AppEnv } from "../middleware/context";
import { handleEvent } from "../services/webhooks";

/** Stripe webhooks: accepted by signature from any IP (F-BE-10 AC2). */
export function webhookRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  app.post("/stripe", async (c) => {
    const raw = await c.req.text();
    let event;
    try {
      event = deps.billing.constructEvent(raw, c.req.header("stripe-signature"));
    } catch {
      throw new ApiError(400, "BAD_SIGNATURE", "Bad Request", "The webhook signature didn't verify.");
    }
    const outcome = await handleEvent(deps, event);
    logger.info({ type: event.type, id: event.id, applied: outcome.applied }, "webhook");
    return c.json({ received: true });
  });
  return app;
}
