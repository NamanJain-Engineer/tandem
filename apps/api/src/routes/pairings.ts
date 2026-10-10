import { Hono } from "hono";
import type { Deps } from "../deps";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { createPairing, createPairingBody, revokePairing } from "../services/pairings";

/** Phone pairings (F-RM-03, ARCHITECTURE §13.3). Desktop bearer only. */
export function pairingRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/", requireUser("bearer"), async (c) => {
    const body = await parseJson(c, createPairingBody);
    return c.json(await createPairing(deps, c.get("user")?.id ?? "", body));
  });

  app.delete("/:id", requireUser("bearer"), async (c) => {
    await revokePairing(deps, c.get("user")?.id ?? "", c.req.param("id"));
    return c.body(null, 204);
  });

  return app;
}
