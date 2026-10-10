import { eq } from "drizzle-orm";
import { Hono, type MiddlewareHandler } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { user } from "../db/schema";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import type { AppEnv } from "../middleware/context";
import { grantCredits, searchUsers, userDetail } from "../services/admin";
import { refundPack, refundPlan } from "../services/refunds";

export const refundBody = z.object({
  reason: z.string().trim().min(3).max(500),
  scope: z.enum(["plan", "pack"]).default("plan"),
  force: z.boolean().default(false),
});

/** Admin-only routes: a portal cookie session whose user has role `admin`. */
export function requireAdmin(deps: Deps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const u = c.get("user");
    if (!u || u.via !== "cookie")
      throw new ApiError(401, "UNAUTHENTICATED", "Unauthorized", "Sign in to continue.");
    const [row] = await deps.db.select({ role: user.role }).from(user).where(eq(user.id, u.id)).limit(1);
    if (row?.role !== "admin")
      throw new ApiError(403, "FORBIDDEN", "Forbidden", "This needs an admin account.");
    await next();
  };
}

export const grantBody = z.object({
  amount: z.number().int().min(1).max(1_000_000),
  reason: z.string().trim().min(3).max(500),
  bucket: z.enum(["monthly", "purchased"]).default("purchased"),
});

/** Support tools (F-BE-12, F-BL-05). */
export function adminRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  app.use("*", requireAdmin(deps));

  app.get("/users", async (c) => c.json({ users: await searchUsers(deps, c.req.query("q") ?? "") }));

  app.get("/users/:id", async (c) => c.json(await userDetail(deps, c.req.param("id"))));

  app.post("/users/:id/credits", async (c) => {
    const body = await parseJson(c, grantBody);
    return c.json(
      await grantCredits(deps, c.get("user")?.id ?? "", c.req.param("id"), body.amount, body.reason, body.bucket),
    );
  });

  app.post("/users/:id/refund", async (c) => {
    const body = await parseJson(c, refundBody);
    const adminId = c.get("user")?.id ?? "";
    const target = c.req.param("id");
    const [exists] = await deps.db.select({ id: user.id }).from(user).where(eq(user.id, target)).limit(1);
    if (!exists) throw new ApiError(404, "NOT_FOUND", "Not Found", "There's no such user.");
    const out =
      body.scope === "pack"
        ? await refundPack(deps, adminId, target, body.reason)
        : await refundPlan(deps, adminId, target, body.reason, body.force);
    return c.json(out);
  });

  return app;
}
