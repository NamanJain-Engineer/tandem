import type { MiddlewareHandler } from "hono";
import { logger } from "../lib/logger";
import { redactHeaders } from "../lib/redact";
import type { AppEnv } from "./context";

export type ClientInfo = { app?: string; version?: string; platform?: string };

/** requestId → clientHeaders → accessLog (ARCHITECTURE §14.1 middleware order). */
export const requestContext: MiddlewareHandler<AppEnv> = async (c, next) => {
  const incoming = c.req.header("x-request-id");
  const requestId = incoming && /^[A-Za-z0-9._-]{1,64}$/.test(incoming) ? incoming : crypto.randomUUID();
  c.set("requestId", requestId);
  c.set("client", {
    app: c.req.header("x-tandem-app"),
    version: c.req.header("x-tandem-version"),
    platform: c.req.header("x-tandem-platform"),
  });
  const started = performance.now();
  await next();
  c.header("x-request-id", requestId);
  logger.info({
    requestId,
    method: c.req.method,
    route: c.req.routePath,
    status: c.res.status,
    ms: Math.round(performance.now() - started),
    client: c.get("client"),
    user: c.get("user")?.id,
    headers: redactHeaders(c.req.raw.headers),
  });
};
