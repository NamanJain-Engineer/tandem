import { Hono } from "hono";
import type { Deps } from "../deps";
import type { AppEnv } from "../middleware/context";

/** Signed remote config with a strong ETag; 304 on a match (F-BE-09). */
export function configRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  app.get("/", (c) => {
    const { payload, sig, kid, etag } = deps.config.signed;
    c.header("ETag", etag);
    c.header("Cache-Control", "public, max-age=600");
    if (c.req.header("if-none-match") === etag) return c.body(null, 304);
    return c.json({ payload, sig, kid });
  });
  return app;
}
