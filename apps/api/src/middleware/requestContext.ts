import type { MiddlewareHandler } from "hono";
import { logger } from "../lib/logger";
import { redactHeaders } from "../lib/redact";

export type ClientInfo = { app?: string; version?: string; platform?: string };
export type AppVariables = { requestId: string; client: ClientInfo };

/** requestId → clientHeaders → accessLog (ARCHITECTURE §14.1 middleware order). */
export const requestContext: MiddlewareHandler<{ Variables: AppVariables }> = async (c, next) => {
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
    headers: redactHeaders(c.req.raw.headers),
  });
};
