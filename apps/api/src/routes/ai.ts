import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { requirePlan } from "../middleware/requirePlan";
import { rewrite, teammateDraft, title } from "../services/ai";
import { takeForUser } from "../services/rateLimit";

export const titleBody = z.object({ text: z.string().min(1).max(4000) });
export const rewriteBody = z.object({
  mode: z.enum(["enhance", "polish"]),
  text: z.string().min(1).max(5000),
  instructions: z.string().max(2000).optional(),
  protected_terms: z.array(z.string().max(100)).max(200).optional(),
});

export const draftBody = z.object({
  description: z.string().min(1).max(2000),
  engines: z.array(z.string().min(1).max(40)).min(1).max(20),
  plugins: z.array(z.string().min(1).max(60)).max(100).default([]),
});

/** Hosted AI on the Claude API (F-BE-06, ARCHITECTURE §14.6). Desktop bearer only. */
export function aiRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  const limit = async (
    userId: string,
    feature: "title" | "rewrite" | "teammate_draft",
    c: { header: (k: string, v: string) => void },
  ) => {
    const r = await takeForUser(deps.redis, userId, feature, deps.clock.nowMs());
    if (!r.allowed) {
      c.header("Retry-After", String(Math.ceil(r.retryAfterMs / 1000)));
      throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Slow down and try again shortly.");
    }
  };

  app.post("/title", requireUser("bearer"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, titleBody);
    await limit(userId, "title", c);
    return c.json({ title: await title(deps, userId, body.text) });
  });

  app.post("/rewrite", requireUser("bearer"), requirePlan(deps, "enhance"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, rewriteBody);
    await limit(userId, "rewrite", c);
    const text = await rewrite(deps, userId, {
      mode: body.mode,
      text: body.text,
      instructions: body.instructions,
      protectedTerms: body.protected_terms,
    });
    return c.json({ text });
  });

  // "Create with AI" (F-AG-04). Writing features share the `enhance` entitlement (D-603).
  app.post("/teammate-draft", requireUser("bearer"), requirePlan(deps, "enhance"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const body = await parseJson(c, draftBody);
    await limit(userId, "teammate_draft", c);
    return c.json(await teammateDraft(deps, userId, body));
  });

  return app;
}
