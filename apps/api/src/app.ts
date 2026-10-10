import { Hono } from "hono";
import { bodyLimit } from "hono/body-limit";
import type { Deps } from "./deps";
import { testRoutesEnabled } from "./env";
import { logger } from "./lib/logger";
import { ApiError, problemResponse } from "./lib/problem";
import { redactText } from "./lib/redact";
import { resolveUser } from "./middleware/auth";
import type { AppEnv } from "./middleware/context";
import { csrf } from "./middleware/csrf";
import { minVersion } from "./middleware/minVersion";
import { authSpikeMonitor, ipRateLimit } from "./middleware/rateLimit";
import { requestContext } from "./middleware/requestContext";
import { turnstile } from "./middleware/turnstile";
import { adminRoutes } from "./routes/admin";
import { aiRoutes } from "./routes/ai";
import { autoRoutes } from "./routes/auto";
import { auditRoutes } from "./routes/audit";
import { billingRoutes } from "./routes/billing";
import { configRoutes } from "./routes/config";
import { creditsRoutes } from "./routes/credits";
import { desktopAuthRoutes } from "./routes/desktopAuth";
import { health } from "./routes/health";
import { meRoutes } from "./routes/me";
import { orgRoutes } from "./routes/orgs";
import { sttRoutes } from "./routes/stt";
import { testRoutes } from "./routes/testRoutes";
import { webhookRoutes } from "./routes/webhooks";

/**
 * Builds the API. Middleware order follows ARCHITECTURE §14.1: request id and
 * client headers → access log → problems → rate limit → auth → route guards.
 */
export function createApp(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.use("*", requestContext);
  app.use("*", minVersion(deps));
  const jsonLimit = bodyLimit({
    maxSize: 1024 * 1024,
    onError: (c) =>
      problemResponse(
        c,
        new ApiError(413, "PAYLOAD_TOO_LARGE", "Payload Too Large", "The request body is over 1 MB."),
      ),
  });
  // Audio uploads stream through the STT relay under its own 25 MB limit.
  app.use("*", (c, next) => (c.req.path === "/v1/stt/transcribe" ? next() : jsonLimit(c, next)));
  app.use("*", resolveUser(deps));
  app.use("*", authSpikeMonitor(deps));
  app.use("*", ipRateLimit(deps));
  app.use("*", csrf(deps.env.WEB_PUBLIC_URL.startsWith("https://")));
  app.use("*", turnstile(deps));

  app.route("/v1/health", health);
  app.route("/v1/config", configRoutes(deps));
  app.route("/v1/auth/desktop", desktopAuthRoutes(deps));
  app.on(["GET", "POST"], "/v1/auth/*", (c) => deps.auth.handler(c.req.raw));
  app.route("/v1/me", meRoutes(deps));
  app.route("/v1/credits", creditsRoutes(deps));
  app.route("/v1/billing", billingRoutes(deps));
  app.route("/v1/ai", aiRoutes(deps));
  app.route("/v1/auto", autoRoutes(deps));
  app.route("/v1/stt", sttRoutes(deps));
  app.route("/v1/orgs", orgRoutes(deps));
  app.route("/v1/audit", auditRoutes(deps));
  app.route("/v1/admin", adminRoutes(deps));
  app.route("/v1/webhooks", webhookRoutes(deps));
  if (testRoutesEnabled(deps.env)) app.route("/__test", testRoutes(deps));

  app.notFound((c) =>
    problemResponse(
      c,
      new ApiError(404, "NOT_FOUND", "Not Found", `${c.req.method} ${c.req.path} was not found.`),
    ),
  );
  app.onError((err, c) => {
    if (err instanceof ApiError) return problemResponse(c, err);
    logger.error({ requestId: c.get("requestId"), err: redactText(err.message) }, "unhandled error");
    return problemResponse(c, new ApiError(500, "INTERNAL", "Internal Server Error", "The request failed."));
  });

  return app;
}
