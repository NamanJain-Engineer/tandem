import { Hono } from "hono";
import { z } from "zod";
import type { Health } from "@tandem/protocol";
import { logger } from "./log";
import { problem } from "./problem";
import { redactHeaders, redactText } from "./redact";

const VERSION = "0.1.0";

const healthSchema = z.object({
  ok: z.literal(true),
  version: z.string(),
});

type Variables = {
  requestId: string;
};

export function createApp() {
  const app = new Hono<{ Variables: Variables }>();

  app.use("*", async (c, next) => {
    const requestId = c.req.header("x-request-id") ?? crypto.randomUUID();
    c.set("requestId", requestId);
    c.header("x-request-id", requestId);
    const started = Date.now();
    await next();
    logger.info({
      requestId,
      method: c.req.method,
      path: c.req.path,
      status: c.res.status,
      ms: Date.now() - started,
      client: {
        app: c.req.header("x-tandem-app"),
        version: c.req.header("x-tandem-version"),
        platform: c.req.header("x-tandem-platform"),
      },
      headers: redactHeaders(c.req.raw.headers),
    });
  });

  app.get("/v1/health", (c) => {
    const body: Health = { ok: true, version: VERSION };
    const parsed = healthSchema.parse(body);
    return c.json(parsed);
  });

  app.notFound((c) => c.json(problem(404, "Not Found", `${c.req.method} ${c.req.path} was not found`), 404));

  app.onError((err, c) => {
    logger.error({ requestId: c.get("requestId"), err: redactText(err.message) });
    return c.json(problem(500, "Internal Server Error", "The request failed."), 500);
  });

  return app;
}
