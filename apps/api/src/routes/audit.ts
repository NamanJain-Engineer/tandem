import { Hono } from "hono";
import type { Deps } from "../deps";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { clientAuditBody, recordClientAudit } from "../services/orgs";

/** Metadata-only audit events from members' apps (F-TL-10). Always 204; kept only when the org's audit is on. */
export function auditRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  app.post("/events", requireUser("bearer"), async (c) => {
    const body = await parseJson(c, clientAuditBody);
    await recordClientAudit(deps, c.get("user")?.id ?? "", body.events);
    return c.body(null, 204);
  });
  return app;
}
