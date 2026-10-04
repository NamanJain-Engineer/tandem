import type { MiddlewareHandler } from "hono";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import type { AppEnv } from "./context";
import { clientIp } from "./rateLimit";

const CHALLENGED = new Set([
  "/v1/auth/sign-up/email",
  "/v1/auth/sign-in/email",
  "/v1/auth/sign-in/magic-link",
  "/v1/auth/forget-password",
  "/v1/auth/request-password-reset",
]);

/**
 * Bot checks apply to web sign-up and sign-in only. A request with a valid
 * bearer token is never challenged (F-BE-10 AC1).
 */
export function turnstile(deps: Deps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    if (c.req.method !== "POST" || !CHALLENGED.has(c.req.path) || c.get("user")?.via === "bearer")
      return next();
    const token = c.req.header("cf-turnstile-response");
    if (!(await deps.captcha.verify(token, clientIp(c)))) {
      throw new ApiError(403, "CAPTCHA_FAILED", "Forbidden", "Complete the check and try again.");
    }
    await next();
  };
}
