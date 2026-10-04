import { Hono } from "hono";
import type { Deps } from "../deps";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { balance, recentLedger } from "../services/ledger";

export function creditsRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  app.get("/", requireUser(), async (c) => {
    const owner = { kind: "user" as const, id: c.get("user")?.id ?? "" };
    const [b, ledger] = await Promise.all([balance(deps.db, owner), recentLedger(deps.db, owner, 100)]);
    return c.json({ balance: b, ledger });
  });
  return app;
}
