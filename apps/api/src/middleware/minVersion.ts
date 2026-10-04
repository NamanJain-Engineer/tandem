import type { MiddlewareHandler } from "hono";
import type { Deps } from "../deps";
import type { AppEnv } from "./context";

/** Every response names the oldest supported app version (F-BE-09). */
export function minVersion(deps: Deps): MiddlewareHandler<AppEnv> {
  return async (c, next) => {
    await next();
    c.header("x-tandem-min-version", deps.config.minAppVersion);
  };
}
