import { Hono } from "hono";
import { bodyLimit } from "hono/body-limit";
import { ApiError, problemResponse } from "./lib/problem";
import { logger } from "./lib/logger";
import { redactText } from "./lib/redact";
import { requestContext, type AppVariables } from "./middleware/requestContext";
import { health } from "./routes/health";

export type Deps = Record<string, never>;

/** Builds the API. Every provider is injected so tests pass mocks (§14.1). */
export function createApp(_deps: Deps = {}) {
  const app = new Hono<{ Variables: AppVariables }>();

  app.use("*", requestContext);
  app.use(
    "*",
    bodyLimit({
      maxSize: 1024 * 1024,
      onError: (c) =>
        problemResponse(
          c,
          new ApiError(413, "PAYLOAD_TOO_LARGE", "Payload Too Large", "The request body is over 1 MB."),
        ),
    }),
  );

  app.route("/v1/health", health);

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
