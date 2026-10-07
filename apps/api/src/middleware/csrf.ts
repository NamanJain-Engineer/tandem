import type { MiddlewareHandler } from "hono";
import { getCookie, setCookie } from "hono/cookie";
import { randomToken } from "../lib/ids";
import { ApiError } from "../lib/problem";
import type { AppEnv } from "./context";

export const CSRF_COOKIE = "tandem_csrf";
export const CSRF_HEADER = "x-csrf-token";
const WRITES = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/**
 * Double-submit CSRF (ARCHITECTURE §14.3): every response carries a readable
 * `tandem_csrf` cookie; cookie-authenticated writes must echo it in
 * `x-csrf-token`. Bearer requests and requests without a session cookie are
 * exempt (webhooks pass by signature).
 */
export function csrf(secure: boolean): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    const cookieToken = getCookie(c, CSRF_COOKIE);
    const hasSession = (c.req.header("cookie") ?? "").includes("session_token");
    if (WRITES.has(c.req.method) && hasSession && !c.get("bearerPresent")) {
      const header = c.req.header(CSRF_HEADER);
      if (!cookieToken || !header || header !== cookieToken) {
        throw new ApiError(
          403,
          "FORBIDDEN_CSRF",
          "Forbidden",
          "The request is missing its CSRF token. Reload the page and try again.",
        );
      }
    }
    await next();
    if (!cookieToken) {
      setCookie(c, CSRF_COOKIE, randomToken(24), { path: "/", sameSite: "Lax", secure, httpOnly: false });
    }
  };
}
