import { Hono } from "hono";
import type { Deps } from "../deps";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { requirePlan } from "../middleware/requirePlan";
import { syncBody, syncDictionary } from "../services/dictionary";

/** Dictionary sync (F-DI-09, ARCHITECTURE §13). Pro, desktop bearer only. */
export function dictionaryRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/sync", requireUser("bearer"), requirePlan(deps, "dictionarySync"), async (c) => {
    const body = await parseJson(c, syncBody);
    return c.json(await syncDictionary(deps, c.get("user")?.id ?? "", body));
  });

  return app;
}
