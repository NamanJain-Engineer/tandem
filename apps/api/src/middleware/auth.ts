import type { MiddlewareHandler } from "hono";
import { verifyAccessToken } from "../auth/jwt";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import type { AppEnv } from "./context";

/** Resolves a desktop bearer JWT or a better-auth cookie session into `c.var.user`. */
export function resolveUser(deps: Deps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    c.set("user", null);
    const header = c.req.header("authorization");
    const bearer = header?.startsWith("Bearer ") ? header.slice(7).trim() : undefined;
    c.set("bearerPresent", Boolean(bearer));
    if (bearer) {
      const claims = await verifyAccessToken(deps.keys, bearer, deps.clock.nowSec());
      if (claims) c.set("user", { id: claims.sub, via: "bearer", sessionFamily: claims.sid });
    } else if (c.req.header("cookie")?.includes("session_token")) {
      const session = await deps.auth.api.getSession({ headers: c.req.raw.headers }).catch(() => null);
      if (session?.user) c.set("user", { id: session.user.id, via: "cookie" });
    }
    await next();
  };
}

export type AuthKind = "bearer" | "cookie" | "any";

/** 401 UNAUTHENTICATED unless the request is signed in the allowed way. */
export function requireUser(kind: AuthKind = "any"): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const user = c.get("user");
    if (!user || (kind !== "any" && user.via !== kind)) {
      throw new ApiError(401, "UNAUTHENTICATED", "Unauthorized", "Sign in to continue.");
    }
    await next();
  };
}
