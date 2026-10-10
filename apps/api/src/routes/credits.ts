import { Hono } from "hono";
import type { Deps } from "../deps";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { creditsFor } from "../services/credits";
import { recentLedger } from "../services/ledger";

export function creditsRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  app.get("/", requireUser(), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const view = await creditsFor(deps, userId);
    const owner = view.org
      ? { kind: "org" as const, id: view.org.id }
      : { kind: "user" as const, id: userId };
    const ledger = await recentLedger(deps.db, owner, 100);
    const { org, ...rest } = view;
    return c.json({ balance: rest, org, ledger });
  });
  return app;
}
