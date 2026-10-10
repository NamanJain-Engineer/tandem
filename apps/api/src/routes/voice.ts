import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { requirePlan } from "../middleware/requirePlan";
import { voiceResolve } from "../services/ai";
import { planFor } from "../services/entitlements";
import { policyOf } from "../services/orgs";
import { type FEATURE_LIMITS, takeForUser } from "../services/rateLimit";
import { act, MAX_CALL_SECONDS, mintSession, settleSession } from "../services/voice";

const bytes = (text: string) => Buffer.byteLength(text, "utf8");

export const voiceSessionBody = z.object({
  voice: z
    .string()
    .regex(/^[a-z0-9][a-z0-9_-]{0,39}$/i, "A voice is a name of letters, digits, - and _.")
    .optional(),
});

export const settleBody = z.object({ seconds: z.number().int().min(0).max(MAX_CALL_SECONDS) });

export const actBody = z.object({
  session_id: z.string().min(1).max(100),
  request: z.string().min(1).max(2000),
  snapshot: z
    .record(z.string(), z.unknown())
    .refine((s) => bytes(JSON.stringify(s)) <= 64 * 1024, { message: "The snapshot is over 64 KB." }),
  results: z
    .array(
      z.object({
        id: z.string().min(1).max(300),
        ok: z.boolean(),
        output: z
          .string()
          .refine((o) => bytes(o) <= 8 * 1024, { message: "A result's output is over 8 KB." }),
      }),
    )
    .max(64)
    .optional(),
});

export const resolveBody = z.object({
  query: z.string().min(1).max(500),
  candidates: z
    .array(
      z.object({
        id: z.string().min(1).max(200),
        kind: z.string().min(1).max(40),
        text: z.string().max(2000),
      }),
    )
    .max(200),
});

/** Conductor (F-BE-08, F-CN-02, F-CN-05; ARCHITECTURE §14.6). Desktop bearer; Pro or Team, except settling. */
export function voiceRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  // A Team admin can turn Conductor off for the org (F-BL-06); the server holds the line too.
  const allowed = async (userId: string) => {
    const { effective, membership } = await planFor(deps, userId);
    if (
      membership &&
      effective.plan === "team" &&
      (await policyOf(deps, membership.orgId)).disableConductor
    ) {
      throw new ApiError(403, "FORBIDDEN_POLICY", "Forbidden", "Your team's admin turned Conductor off.", {
        feature: "conductor",
      });
    }
  };

  const limit = async (
    userId: string,
    feature: keyof typeof FEATURE_LIMITS,
    c: { header: (k: string, v: string) => void },
  ) => {
    const r = await takeForUser(deps.redis, userId, feature, deps.clock.nowMs());
    if (!r.allowed) {
      c.header("Retry-After", String(Math.ceil(r.retryAfterMs / 1000)));
      throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Slow down and try again shortly.");
    }
  };

  app.post("/session", requireUser("bearer"), requirePlan(deps, "conductor"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, voiceSessionBody);
    await allowed(userId);
    await limit(userId, "voice_session", c);
    return c.json(await mintSession(deps, userId, body.voice));
  });

  // Never gated by plan or policy: a call that loses either still settles, and its hold comes back.
  app.post("/session/:id/settle", requireUser("bearer"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, settleBody);
    return c.json(await settleSession(deps, userId, c.req.param("id"), body.seconds));
  });

  app.post("/act", requireUser("bearer"), requirePlan(deps, "conductor"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, actBody);
    await allowed(userId);
    await limit(userId, "voice_act", c);
    return c.json(await act(deps, userId, body));
  });

  app.post("/resolve", requireUser("bearer"), requirePlan(deps, "conductor"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, resolveBody);
    await allowed(userId);
    await limit(userId, "voice_resolve", c);
    return c.json({ matches: await voiceResolve(deps, userId, body) });
  });

  return app;
}
