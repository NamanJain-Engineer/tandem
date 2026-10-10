import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { requirePlan } from "../middleware/requirePlan";
import { AUTO_MODES, autoNext } from "../services/ai";
import { planFor } from "../services/entitlements";
import { policyOf } from "../services/orgs";
import { takeForUser } from "../services/rateLimit";

export const autoNextBody = z
  .object({
    mode: z.enum(AUTO_MODES),
    goal: z.string().max(2000).optional(),
    turn: z.number().int().min(0).max(30),
    ceiling: z.number().int().min(1).max(30),
    scrollback: z.string().max(16_384),
  })
  .refine((b) => b.turn < b.ceiling, { message: "The turn ceiling is reached.", path: ["turn"] })
  .refine((b) => b.mode !== "custom" || (b.goal ?? "").trim().length > 0, {
    message: "A custom Auto run needs a goal.",
    path: ["goal"],
  });

/** Auto's supervisor (F-AU-01, ARCHITECTURE §8.8). Desktop bearer, Pro or Team. */
export function autoRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/next", requireUser("bearer"), requirePlan(deps, "auto"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, autoNextBody);
    // A Team admin can turn Auto off for the org (F-BL-06); the server holds the line too.
    const { effective, membership } = await planFor(deps, userId);
    if (membership && effective.plan === "team" && (await policyOf(deps, membership.orgId)).disableAuto) {
      throw new ApiError(403, "FORBIDDEN_POLICY", "Forbidden", "Your team's admin turned Auto off.", {
        feature: "auto",
      });
    }
    const r = await takeForUser(deps.redis, userId, "auto", deps.clock.nowMs());
    if (!r.allowed) {
      c.header("Retry-After", String(Math.ceil(r.retryAfterMs / 1000)));
      throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Slow down and try again shortly.");
    }
    return c.json(await autoNext(deps, userId, body));
  });

  return app;
}
