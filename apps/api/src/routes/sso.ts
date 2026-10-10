import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { parseJson } from "../lib/validate";
import type { AppEnv } from "../middleware/context";
import { ssoLookup, startSso } from "../services/sso";

export const ssoStartBody = z.object({
  email: z.string().email().max(254),
  /** A path on the web app to land on afterwards. */
  callbackURL: z.string().max(500).optional(),
});
export const ssoLookupBody = z.object({ email: z.string().email().max(254) });

/** Team SSO's first leg (F-BL-06 P2). Public: the sign-in page calls it. */
export function ssoRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/lookup", async (c) => {
    const body = await parseJson(c, ssoLookupBody);
    return c.json(await ssoLookup(deps, body.email));
  });

  app.post("/start", async (c) => {
    const body = await parseJson(c, ssoStartBody);
    return c.json(await startSso(deps, body.email, body.callbackURL));
  });

  return app;
}
