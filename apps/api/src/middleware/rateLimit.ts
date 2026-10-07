import type { MiddlewareHandler } from "hono";
import { getConnInfo } from "@hono/node-server/conninfo";
import type { Deps } from "../deps";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import { take } from "../services/rateLimit";
import type { AppEnv } from "./context";

export const IP_CAPACITY = 300;
export const IP_WINDOW_MS = 60_000;
export const AUTH_SPIKE_PER_MINUTE = 20;

export function clientIp(c: Parameters<MiddlewareHandler<AppEnv>>[0]): string {
  const forwarded = c.req.header("x-forwarded-for")?.split(",")[0]?.trim();
  if (forwarded) return forwarded;
  try {
    return getConnInfo(c).remote.address ?? "unknown";
  } catch {
    return "unknown";
  }
}

/** 300 requests / 60 s per IP. Health, webhooks and test-only routes are excluded (ARCHITECTURE §14.9). */
export function ipRateLimit(deps: Deps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const path = c.req.path;
    if (path === "/v1/health" || path.startsWith("/v1/webhooks/") || path.startsWith("/__test/"))
      return next();
    const result = await take(
      deps.redis,
      `rl:ip:${clientIp(c)}`,
      IP_CAPACITY,
      IP_WINDOW_MS,
      deps.clock.nowMs(),
    );
    if (!result.allowed) {
      c.header("Retry-After", String(Math.ceil(result.retryAfterMs / 1000)));
      throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Slow down and try again shortly.");
    }
    await next();
  };
}

/**
 * Counts 401/403/429 responses to authenticated clients per minute and logs
 * `alert=auth_spike` past the threshold (F-BE-10).
 */
export function authSpikeMonitor(deps: Deps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    await next();
    const status = c.res.status;
    if (!(status === 401 || status === 403 || status === 429)) return;
    const authed = c.get("bearerPresent") || Boolean(c.get("user"));
    if (!authed) return;
    const minute = Math.floor(deps.clock.nowMs() / 60_000);
    const key = `metrics:authfail:${minute}`;
    const count = await deps.redis.incr(key);
    if (count === 1) await deps.redis.expire(key, 120);
    if (count === AUTH_SPIKE_PER_MINUTE + 1) {
      logger.warn({ alert: "auth_spike", minute, count }, "authenticated clients are being rejected");
    }
  };
}
