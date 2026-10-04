import type { MiddlewareHandler } from "hono";
import type { Limits } from "@tandem/protocol/plans";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import { planFor } from "../services/entitlements";
import type { AppEnv } from "./context";

export type GatedFeature = { [K in keyof Limits]: Limits[K] extends boolean ? K : never }[keyof Limits];

/**
 * Server-side entitlement check: the plan is recomputed from the database on
 * every call, so a client claiming "pro" changes nothing (F-BE-04 AC4).
 */
export function requirePlan(deps: Deps, feature: GatedFeature): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const user = c.get("user");
    if (!user) throw new ApiError(401, "UNAUTHENTICATED", "Unauthorized", "Sign in to continue.");
    const { effective } = await planFor(deps, user.id);
    if (!effective.limits[feature]) {
      throw new ApiError(403, "FORBIDDEN_PLAN", "Forbidden", "This feature is included with Pro.", {
        feature,
        plan: effective.plan,
      });
    }
    await next();
  };
}
